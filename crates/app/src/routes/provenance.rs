//! Where a work's text lives, in one word, decided here rather than in the interface (M71-04).
//!
//! The catalogue is two pools and always has been: works whose text this instance holds, and works it knows about and
//! points at. A reader scanning a list cannot tell them apart unless the payload says so, and a badge coined in the UI
//! would be a second vocabulary that drifts from the first.
//!
//! # The four words
//!
//! * **`hosted`** — the instance holds a readable unit. The reader can start now.
//! * **`hosted_locked`** — the text exists here and this reader cannot open it: a restricted work whose record is public
//!   by design (M70-04) and whose body needs a session. Saying `hosted` would promise a chapter the link cannot deliver.
//! * **`referenced`** — no text here, but a catalogue or library row with a source URL: this is a pointer, and the
//!   pointer is what the reader gets.
//! * **`known`** — a work this instance can describe and has no text and no source for: an import that arrived as
//!   metadata only, or a stub an author has not begun.
//!
//! Four words, not three. The brief proposed three and then discovered the fourth in a footnote; a restricted work is
//! exactly the case that needs it, because it is hosted and unreadable at the same time and collapsing either half
//! makes the badge lie in a different direction.
//!
//! # Cost
//!
//! One grouped query per list, not one per row: the caller collects its ids and asks once. A fifty-row list costs one
//! statement with fifty placeholders, which is the shape SQLite and PostgreSQL both handle without a plan surprise.

use std::collections::HashMap;

use crate::state::AppState;

/// Where a work's text lives, from the point of view of the reader asking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Provenance {
    Hosted,
    HostedLocked,
    Referenced,
    Known,
}

impl Provenance {
    /// The wire word. Lowercase, stable, and the only spelling any surface may use.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Provenance::Hosted => "hosted",
            Provenance::HostedLocked => "hosted_locked",
            Provenance::Referenced => "referenced",
            Provenance::Known => "known",
        }
    }
}

/// The provenance of every work in `ids`, in one round trip.
///
/// Ids that name nothing are absent from the map rather than defaulted to `known`: a caller that receives no entry has
/// asked about a work that does not exist, which is a different fact from a work with no text.
pub(crate) async fn for_work_ids(
    state: &AppState,
    ids: &[String],
) -> HashMap<String, Provenance> {
    let mut out = HashMap::with_capacity(ids.len());
    if ids.is_empty() {
        return out;
    }

    // Placeholders are generated per dialect: `?` for SQLite, `$n` for PostgreSQL. Neither is interpolated from user
    // input — only the count matters — but the two strings differ, so both are built here.
    let sqlite_marks = vec!["?"; ids.len()].join(",");
    let pg_marks = (1..=ids.len())
        .map(|n| format!("${n}"))
        .collect::<Vec<_>>()
        .join(",");

    let sqlite = format!(
        "SELECT w.id, w.visibility, \
                EXISTS(SELECT 1 FROM units u WHERE u.work_id = w.id AND u.current_revision_id IS NOT NULL) \
                    AS readable, \
                EXISTS(SELECT 1 FROM library_items li WHERE li.work_id = w.id \
                        AND COALESCE(li.source_url, '') <> '') AS sourced \
           FROM works w WHERE w.id IN ({sqlite_marks})"
    );
    let postgres = format!(
        "SELECT w.id::text, w.visibility, \
                EXISTS(SELECT 1 FROM units u WHERE u.work_id = w.id AND u.current_revision_id IS NOT NULL), \
                EXISTS(SELECT 1 FROM library_items li WHERE li.work_id = w.id \
                        AND COALESCE(li.source_url, '') <> '') \
           FROM works w WHERE w.id::text IN ({pg_marks})"
    );

    use darkhold_db::Backend;
    let rows: Vec<(String, String, bool, bool)> = match state.db().backend() {
        Backend::Sqlite => {
            let mut query = sqlx::query_as::<_, (String, String, bool, bool)>(&sqlite);
            for id in ids {
                query = query.bind(id);
            }
            match query.fetch_all(state.db().sqlite_pool().expect("sqlite")).await {
                Ok(rows) => rows,
                Err(error) => {
                    tracing::warn!(%error, "provenance lookup failed; reporting known");
                    return out;
                }
            }
        }
        Backend::Postgres => {
            let mut query = sqlx::query_as::<_, (String, String, bool, bool)>(&postgres);
            for id in ids {
                query = query.bind(id);
            }
            match query
                .fetch_all(state.db().postgres_pool().expect("postgres"))
                .await
            {
                Ok(rows) => rows,
                Err(error) => {
                    tracing::warn!(%error, "provenance lookup failed; reporting known");
                    return out;
                }
            }
        }
    };

    for (id, visibility, readable, sourced) in rows {
        let word = if readable {
            Provenance::Hosted
        } else if visibility == "restricted" {
            // Restricted without a readable unit is still the locked case: the record is public, the text is not.
            Provenance::HostedLocked
        } else if sourced {
            Provenance::Referenced
        } else {
            Provenance::Known
        };
        out.insert(id, word);
    }
    out
}

/// The ids a JSON list names, for the one query.
pub(crate) fn ids_in(items: &[serde_json::Value], field: &str) -> Vec<String> {
    items
        .iter()
        .filter_map(|item| item.get(field).and_then(|v| v.as_str()).map(str::to_owned))
        .collect()
}

/// Write the word into each item. Missing entries (a work that is not in the database) are left alone.
pub(crate) fn attach(
    items: &mut [serde_json::Value],
    field: &str,
    map: &HashMap<String, Provenance>,
) {
    for item in items.iter_mut() {
        let Some(id) = item.get(field).and_then(|v| v.as_str()).map(str::to_owned) else {
            continue;
        };
        if let (Some(word), Some(object)) = (map.get(&id), item.as_object_mut()) {
            object.insert(
                "provenance".to_owned(),
                serde_json::Value::String(word.as_str().to_owned()),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Provenance;

    #[test]
    fn the_four_words_are_lowercase_and_distinct() {
        let words = [
            Provenance::Hosted.as_str(),
            Provenance::HostedLocked.as_str(),
            Provenance::Referenced.as_str(),
            Provenance::Known.as_str(),
        ];
        assert_eq!(words, ["hosted", "hosted_locked", "referenced", "known"]);
        let unique: std::collections::HashSet<_> = words.iter().collect();
        assert_eq!(unique.len(), 4, "a vocabulary with repeats is not one");
    }
}

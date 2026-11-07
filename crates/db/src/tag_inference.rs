//! Inferring tags for imported works, and measuring whether the catalogue is tagged at all (M63-32).
//!
//! The card offers two routes: backfill from the source metadata, or a tag-inference pass. The first is not available *here* — the
//! importer stores `library_items.provenance_json` and no category field at all, so there is nothing to backfill from without
//! re-fetching every source page, which is a network operation and not a code change. This is the second route.
//!
//! **What it does:** for a work with no tags whose summary or title names an existing taxonomy node, record a *proposal* — through
//! `tag_assertions::assert_tag` with `Claimant::Model` and `asserted_by = "text-mention-pass"`. That claimant is chosen deliberately:
//! a string match is not the source's own metadata (`imported`) and not a person (`curator`), and M69-17 already guarantees a model
//! row is stored `inferred + unconfirmed` and therefore **invisible to readers** until a human or community assertion confirms it. A
//! wrong inference costs nothing anybody sees; a confirmed one is a real tag with a real claimant.
//!
//! **What it refuses to do:** guess. It matches existing `canonical_entities` of kind `tag` against the text already stored, so it
//! can only propose a node the instance already has, and it never writes a confirmation.
//!
//! The coverage report is the other half of the card, and it is the answer to "can this instance rank by fandom at all": per source,
//! how many items are tagged, how many carry likes, and how many carry both. Those two numbers are what made `--sort likes` order
//! ties in the first place.

use crate::tag_assertions::{assert_tag, Claimant};
use crate::{Backend, Database, Result};
use serde::Serialize;

/// What one inference pass did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct InferenceReport {
    /// Works examined (untagged, with text to read).
    pub examined: i64,
    /// Proposals written. Each is invisible to readers until confirmed.
    pub proposed: i64,
}

/// One source's coverage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceCoverage {
    /// `pawchive`, `ao3`, `sb`, `sv`, `qq`, `ffnet`, …
    pub source_key: String,
    /// Items imported from it.
    pub items: i64,
    /// Items whose work carries at least one tag.
    pub tagged: i64,
    /// Items carrying an imported endorsement count.
    pub with_likes: i64,
    /// Items carrying both — the only rows a fandom ranking can order by popularity honestly.
    pub tagged_and_liked: i64,
}

/// Per-source tagging coverage. Sorted by source key so two reads agree.
pub async fn coverage(db: &Database) -> Result<Vec<SourceCoverage>> {
    let sql = db.sql(
        "SELECT li.source_key,
                COUNT(*) AS items,
                SUM(CASE WHEN EXISTS (SELECT 1 FROM work_tags wt WHERE wt.work_id = li.work_id
                            AND NOT (wt.source = 'inferred' AND wt.confirmation = 'unconfirmed')) THEN 1 ELSE 0 END) AS tagged,
                SUM(CASE WHEN li.likes > 0 THEN 1 ELSE 0 END) AS with_likes,
                SUM(CASE WHEN li.likes > 0
                          AND EXISTS (SELECT 1 FROM work_tags wt WHERE wt.work_id = li.work_id
                            AND NOT (wt.source = 'inferred' AND wt.confirmation = 'unconfirmed'))
                         THEN 1 ELSE 0 END) AS tagged_and_liked
           FROM library_items li
          WHERE li.work_id IS NOT NULL
          GROUP BY li.source_key
          ORDER BY li.source_key",
        "SELECT li.source_key,
                COUNT(*)::bigint AS items,
                SUM(CASE WHEN EXISTS (SELECT 1 FROM work_tags wt WHERE wt.work_id = li.work_id
                            AND NOT (wt.source = 'inferred' AND wt.confirmation = 'unconfirmed')) THEN 1 ELSE 0 END)::bigint AS tagged,
                SUM(CASE WHEN li.likes > 0 THEN 1 ELSE 0 END)::bigint AS with_likes,
                SUM(CASE WHEN li.likes > 0
                          AND EXISTS (SELECT 1 FROM work_tags wt WHERE wt.work_id = li.work_id
                            AND NOT (wt.source = 'inferred' AND wt.confirmation = 'unconfirmed'))
                         THEN 1 ELSE 0 END)::bigint AS tagged_and_liked
           FROM library_items li
          WHERE li.work_id IS NOT NULL
          GROUP BY li.source_key
          ORDER BY li.source_key",
    );
    let rows: Vec<(String, i64, i64, i64, i64)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(rows
        .into_iter()
        .map(|(source_key, items, tagged, with_likes, tagged_and_liked)| SourceCoverage {
            source_key,
            items,
            tagged,
            with_likes,
            tagged_and_liked,
        })
        .collect())
}

/// One untagged work with text to read.
struct Candidate {
    work_id: String,
    text: String,
}

/// Run one inference pass over up to `limit` untagged imported works.
///
/// `limit` because this is meant to be run repeatedly by an operator watching the numbers, not fired once over a million rows: a pass
/// that proposes ten thousand invisible tags at once is a table nobody can review.
pub async fn propose_from_text(db: &Database, limit: i64) -> Result<InferenceReport> {
    // A tag node's canonical text, longest first: "hurt comfort" must win over "comfort" when both appear, so the pass does not
    // propose the shorter node merely because it was inserted earlier.
    let nodes = tag_nodes(db).await?;
    if nodes.is_empty() {
        return Ok(InferenceReport {
            examined: 0,
            proposed: 0,
        });
    }
    let candidates = untagged_texts(db, limit).await?;
    let mut proposed = 0;
    for candidate in &candidates {
        let haystack = candidate.text.to_lowercase();
        // Per work, per node: a proposal is written once and `assert_tag` upserts, so a second pass over the same work is a no-op
        // rather than a second row.
        let mut matched: Vec<&(String, String)> = nodes
            .iter()
            .filter(|(_, canonical)| haystack.contains(&canonical.to_lowercase()))
            .collect();
        matched.sort_by_key(|(_, canonical)| std::cmp::Reverse(canonical.len()));
        // A node whose text is contained in a longer matched node's text rides along on the same words: a summary saying "hurt
        // comfort" matches both "hurt comfort" and "comfort", and proposing both gives a work one tag and one fragment of it. The
        // longer node is the one the reader means; the shorter is kept only when nothing longer covered it.
        let mut kept: Vec<&(String, String)> = Vec::new();
        for candidate in matched {
            let canonical = candidate.1.to_lowercase();
            let subsumed = kept
                .iter()
                .any(|(_, longer)| longer.to_lowercase().contains(&canonical));
            if !subsumed {
                kept.push(candidate);
            }
        }
        for (node_id, _) in kept {
            assert_tag(
                db,
                &candidate.work_id,
                node_id,
                Claimant::Model,
                false,
                0.5,
            )
            .await?;
            proposed += 1;
        }
    }
    Ok(InferenceReport {
        examined: candidates.len() as i64,
        proposed,
    })
}

async fn tag_nodes(db: &Database) -> Result<Vec<(String, String)>> {
    let sql = db.sql(
        "SELECT id, canonical FROM canonical_entities WHERE kind = 'tag' ORDER BY LENGTH(canonical) DESC, id",
        "SELECT id, canonical FROM canonical_entities WHERE kind = 'tag' ORDER BY LENGTH(canonical) DESC, id",
    );
    let rows: Vec<(String, String)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(rows)
}

async fn untagged_texts(db: &Database, limit: i64) -> Result<Vec<Candidate>> {
    let sql = db.sql(
        "SELECT w.id, w.title || ' ' || COALESCE(w.summary, '') || ' ' || COALESCE(li.summary, '')
           FROM works w
           JOIN library_items li ON li.work_id = w.id
          WHERE NOT EXISTS (SELECT 1 FROM work_tags wt WHERE wt.work_id = w.id)
          ORDER BY w.id
          LIMIT ?1",
        "SELECT w.id, w.title || ' ' || COALESCE(w.summary, '') || ' ' || COALESCE(li.summary, '')
           FROM works w
           JOIN library_items li ON li.work_id = w.id
          WHERE NOT EXISTS (SELECT 1 FROM work_tags wt WHERE wt.work_id = w.id)
          ORDER BY w.id
          LIMIT $1",
    );
    let rows: Vec<(String, String)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(limit)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(limit)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(rows
        .into_iter()
        .map(|(work_id, text)| Candidate { work_id, text })
        .collect())
}

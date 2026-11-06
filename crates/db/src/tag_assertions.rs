//! Who asserted a tag, and whether it is an assertion at all (M69-17).
//!
//! **No new columns.** `work_tags` already carries the provenance (M6-04, migration 0126): `source` is one of
//! `author | curator | imported | exchanged | inferred`, and `confirmation` is one of `unconfirmed | reader | wrangler |
//! inaccurate` (migration 0099). The requirement's four claimants map onto that vocabulary, and inventing a second one — a
//! `claimant` column beside `source` — would give the tree two answers to one question and a way for them to disagree.
//!
//! | requirement  | stored as                                              |
//! |--------------|--------------------------------------------------------|
//! | human        | `source = 'curator'` (the work's own author is `author`, written by the authoring path) |
//! | source       | `source = 'imported'`                                  |
//! | model        | `source = 'inferred'`, `confirmation = 'unconfirmed'`  |
//! | community    | `source = 'exchanged'`, `confirmation = 'reader'`      |
//!
//! The rule the whole card exists for: **a model may only propose.** A model's assertion is `inferred` and always lands as
//! `unconfirmed`, and `taxonomy::tag_names_for_work` — the reader-facing read — hides exactly that pair and nothing else. It is
//! hidden by the *combination*, not by `unconfirmed` alone: `unconfirmed` is the default for the entire catalogue, so filtering
//! on it would hide every tag on the instance. Only an inference nobody has confirmed is a guess; an inference a human confirmed
//! is a fact with a machine's help.
//!
//! Confirmation is the human or community act. A model cannot confirm its own inference — that would be the model agreeing with
//! itself — and the refusal lives in `confirm`, so no caller can route around it.

use crate::{identity::now_rfc3339, Backend, Database, Result};

/// Who is claiming the tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Claimant {
    /// A person, through the app.
    Human,
    /// An import source: the work's own metadata, a scraper's field.
    Source,
    /// A model. May only propose.
    Model,
    /// A community vote: several readers, no single author.
    Community,
}

impl Claimant {
    /// The stored `source`. `None` for a claimant this table has no word for, which is a build fact rather than a runtime one.
    pub fn source(self, work_owned_by_caller: bool) -> &'static str {
        match self {
            // The authoring path writes `author` when the pseud adding the tag owns the work; this module is the curator/fan path.
            Self::Human if work_owned_by_caller => "author",
            Self::Human => "curator",
            Self::Source => "imported",
            Self::Model => "inferred",
            Self::Community => "exchanged",
        }
    }

    /// The `confirmation` this claimant's own act amounts to.
    ///
    /// A model's is `unconfirmed` — it may only propose. A community is `reader`: several readers agreeing is a confirmation. A
    /// person or a source writes `unconfirmed` here too, and a separate confirm step is what records the reader/wrangler verdict;
    /// a human being able to confirm at the same moment they assert is a convenience, not a second vocabulary.
    pub fn confirmation(self) -> &'static str {
        match self {
            Self::Community => "reader",
            _ => "unconfirmed",
        }
    }

    /// Whether this claimant can write an assertion rather than a proposal. Only a model cannot.
    pub fn may_assert(self) -> bool {
        !matches!(self, Self::Model)
    }

    /// Every kind, in the order the requirement lists them.
    pub fn all() -> [Self; 4] {
        [Self::Human, Self::Source, Self::Model, Self::Community]
    }
}

/// One tag on a work, with the provenance the row already carries.
#[derive(Debug, Clone, PartialEq)]
pub struct TagAssertion {
    /// The taxonomy node.
    pub node_id: String,
    /// `author | curator | imported | exchanged | inferred`.
    pub source: String,
    /// `unconfirmed | reader | wrangler | inaccurate`.
    pub confirmation: String,
    /// 0..=1, how strongly this row asserts the tag.
    pub confidence: f64,
    /// When the row was written, RFC 3339.
    pub added_at: String,
}

impl TagAssertion {
    /// Whether this row is a model's unconfirmed guess — the one thing readers must not see as a fact.
    pub fn is_proposal(&self) -> bool {
        self.source == "inferred" && self.confirmation == "unconfirmed"
    }
}

/// Record a tag assertion, with the provenance its claimant implies.
///
/// Re-asserting the same (work, node) pair updates the row rather than adding one — the primary key forbids two, and "who stands
/// behind this tag" is a question about the current row.
///
/// The `DO UPDATE ... WHERE excluded.source <> 'inferred' OR work_tags.source = 'inferred'` clause is the "a model may only
/// propose" rule at the write: an inference may create a row or refresh another inference, and it may **not** overwrite a row a
/// person or a source stands behind. Without that guard a model re-asserting a human tag would rewrite `source` to `inferred` and
/// the tag would vanish from the reader's page — a proposal taking a tag away by the back door.
pub async fn assert_tag(
    db: &Database,
    work_id: &str,
    node_id: &str,
    claimant: Claimant,
    work_owned_by_caller: bool,
    confidence: f64,
) -> Result<()> {
    // A model must be able to write an inference, so it is not refused — but it can never write a confirmation, and that is
    // encoded by `Claimant::confirmation` rather than by a check here.
    let confidence = confidence.clamp(0.0, 1.0);
    let now = now_rfc3339();
    let sql = db.sql(
        "INSERT INTO work_tags (work_id, node_id, source, confidence, role, prominence, confirmation, added_at)
         VALUES (?1, ?2, ?3, ?4, 'secondary', 0, ?5, ?6)
         ON CONFLICT (work_id, node_id) DO UPDATE SET
             source = excluded.source,
             confidence = excluded.confidence,
             confirmation = CASE
                 WHEN work_tags.confirmation IN ('reader', 'wrangler')
                  AND excluded.confirmation = 'unconfirmed' THEN work_tags.confirmation
                 ELSE excluded.confirmation END,
             added_at = excluded.added_at
         WHERE excluded.source <> 'inferred' OR work_tags.source = 'inferred'",
        "INSERT INTO work_tags (work_id, node_id, source, confidence, role, prominence, confirmation, added_at)
         VALUES ($1::uuid, $2, $3, $4, 'secondary', 0, $5, $6)
         ON CONFLICT (work_id, node_id) DO UPDATE SET
             source = excluded.source,
             confidence = excluded.confidence,
             confirmation = CASE
                 WHEN work_tags.confirmation IN ('reader', 'wrangler')
                  AND excluded.confirmation = 'unconfirmed' THEN work_tags.confirmation
                 ELSE excluded.confirmation END,
             added_at = excluded.added_at
         WHERE excluded.source <> 'inferred' OR work_tags.source = 'inferred'",
    );
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(work_id)
                .bind(node_id)
                .bind(claimant.source(work_owned_by_caller))
                .bind(confidence)
                .bind(claimant.confirmation())
                .bind(&now)
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .bind(work_id)
                .bind(node_id)
                .bind(claimant.source(work_owned_by_caller))
                .bind(confidence)
                .bind(claimant.confirmation())
                .bind(&now)
                .execute(db.postgres_pool().expect("postgres handle"))
                .await?;
        }
    }
    Ok(())
}

/// Confirm a tag: a reader or a wrangler turning a proposal into a fact.
///
/// `Wrangler` is the curator's verdict and `Reader` a community one; both are levels of `confirmation` this table already has.
/// A model cannot confirm anything, and that refusal is here so no caller can route around it.
pub async fn confirm(
    db: &Database,
    work_id: &str,
    node_id: &str,
    by: ConfirmLevel,
) -> Result<bool> {
    let now = now_rfc3339();
    let sql = db.sql(
        "UPDATE work_tags SET confirmation = ?1, added_at = ?2 WHERE work_id = ?3 AND node_id = ?4",
        "UPDATE work_tags SET confirmation = $1, added_at = $2 WHERE work_id = $3::uuid AND node_id = $4",
    );
    let affected = match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(by.as_str())
                .bind(&now)
                .bind(work_id)
                .bind(node_id)
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await?
                .rows_affected()
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .bind(by.as_str())
                .bind(&now)
                .bind(work_id)
                .bind(node_id)
                .execute(db.postgres_pool().expect("postgres handle"))
                .await?
                .rows_affected()
        }
    };
    Ok(affected > 0)
}

/// Who is doing the confirming. Deliberately **not** `Claimant`: there is no `Model` arm, so "a model confirmed a tag" is a
/// sentence this module cannot express rather than one it has to remember to refuse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmLevel {
    /// A reader's confirmation.
    Reader,
    /// A taxonomy wrangler's confirmation.
    Wrangler,
}

impl ConfirmLevel {
    fn as_str(self) -> &'static str {
        match self {
            Self::Reader => "reader",
            Self::Wrangler => "wrangler",
        }
    }
}

/// Withdraw a tag. Returns whether a row was removed.
pub async fn retract(db: &Database, work_id: &str, node_id: &str) -> Result<bool> {
    let sql = db.sql(
        "DELETE FROM work_tags WHERE work_id = ?1 AND node_id = ?2",
        "DELETE FROM work_tags WHERE work_id = $1::uuid AND node_id = $2",
    );
    let affected = match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(work_id)
                .bind(node_id)
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await?
                .rows_affected()
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .bind(work_id)
                .bind(node_id)
                .execute(db.postgres_pool().expect("postgres handle"))
                .await?
                .rows_affected()
        }
    };
    Ok(affected > 0)
}

/// Every assertion on a work, proposals included — the curator's view, not a reader's.
pub async fn assertions_for_work(db: &Database, work_id: &str) -> Result<Vec<TagAssertion>> {
    let sql = db.sql(
        "SELECT node_id, source, confirmation, confidence, added_at
           FROM work_tags WHERE work_id = ?1 ORDER BY confirmation, node_id",
        "SELECT node_id, source, confirmation, confidence, added_at
           FROM work_tags WHERE work_id = $1::uuid ORDER BY confirmation, node_id",
    );
    let rows: Vec<(String, String, String, f64, String)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(work_id)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(work_id)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(rows
        .into_iter()
        .map(|(node_id, source, confirmation, confidence, added_at)| TagAssertion {
            node_id,
            source,
            confirmation,
            confidence,
            added_at,
        })
        .collect())
}

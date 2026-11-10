//! Taxonomy repository: nodes, aliases, tags.
//!
//! Spec §15.1–15.3. Both dialects.

use crate::{rewrite_placeholders, Backend, Database};
use anyhow::Result;
use serde::Serialize;

/// A taxonomy node.
#[derive(Debug, Clone, Serialize)]
pub struct CanonicalEntity {
    pub id: String,
    pub kind: String,
    pub canonical: String,
    pub norm: String,
    pub created_at: String,
    /// §15.17: `curated` or `unverified`. A node created by a person through
    /// the taxonomy UI is `curated` — §19.4 quorum is what makes a node
    /// curated. A node auto-created from an exchange signal is `unverified`:
    /// usable immediately, and never presented as curated.
    ///
    /// Read this on every surface that shows a node. §15.17's second acceptance
    /// line is that an unverified entity is never rendered as curated, and the
    /// only way to honour that is for the status to travel with the node rather
    /// than be re-derived by each surface from whatever table it happened to
    /// read.
    pub review_status: String,
    /// §15.17: review priority, nothing else. Never a demand weight
    /// (§16.16.1), never a ranking input, and never a count of readers.
    pub signal_count: i64,
}

/// A taxonomy alias.
#[derive(Debug, Clone, Serialize)]
pub struct EntityAlias {
    pub alias: String,
    pub norm: String,
    pub node_id: String,
    pub source: String,
}

/// A work tag.
///
/// The assertion as the store wrote it, so a caller can show what it claimed
/// rather than what it hoped it claimed: the prominence, the provenance the
/// code path supplied, and the role.
#[derive(Debug, Clone, Serialize)]
pub struct WorkTag {
    pub work_id: String,
    pub node_id: String,
    /// How strongly the work leans on the tag (spec §6.4's `prominence`).
    pub prominence: i64,
    /// Where the claim came from: `author` · `curator` · `imported` ·
    /// `exchanged` · `inferred`.
    pub source: String,
    /// The claim's role: `primary` · `secondary` · `background`.
    pub role: String,
    pub added_at: String,
}

/// The normalised identity form for a node name.
///
/// Delegates to the domain's `normalise` rather than trimming and lowercasing
/// here. Two normalisers in one codebase is not a style question: `SLOW   BURN`
/// (doubled spacing) is `slow   burn` under `trim().to_lowercase()` and
/// `slow burn` under the domain rule, so the same submitted name produced two
/// different nodes depending on which function wrote the row — and the unique
/// index on `(kind, norm)` did not catch it, because both rows were internally
/// consistent.
///
/// §15.17's "a signal naming a known tag adds an alias rather than creating a
/// duplicate" is only true if there is one normaliser. The domain rule is also
/// the one the wire contract's tests pin, so it is the one that wins.
fn canonical_form(text: &str) -> String {
    darkhold_domain::exchange::normalise(text)
}

/// Search nodes by kind and prefix.
pub async fn search_nodes(
    db: &Database,
    kind: Option<&str>,
    prefix: &str,
    limit: i64,
) -> Result<Vec<CanonicalEntity>> {
    let sql = db.sql(
        "SELECT id, kind, canonical, norm, created_at, review_status, CAST(signal_count AS BIGINT) FROM canonical_entities
         WHERE (?1 IS NULL OR kind = ?1) AND norm LIKE ?2 || '%'
         ORDER BY norm ASC LIMIT ?3",
        "SELECT id::text, kind, canonical, norm, created_at, review_status, CAST(signal_count AS BIGINT) FROM canonical_entities
         WHERE ($1 IS NULL OR kind = $1) AND norm LIKE $2 || '%'
         ORDER BY norm ASC LIMIT $3",
    );
    let rows = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as::<_, (String, String, String, String, String, String, i64)>(&sql)
                .bind(kind)
                .bind(prefix)
                .bind(limit)
                .fetch_all(db.sqlite_pool().expect("sqlite"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as::<_, (String, String, String, String, String, String, i64)>(&sql)
                .bind(kind)
                .bind(prefix)
                .bind(limit)
                .fetch_all(db.postgres_pool().expect("postgres"))
                .await?
        }
    };
    Ok(rows
        .into_iter()
        .map(
            |(id, kind, canonical, norm, created_at, review_status, signal_count)| {
                CanonicalEntity {
                    id,
                    kind,
                    canonical,
                    norm,
                    created_at,
                    review_status,
                    signal_count,
                }
            },
        )
        .collect())
}

/// Search taxonomy nodes by prefix, then fall back to fuzzy matching.
///
/// Exact prefix matches are returned first (up to `limit`), then fuzzy
/// matches fill any remaining slots. Fuzzy matching uses Levenshtein
/// similarity with a floor of `lexicon::taxonomy::FUZZY_SIMILARITY_FLOOR`.
pub async fn search_nodes_fuzzy(
    db: &Database,
    kind: Option<&str>,
    query: &str,
    limit: i64,
) -> Result<Vec<CanonicalEntity>> {
    use darkhold_domain::taxonomy::{canonical_form, fuzzy_match};
    use std::collections::HashMap;

    let prefix = query.trim();
    let normalized = canonical_form(prefix);

    // 1. Exact prefix matches (normalized so LIKE is case-consistent).
    let sql_prefix = db.sql(
        "SELECT id, kind, canonical, norm, created_at, review_status, CAST(signal_count AS BIGINT) FROM canonical_entities
         WHERE (?1 IS NULL OR kind = ?1) AND norm LIKE ?2 || '%'
         ORDER BY norm ASC",
        "SELECT id::text, kind, canonical, norm, created_at, review_status, CAST(signal_count AS BIGINT) FROM canonical_entities
         WHERE ($1 IS NULL OR kind = $1) AND norm LIKE $2 || '%'
         ORDER BY norm ASC",
    );
    let prefix_rows: Vec<(String, String, String, String, String, String, i64)> = match db.backend()
    {
        Backend::Sqlite => {
            sqlx::query_as(&sql_prefix)
                .bind(kind)
                .bind(&normalized)
                .fetch_all(db.sqlite_pool().expect("sqlite"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql_prefix)
                .bind(kind)
                .bind(&normalized)
                .fetch_all(db.postgres_pool().expect("postgres"))
                .await?
        }
    };
    let prefix_nodes: Vec<CanonicalEntity> = prefix_rows
        .iter()
        .map(
            |(id, kind, canonical, norm, created_at, review_status, signal_count)| {
                CanonicalEntity {
                    id: id.clone(),
                    kind: kind.clone(),
                    canonical: canonical.clone(),
                    norm: norm.clone(),
                    created_at: created_at.clone(),
                    review_status: review_status.clone(),
                    signal_count: *signal_count,
                }
            },
        )
        .collect();

    let exact_count = prefix_nodes.len();
    if exact_count as i64 >= limit {
        return Ok(prefix_nodes);
    }

    // 2. Fuzzy fallback: query non-prefix nodes and rank by similarity.
    let remaining = (limit - exact_count as i64) as usize;
    let sql_rest = db.sql(
        "SELECT id, kind, canonical, norm, created_at, review_status, CAST(signal_count AS BIGINT) FROM canonical_entities
         WHERE (?1 IS NULL OR kind = ?1) AND norm NOT LIKE ?2 || '%'
         ORDER BY norm ASC",
        "SELECT id::text, kind, canonical, norm, created_at, review_status, CAST(signal_count AS BIGINT) FROM canonical_entities
         WHERE ($1 IS NULL OR kind = $1) AND norm NOT LIKE $2 || '%'
         ORDER BY norm ASC",
    );
    let rest_rows: Vec<(String, String, String, String, String, String, i64)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql_rest)
                .bind(kind)
                .bind(&normalized)
                .fetch_all(db.sqlite_pool().expect("sqlite"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql_rest)
                .bind(kind)
                .bind(&normalized)
                .fetch_all(db.postgres_pool().expect("postgres"))
                .await?
        }
    };

    let candidates: Vec<(String, String)> = rest_rows
        .iter()
        .map(|(id, _k, _c, norm, _cr, _rs, _sc)| (id.clone(), norm.clone()))
        .collect();
    let fuzzy = fuzzy_match(&normalized, candidates, remaining);

    // Map fuzzy match ids back to full CanonicalEntity records.
    let lookup: HashMap<String, CanonicalEntity> = rest_rows
        .into_iter()
        .map(
            |(id, kind, canonical, norm, created_at, review_status, signal_count)| {
                (
                    id.clone(),
                    CanonicalEntity {
                        id,
                        kind,
                        canonical,
                        norm,
                        created_at,
                        review_status,
                        signal_count,
                    },
                )
            },
        )
        .collect();

    let mut result = prefix_nodes;
    for m in fuzzy {
        if let Some(node) = lookup.get(&m.id) {
            result.push(node.clone());
        }
    }
    Ok(result)
}

/// Get a node by id.
pub async fn node_by_id(db: &Database, id: &str) -> Result<Option<CanonicalEntity>> {
    let sql = db.sql(
        "SELECT id, kind, canonical, norm, created_at, review_status, CAST(signal_count AS BIGINT) FROM canonical_entities WHERE id = ?",
        "SELECT id, kind, canonical, norm, created_at, review_status, CAST(signal_count AS BIGINT) FROM canonical_entities WHERE id = $1",
    );
    let row = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as::<_, (String, String, String, String, String, String, i64)>(&sql)
                .bind(id)
                .fetch_optional(db.sqlite_pool().expect("sqlite"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as::<_, (String, String, String, String, String, String, i64)>(&sql)
                .bind(id)
                .fetch_optional(db.postgres_pool().expect("postgres"))
                .await?
        }
    };
    Ok(row.map(
        |(id, kind, canonical, norm, created_at, review_status, signal_count)| CanonicalEntity {
            id,
            kind,
            canonical,
            norm,
            created_at,
            review_status,
            signal_count,
        },
    ))
}

/// Create a taxonomy node.
pub async fn create_node(db: &Database, kind: &str, canonical: &str) -> Result<CanonicalEntity> {
    let id = uuid::Uuid::new_v4().to_string();
    let norm = canonical_form(canonical);
    let now = crate::identity::now_rfc3339();
    let sql = db.sql(
        "INSERT INTO canonical_entities (id, kind, canonical, norm, created_at) VALUES (?, ?, ?, ?, ?)",
        "INSERT INTO canonical_entities (id, kind, canonical, norm, created_at) VALUES ($1::uuid, $2, $3, $4, $5)",
    );
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(&id)
                .bind(kind)
                .bind(canonical)
                .bind(&norm)
                .bind(&now)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .bind(&id)
                .bind(kind)
                .bind(canonical)
                .bind(&norm)
                .bind(&now)
                .execute(db.postgres_pool().expect("postgres"))
                .await?;
        }
    }
    Ok(CanonicalEntity {
        id,
        kind: kind.to_owned(),
        canonical: canonical.to_owned(),
        norm,
        created_at: now,
        // A node created through this function was created by a person. Its
        // authority came from §19.4 quorum, not from the exchange, so it starts
        // curated. The signal path uses `ensure_node_from_signal` instead, which
        // is the only way to get an unverified node — so "who made this" is
        // visible in which function was called, not in a boolean argument that
        // could be passed the wrong way round.
        review_status: "curated".to_owned(),
        signal_count: 0,
    })
}

/// §15.17: get or create a node from a name a signal carried.
///
/// The one function that can produce an **unverified** node, and that is the
/// point. §15.17's load-bearing claim is that a new name from a signal is
/// usable immediately — attachable to a work, searchable, visible in the tag
/// browser — while carrying `unverified`.
///
/// The upsert is not an optimisation. A check-then-insert races: two signals
/// naming the same brand-new tag on a young instance, which is the *common*
/// case there, and the loser gets a unique-constraint error instead of a node.
/// The `signal_count` increment is in the same statement for the same reason —
/// the count orders the review queue, and a lost increment makes a genuinely
/// urgent tag look idle.
///
/// A node that already exists is returned **unchanged**, including its
/// `review_status`. An existing curated node must not be demoted because someone
/// signalled the same name, and an existing unverified node must not be promoted
/// because enough people agreed — promotion is §19.4 quorum work, done by a
/// curator, never a side effect of counting.
pub async fn ensure_node_from_signal(
    db: &Database,
    kind: &str,
    canonical: &str,
) -> Result<CanonicalEntity> {
    let norm = canonical_form(canonical);
    let now = crate::identity::now_rfc3339();
    let id = uuid::Uuid::new_v4().to_string();
    // Note what is *not* in the update arm: `canonical` and `review_status`.
    // Reinforcing a node must not rewrite a curator's chosen display form, and
    // agreement is not curation.
    let sql = db.sql(
        "INSERT INTO canonical_entities (id, kind, canonical, norm, created_at, review_status, signal_count, node_state)
         VALUES (?, ?, ?, ?, ?, 'unverified', 1, 'assertion_only')
         ON CONFLICT (kind, norm) DO UPDATE SET signal_count = canonical_entities.signal_count + 1",
        "INSERT INTO canonical_entities (id, kind, canonical, norm, created_at, review_status, signal_count, node_state)
         VALUES ($1::uuid, $2, $3, $4, $5, 'unverified', 1, 'assertion_only')
         ON CONFLICT (kind, norm) DO UPDATE SET signal_count = canonical_entities.signal_count + 1",
    );
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(&id)
                .bind(kind)
                .bind(canonical)
                .bind(&norm)
                .bind(&now)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .bind(&id)
                .bind(kind)
                .bind(canonical)
                .bind(&norm)
                .bind(&now)
                .execute(db.postgres_pool().expect("postgres"))
                .await?;
        }
    }
    let sql = db.sql(
        "SELECT id, kind, canonical, norm, created_at, review_status, signal_count
         FROM canonical_entities WHERE kind = ? AND norm = ?",
        "SELECT id, kind, canonical, norm, created_at, review_status, CAST(signal_count AS BIGINT)
         FROM canonical_entities WHERE kind = $1 AND norm = $2",
    );
    #[derive(sqlx::FromRow)]
    struct Row {
        id: String,
        kind: String,
        canonical: String,
        norm: String,
        created_at: String,
        review_status: String,
        signal_count: i64,
    }
    let row: Row = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(kind)
                .bind(&norm)
                .fetch_one(db.sqlite_pool().expect("sqlite"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(kind)
                .bind(&norm)
                .fetch_one(db.postgres_pool().expect("postgres"))
                .await?
        }
    };
    Ok(CanonicalEntity {
        id: row.id,
        kind: row.kind,
        canonical: row.canonical,
        norm: row.norm,
        created_at: row.created_at,
        review_status: row.review_status,
        signal_count: row.signal_count,
    })
}

// `curate_node` lived here: it set `canonical_entities.canonical` and `review_status = 'curated'` and nothing else. §15.17
// keeps two tables describing one curated state, and `curate_name` writes both in one transaction; `curate_node` wrote one, so a
// caller that reached it curated a name in the vocabulary `search_nodes` reads while leaving the exchange's row unverified — the
// exact disagreement migration 0082 says a join silently drops, and `curation_reaches_taxonomy.rs` names this function as the
// hazard `curate_name` exists to remove. It also recorded no curator: the exchange's `curated_by`/`curated_at` columns exist
// precisely so "who decided this" is answerable, and the narrower function left them NULL on the row it did touch.
//
// There is no replacement here on purpose: `curate_name` is the only doorway, and a second `pub` writer is how the two
// vocabularies drifted in the first place.

/// Curate a name in **both** vocabularies, atomically.
///
/// §15.17 keeps two tables describing one state: `canonical_entities` (what a tag
/// browser reads, what `search_nodes` searches, what `work_tags` references) and
/// `canonical_entities` (the exchange's own view, carrying the curator's account and
/// timestamps). Migration 0082 states the rule this function exists to enforce:
///
/// > Two tables describing one state must not spell it two ways, or a query
/// > joining them silently drops every curated row.
///
/// Until now the two writes were two calls in a route handler
/// (`routes/exchange.rs::curate_entity`), which has three consequences:
///
/// 1. **It is a route-only invariant.** Any other caller — a job, an import, the
///    admin path — curates one table and silently misses the other. `curate_node`
///    and `curate_entity` are both `pub`, so that is not a compiler error.
/// 2. **It is not atomic.** The first write commits before the second begins, so a
///    failure between them leaves a name curated in one vocabulary and unverified in
///    the other — the exact state 0082 says a join drops.
/// 3. **The pair is the only place that knows they must be written together.** A
///    fourth writer would have no way to learn that.
///
/// So the pairing moves into the db layer, where it is one call, one transaction,
/// and reachable by anything that can reach the other two.
///
/// `Ok(false)` when neither table had the name: a curate of a name that does not
/// exist is not a silent success, and the route maps that to 404 rather than
/// reporting `curated: true` for work that never happened.
///
/// Neither write may demote: both `UPDATE`s are already narrow (they set
/// `review_status = 'curated'` and `canonical` on a `(kind, norm)` match), and
/// `ensure_node_from_signal`'s update arm deliberately excludes both columns, so a
/// later signal cannot undo this.
pub async fn curate_name(
    db: &Database,
    kind: &str,
    norm: &str,
    canonical_form: &str,
    curator: &str,
) -> Result<bool> {
    let now = crate::identity::now_rfc3339();
    // Two statements rather than one: the tables have no shared key (one is keyed
    // (kind, norm), the other by a uuid `id`), so there is no single statement that
    // writes both. `canonical_entities` also keeps its own `created_at`.
    let node_sql = db.sql(
        "UPDATE canonical_entities SET canonical = ?, review_status = 'curated'
         WHERE kind = ? AND norm = ?",
        "UPDATE canonical_entities SET canonical = $1, review_status = 'curated'
         WHERE kind = $2 AND norm = $3",
    );
    let entity_sql = db.sql(
        "UPDATE canonical_entities
         SET canonical = ?, review_status = 'curated', curated_by = ?, curated_at = ?, updated_at = ?
         WHERE kind = ? AND norm = ?",
        "UPDATE canonical_entities
         SET canonical = $1, review_status = 'curated', curated_by = $2, curated_at = $3, updated_at = $4
         WHERE kind = $5 AND norm = $6",
    );

    match db.backend() {
        Backend::Sqlite => {
            let mut tx = db.sqlite_pool().expect("sqlite").begin().await?;
            let node = sqlx::query(&node_sql)
                .bind(canonical_form)
                .bind(kind)
                .bind(norm)
                .execute(&mut *tx)
                .await?
                .rows_affected();
            let entity = sqlx::query(&entity_sql)
                .bind(canonical_form)
                .bind(curator)
                .bind(&now)
                .bind(&now)
                .bind(kind)
                .bind(norm)
                .execute(&mut *tx)
                .await?
                .rows_affected();
            if node == 0 && entity == 0 {
                tx.rollback().await?;
                return Ok(false);
            }
            tx.commit().await?;
            Ok(true)
        }
        Backend::Postgres => {
            let mut tx = db.postgres_pool().expect("postgres").begin().await?;
            let node = sqlx::query(&node_sql)
                .bind(canonical_form)
                .bind(kind)
                .bind(norm)
                .execute(&mut *tx)
                .await?
                .rows_affected();
            let entity = sqlx::query(&entity_sql)
                .bind(canonical_form)
                .bind(curator)
                .bind(&now)
                .bind(&now)
                .bind(kind)
                .bind(norm)
                .execute(&mut *tx)
                .await?
                .rows_affected();
            if node == 0 && entity == 0 {
                tx.rollback().await?;
                return Ok(false);
            }
            tx.commit().await?;
            Ok(true)
        }
    }
}

/// The nodes awaiting review, most-reinforced first (§15.17's queue).
///
/// `ORDER BY signal_count DESC` is the whole point of the function, so it is in
/// the ORDER BY rather than a Rust sort — the `canonical_entities_review` index is
/// built for it, and sorting a full table read in Rust is both slower and
/// ignores it.
pub async fn list_unverified_nodes(db: &Database, limit: i64) -> Result<Vec<CanonicalEntity>> {
    #[derive(sqlx::FromRow)]
    struct Row {
        id: String,
        kind: String,
        canonical: String,
        norm: String,
        created_at: String,
        review_status: String,
        signal_count: i64,
    }
    let sql = db.sql(
        // No CAST on the SQLite arm: `signal_count` is INTEGER there, and the
        // cast exists only because PostgreSQL types a bare COUNT-style integer
        // column INT4. Carrying it on both arms is harmless SQL that hides which
        // one actually needs it.
        "SELECT id, kind, canonical, norm, created_at, review_status, signal_count
         FROM canonical_entities WHERE review_status = 'unverified'
         ORDER BY signal_count DESC, norm ASC LIMIT ?",
        "SELECT id, kind, canonical, norm, created_at, review_status, CAST(signal_count AS BIGINT)
         FROM canonical_entities WHERE review_status = 'unverified'
         ORDER BY signal_count DESC, norm ASC LIMIT $1",
    );
    let rows: Vec<Row> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(limit)
                .fetch_all(db.sqlite_pool().expect("sqlite"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(limit)
                .fetch_all(db.postgres_pool().expect("postgres"))
                .await?
        }
    };
    Ok(rows
        .into_iter()
        .map(|r| CanonicalEntity {
            id: r.id,
            kind: r.kind,
            canonical: r.canonical,
            norm: r.norm,
            created_at: r.created_at,
            review_status: r.review_status,
            signal_count: r.signal_count,
        })
        .collect())
}

/// Create an alias.
pub async fn create_alias(
    db: &Database,
    alias: &str,
    node_id: &str,
    source: &str,
) -> Result<EntityAlias> {
    let norm = canonical_form(alias);
    let sql = db.sql(
        "INSERT INTO entity_aliases (alias, norm, node_id, source) VALUES (?, ?, ?, ?)",
        "INSERT INTO entity_aliases (alias, norm, node_id, source) VALUES ($1, $2, $3::uuid, $4)",
    );
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(alias)
                .bind(&norm)
                .bind(node_id)
                .bind(source)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .bind(alias)
                .bind(&norm)
                .bind(node_id)
                .bind(source)
                .execute(db.postgres_pool().expect("postgres"))
                .await?;
        }
    }
    Ok(EntityAlias {
        alias: alias.to_owned(),
        norm,
        node_id: node_id.to_owned(),
        source: source.to_owned(),
    })
}

/// Where a tag assertion came from (spec §6.4).
///
/// **The distinction is the one that matters to a reader.** A tag the
/// *author* put on their own work is a statement of fact about the work, and
/// a tag the importer guessed from a source's own metadata is a guess. Both
/// are in the table, and a surface that shows a work's tags can say which is
/// which.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagSource {
    /// The author tagged their own work.
    Author,
    /// A curator tagged it.
    Curator,
    /// It came in with an import.
    Imported,
    /// It came in through the metadata exchange.
    Exchanged,
    /// It was inferred, not asserted by anybody.
    Inferred,
}

impl TagSource {
    /// The storage form.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Author => "author",
            Self::Curator => "curator",
            Self::Imported => "imported",
            Self::Exchanged => "exchanged",
            Self::Inferred => "inferred",
        }
    }

    /// The storage form, from a column value.
    ///
    /// An unrecognised value is **imported**: the value the table's default
    /// carries, and the one that says the least.
    pub fn parse(raw: &str) -> Self {
        match raw {
            "author" => Self::Author,
            "curator" => Self::Curator,
            "exchanged" => Self::Exchanged,
            "inferred" => Self::Inferred,
            _ => Self::Imported,
        }
    }
}

/// A tag's role in a work's description (spec §6.4, §2.2).
///
/// The primary/secondary split is the **theme overlap rule**: a tag's primary
/// theme is the theme whose seed claims it, and the secondary is derived.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagRole {
    /// The tag's own theme claims it.
    Primary,
    /// Another theme's seed also claims it.
    Secondary,
    /// It is in the work and nothing claims it.
    Background,
}

impl TagRole {
    /// The storage form.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Primary => "primary",
            Self::Secondary => "secondary",
            Self::Background => "background",
        }
    }

    /// The storage form, from a column value.
    pub fn parse(raw: &str) -> Self {
        match raw {
            "secondary" => Self::Secondary,
            "background" => Self::Background,
            _ => Self::Primary,
        }
    }
}

/// What to assert about a work's tag.
#[derive(Debug, Clone, Copy)]
pub struct TagAssertion {
    /// The tag's prominence: how much the work leans on it.
    pub prominence: i64,
    /// Where the claim came from.
    pub source: TagSource,
    /// The claim's role in the work's description.
    pub role: TagRole,
}

impl Default for TagAssertion {
    /// The assertion that says the least: nobody claimed it, and the tag is not
    /// the work's headline description.
    fn default() -> Self {
        Self {
            prominence: 0,
            source: TagSource::Imported,
            role: TagRole::Secondary,
        }
    }
}

/// Tag a work.
pub async fn tag_work(
    db: &Database,
    work_id: &str,
    node_id: &str,
    assertion: TagAssertion,
) -> Result<WorkTag> {
    let now = crate::identity::now_rfc3339();
    let sql = db.sql(
        "INSERT INTO work_tags (work_id, node_id, prominence, source, role, added_at)
         VALUES (?, ?, ?, ?, ?, ?)",
        "INSERT INTO work_tags (work_id, node_id, prominence, source, role, added_at)
         VALUES ($1::uuid, $2, $3, $4, $5, $6)",
    );
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(work_id)
                .bind(node_id)
                .bind(assertion.prominence)
                .bind(assertion.source.as_str())
                .bind(assertion.role.as_str())
                .bind(&now)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .bind(work_id)
                .bind(node_id)
                .bind(assertion.prominence)
                .bind(assertion.source.as_str())
                .bind(assertion.role.as_str())
                .bind(&now)
                .execute(db.postgres_pool().expect("postgres"))
                .await?;
        }
    }
    Ok(WorkTag {
        work_id: work_id.to_owned(),
        node_id: node_id.to_owned(),
        prominence: assertion.prominence,
        source: assertion.source.as_str().to_owned(),
        role: assertion.role.as_str().to_owned(),
        added_at: now,
    })
}

/// Recompute every tag's confidence from the votes (spec §6.4).
///
/// The confidence is fed by `work_tag_votes` and **only** by it: never by
/// gravity, never by a rank, never by a reader count. A vote is a reader
/// saying "this tag is right" or "this tag is wrong", and the share of the
/// votes that agree is the confidence.
///
/// **A vote is not a reader.** The votes are counted, not the voters, and a
/// reader who votes twice on the same tag is one row -- the primary key says
/// so. So the number is a share of opinions and never a headcount.
pub async fn recompute_confidence(db: &Database) -> Result<i64, sqlx::Error> {
    let sql = db.sql(
        "UPDATE work_tags
            SET confidence = (
                SELECT CASE
                    WHEN COUNT(*) = 0 THEN 1.0
                    ELSE (CAST(SUM(CASE WHEN vote > 0 THEN 1 ELSE 0 END) AS REAL) + 1.0)
                         / (COUNT(*) + 1.0)
                END
                  FROM work_tag_votes v
                 WHERE v.work_id = work_tags.work_id AND v.node_id = work_tags.node_id
            )",
        "UPDATE work_tags
            SET confidence = (
                SELECT CASE
                    WHEN COUNT(*) = 0 THEN 1.0
                    ELSE (CAST(SUM(CASE WHEN vote > 0 THEN 1 ELSE 0 END) AS REAL) + 1.0)
                         / (COUNT(*) + 1.0)
                END
                  FROM work_tag_votes v
                 WHERE v.work_id::text = work_tags.work_id::text
                   AND v.node_id = work_tags.node_id
            )",
    );
    let affected = match db.backend() {
        Backend::Sqlite => sqlx::query(&sql)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await?
            .rows_affected(),
        Backend::Postgres => sqlx::query(&sql)
            .execute(db.postgres_pool().expect("postgres"))
            .await?
            .rows_affected(),
    };
    Ok(affected as i64)
}

/// §6.4's confidence for one (work, tag), computed from the votes and nothing else.
///
/// The expression is the sweep's, scoped: with no votes a tag reads as `1.0` (a claim nobody has disputed is not a claim nobody
/// believes), and otherwise the smoothed share of agreeing votes, `(agree + 1) / (votes + 1)`. It lives here once so the vote path
/// and the repair pass cannot compute two different numbers.
async fn confidence_expression(
    db: &Database,
    work_id: &str,
    node_id: &str,
) -> Result<f64, sqlx::Error> {
    let sql = db.sql(
        "SELECT CASE
             WHEN COUNT(*) = 0 THEN 1.0
             ELSE (CAST(SUM(CASE WHEN vote > 0 THEN 1 ELSE 0 END) AS REAL) + 1.0) / (COUNT(*) + 1.0)
         END
           FROM work_tag_votes
          WHERE work_id = ? AND node_id = ?",
        // `CASE` unifies its branches to one type, and `1.0` is a NUMERIC literal on PostgreSQL, so the whole expression decodes as
        // NUMERIC unless it is cast — and sqlx cannot turn a NUMERIC into `f64`. The cast goes on the CASE, not on one branch: a cast
        // inside `THEN` would leave the ELSE to decide the type, which is the same mistake spelled differently.
        "SELECT CAST(CASE
             WHEN COUNT(*) = 0 THEN 1.0
             ELSE (CAST(SUM(CASE WHEN vote > 0 THEN 1 ELSE 0 END) AS REAL) + 1.0) / (COUNT(*) + 1.0)
         END AS DOUBLE PRECISION)
           FROM work_tag_votes
          WHERE work_id::text = $1 AND node_id = $2",
    );
    let value: f64 = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar(&sql)
                .bind(work_id)
                .bind(node_id)
                .fetch_one(db.sqlite_pool().expect("sqlite"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar(&sql)
                .bind(work_id)
                .bind(node_id)
                .fetch_one(db.postgres_pool().expect("postgres"))
                .await?
        }
    };
    Ok(value)
}

/// The confidence the work's tag row currently carries, or `None` when the work does not carry the tag.
///
/// `None` and `1.0` are different answers — "this work is not tagged that way" versus "nobody has disputed it" — which is why this
/// returns an `Option` and the route maps the `None` to `404`.
pub async fn tag_confidence(
    db: &Database,
    work_id: &str,
    node_id: &str,
) -> Result<Option<f64>, sqlx::Error> {
    let sql = db.sql(
        "SELECT confidence FROM work_tags WHERE work_id = ? AND node_id = ?",
        "SELECT confidence FROM work_tags WHERE work_id::text = $1 AND node_id = $2",
    );
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar(&sql)
                .bind(work_id)
                .bind(node_id)
                .fetch_optional(db.sqlite_pool().expect("sqlite"))
                .await
        }
        Backend::Postgres => {
            sqlx::query_scalar(&sql)
                .bind(work_id)
                .bind(node_id)
                .fetch_optional(db.postgres_pool().expect("postgres"))
                .await
        }
    }
}

/// The caller's own vote on one tag: `Some(-1 | 0 | 1)`, or `None` when they have not voted or withdrew.
///
/// Only ever the caller's: there is no function here that returns anybody else's vote, because a scoped opinion published with a
/// handle is a durable public judgement about another person's work.
pub async fn tag_vote_for(
    db: &Database,
    work_id: &str,
    node_id: &str,
    voter_pseud_id: &str,
) -> Result<Option<i8>, sqlx::Error> {
    let sql = db.sql(
        "SELECT vote FROM work_tag_votes WHERE work_id = ? AND node_id = ? AND voter_pseud_id = ?",
        // `vote` is INTEGER (INT4) on PostgreSQL and sqlx decodes INT4 only into `i32`; the column is cast so one decode target
        // serves both engines.
        "SELECT CAST(vote AS BIGINT) FROM work_tag_votes WHERE work_id::text = $1 AND node_id = $2 AND voter_pseud_id = $3",
    );
    let vote: Option<i64> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar(&sql)
                .bind(work_id)
                .bind(node_id)
                .bind(voter_pseud_id)
                .fetch_optional(db.sqlite_pool().expect("sqlite"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar(&sql)
                .bind(work_id)
                .bind(node_id)
                .bind(voter_pseud_id)
                .fetch_optional(db.postgres_pool().expect("postgres"))
                .await?
        }
    };
    Ok(vote.map(|v| v as i8))
}

/// Cast, change or withdraw a reader's vote on a tag the work carries, and return the confidence that results.
///
/// `vote == 0` **withdraws the row** rather than storing an abstention: "I take it back" is not evidence against the tag, and
/// counting it in the denominator would let four readers who all withdrew pull a tag's confidence down — a disagreement count with
/// no disagreement in it. Every other value is one row per (work, tag, pseud), because the primary key says a reader who votes
/// twice is one opinion.
///
/// The confidence is recomputed in the same request, not by a nightly sweep: a reader who takes the trouble to answer should see
/// the number their answer produced. The sweep (`recompute_confidence`) remains the repair tool for the window between the vote
/// write and the aggregate — a crash in between is exactly the drift it exists to fix.
///
/// Returns `Ok(None)` when the work does not carry the tag: there is no claim to judge, and the route answers `404`.
pub async fn cast_tag_vote(
    db: &Database,
    work_id: &str,
    node_id: &str,
    voter_pseud_id: &str,
    vote: i8,
) -> Result<Option<f64>, sqlx::Error> {
    if tag_confidence(db, work_id, node_id).await?.is_none() {
        return Ok(None);
    }
    let now = crate::identity::now_rfc3339();
    if vote == 0 {
        let sql = db.sql(
            "DELETE FROM work_tag_votes WHERE work_id = ? AND node_id = ? AND voter_pseud_id = ?",
            "DELETE FROM work_tag_votes WHERE work_id::text = $1 AND node_id = $2 AND voter_pseud_id = $3",
        );
        match db.backend() {
            Backend::Sqlite => {
                sqlx::query(&sql)
                    .bind(work_id)
                    .bind(node_id)
                    .bind(voter_pseud_id)
                    .execute(db.sqlite_pool().expect("sqlite"))
                    .await?;
            }
            Backend::Postgres => {
                sqlx::query(&sql)
                    .bind(work_id)
                    .bind(node_id)
                    .bind(voter_pseud_id)
                    .execute(db.postgres_pool().expect("postgres"))
                    .await?;
            }
        }
    } else {
        // `work_id` is UUID on PostgreSQL and TEXT on SQLite; the column is cast rather than the parameter, so a malformed id is a
        // `404` from `tag_confidence` above and never a driver error here.
        let sql = db.sql(
            "INSERT INTO work_tag_votes (work_id, node_id, voter_pseud_id, vote, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?)
             ON CONFLICT (work_id, node_id, voter_pseud_id)
             DO UPDATE SET vote = excluded.vote, updated_at = excluded.updated_at",
            "INSERT INTO work_tag_votes (work_id, node_id, voter_pseud_id, vote, created_at, updated_at)
             VALUES ($1::uuid, $2, $3, $4, $5, $6)
             ON CONFLICT (work_id, node_id, voter_pseud_id)
             DO UPDATE SET vote = excluded.vote, updated_at = excluded.updated_at",
        );
        match db.backend() {
            Backend::Sqlite => {
                sqlx::query(&sql)
                    .bind(work_id)
                    .bind(node_id)
                    .bind(voter_pseud_id)
                    .bind(vote as i64)
                    .bind(&now)
                    .bind(&now)
                    .execute(db.sqlite_pool().expect("sqlite"))
                    .await?;
            }
            Backend::Postgres => {
                // INT4 on PostgreSQL: binding `i64` here fails with a type mismatch the driver reports as a 500, and SQLite's
                // native `i64` hides it — the same trap `single_use` set in the export grants.
                sqlx::query(&sql)
                    .bind(work_id)
                    .bind(node_id)
                    .bind(voter_pseud_id)
                    .bind(vote as i32)
                    .bind(&now)
                    .bind(&now)
                    .execute(db.postgres_pool().expect("postgres"))
                    .await?;
            }
        }
    }

    let confidence = confidence_expression(db, work_id, node_id).await?;
    let sql = db.sql(
        "UPDATE work_tags SET confidence = ? WHERE work_id = ? AND node_id = ?",
        "UPDATE work_tags SET confidence = $1 WHERE work_id::text = $2 AND node_id = $3",
    );
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(confidence)
                .bind(work_id)
                .bind(node_id)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .bind(confidence)
                .bind(work_id)
                .bind(node_id)
                .execute(db.postgres_pool().expect("postgres"))
                .await?;
        }
    }
    Ok(Some(confidence))
}

/// List all tags
pub async fn list_tags(
    db: &Database,
    limit: i64,
    offset: i64,
) -> Result<Vec<(String, String, String, i64)>> {
    let sql = db.sql(
        "SELECT id, canonical, kind, (
            SELECT COUNT(*) FROM work_tags wt WHERE wt.node_id = canonical_entities.id
        ) AS work_count
         FROM canonical_entities
         WHERE kind = 'tag'
         ORDER BY canonical ASC
         LIMIT ? OFFSET ?",
        "SELECT id::text, canonical, kind, (
            SELECT COUNT(*) FROM work_tags wt WHERE wt.node_id = canonical_entities.id
        ) AS work_count
         FROM canonical_entities
         WHERE kind = 'tag'
         ORDER BY canonical ASC
         LIMIT $1 OFFSET $2",
    );
    let rows = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as::<_, (String, String, String, i64)>(&sql)
                .bind(limit)
                .bind(offset)
                .fetch_all(db.sqlite_pool().expect("sqlite"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as::<_, (String, String, String, i64)>(&sql)
                .bind(limit)
                .bind(offset)
                .fetch_all(db.postgres_pool().expect("postgres"))
                .await?
        }
    };
    Ok(rows)
}

/// Look up all tag names for a work (canonical forms).
pub async fn tag_names_for_work(db: &Database, work_id: &str) -> Result<Vec<String>> {
    let rows: Vec<(String,)> = match db.backend() {
        Backend::Sqlite => sqlx::query_as(
            "SELECT tn.canonical
             FROM work_tags wt
             JOIN canonical_entities tn ON tn.id = wt.node_id
             WHERE wt.work_id = ? AND tn.kind = 'tag'
               AND NOT (wt.source = 'inferred' AND wt.confirmation = 'unconfirmed')",
        )
        .bind(work_id)
        .fetch_all(db.sqlite_pool().expect("sqlite"))
        .await
        .unwrap_or_default(),
        Backend::Postgres => sqlx::query_as(
            "SELECT tn.canonical
             FROM work_tags wt
             JOIN canonical_entities tn ON tn.id = wt.node_id
             WHERE wt.work_id = $1::uuid AND tn.kind = 'tag'
               AND NOT (wt.source = 'inferred' AND wt.confirmation = 'unconfirmed')",
        )
        .bind(work_id)
        .fetch_all(db.postgres_pool().expect("postgres"))
        .await
        .unwrap_or_default(),
    };
    Ok(rows.into_iter().map(|(n,)| n).collect())
}

/// Tags that co-occur with `tag_norm`, ranked by **NPMI** rather than by raw co-occurrence (M69-19).
///
/// Raw counts reward ubiquitous tags: `angst` co-occurs with everything, so it "relates" to everything and therefore
/// says nothing. NPMI normalises each co-occurrence by how often both tags appear at all:
///
/// ```text
/// npmi(a,b) = ln( P(a,b) / (P(a) P(b)) ) / -ln P(a,b)
/// ```
///
/// which is 1 when `b` appears exactly when `a` does, 0 when they are independent, and negative when they avoid each
/// other. The denominator makes the value comparable across tags with different popularity, which is the whole point.
///
/// Counts are fetched as counts and the logarithm is taken here rather than in SQL: `log()` is a compile-time option in
/// SQLite and a different function name in PostgreSQL, and a ranking that silently differs per backend is worse than
/// one that costs a few hundred multiplications.
///
/// Returns `(canonical name, npmi, co-occurrence count)`, highest NPMI first, pairs seen in fewer than two works last —
/// a pair seen once is noise, and its NPMI is 1.0 by construction (P(a,b) = P(a) = P(b)), which would put the rarest
/// accident at the top of the list.
pub async fn related_tags_by_npmi(
    db: &Database,
    tag_norm: &str,
    limit: i64,
) -> Result<Vec<(String, f64, i64)>> {
    let sqlite_pair = "SELECT tn.canonical, COUNT(*) AS pair_count \
         FROM work_tags a \
         JOIN work_tags b ON b.work_id = a.work_id AND b.node_id <> a.node_id \
         JOIN canonical_entities tn ON tn.id = b.node_id \
         WHERE a.node_id = (SELECT id FROM canonical_entities WHERE kind = 'tag' AND norm = ?) \
           AND tn.kind = 'tag' \
         GROUP BY b.node_id \
         ORDER BY pair_count DESC \
         LIMIT 500";
    let postgres_pair = "SELECT tn.canonical, COUNT(*) AS pair_count \
         FROM work_tags a \
         JOIN work_tags b ON b.work_id = a.work_id AND b.node_id <> a.node_id \
         JOIN canonical_entities tn ON tn.id = b.node_id \
         WHERE a.node_id = (SELECT id FROM canonical_entities WHERE kind = 'tag' AND norm = $1) \
           AND tn.kind = 'tag' \
         GROUP BY tn.canonical \
         ORDER BY pair_count DESC \
         LIMIT 500";

    let pairs: Vec<(String, i64)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(sqlite_pair)
                .bind(tag_norm)
                .fetch_all(db.sqlite_pool().expect("sqlite"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(postgres_pair)
                .bind(tag_norm)
                .fetch_all(db.postgres_pool().expect("postgres"))
                .await?
        }
    };
    if pairs.is_empty() {
        return Ok(Vec::new());
    }

    // The three denominators NPMI needs: how many works carry the tag, how many carry each candidate, and how many
    // work-tag links exist in total (the probability space).
    let total_sql = db.sql(
        "SELECT COUNT(*) FROM work_tags",
        "SELECT COUNT(*)::bigint FROM work_tags",
    );
    let total: i64 = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar(&total_sql)
                .fetch_one(db.sqlite_pool().expect("sqlite"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar(&total_sql)
                .fetch_one(db.postgres_pool().expect("postgres"))
                .await?
        }
    };
    if total <= 0 {
        return Ok(Vec::new());
    }

    // One query for every tag's own count, not one per candidate: the candidate list can be hundreds long, and a
    // ranking that costs a round trip per row is a ranking that will be turned off under load.
    let counts_sql = db.sql(
        "SELECT tn.canonical, COUNT(*) FROM work_tags wt JOIN canonical_entities tn ON tn.id = wt.node_id \
          WHERE tn.kind = 'tag' GROUP BY tn.canonical",
        "SELECT tn.canonical, COUNT(*)::bigint FROM work_tags wt JOIN canonical_entities tn ON tn.id = wt.node_id \
          WHERE tn.kind = 'tag' GROUP BY tn.canonical",
    );
    let counts: Vec<(String, i64)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&counts_sql)
                .fetch_all(db.sqlite_pool().expect("sqlite"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&counts_sql)
                .fetch_all(db.postgres_pool().expect("postgres"))
                .await?
        }
    };
    let by_tag: std::collections::HashMap<String, i64> = counts.into_iter().collect();

    // The tag's own canonical name: the caller supplies the normalised form, and the counts table is keyed by the
    // display form, so the bridge is looked up rather than guessed.
    let canonical_sql = db.sql(
        "SELECT canonical FROM canonical_entities WHERE kind = 'tag' AND norm = ?",
        "SELECT canonical FROM canonical_entities WHERE kind = 'tag' AND norm = $1",
    );
    let tag_canonical: Option<String> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar(&canonical_sql)
                .bind(tag_norm)
                .fetch_optional(db.sqlite_pool().expect("sqlite"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar(&canonical_sql)
                .bind(tag_norm)
                .fetch_optional(db.postgres_pool().expect("postgres"))
                .await?
        }
    };
    let Some(tag_canonical) = tag_canonical else {
        return Ok(Vec::new());
    };
    let tag_count = *by_tag.get(&tag_canonical).unwrap_or(&0);
    if tag_count <= 0 {
        return Ok(Vec::new());
    }

    let total_f = total as f64;
    let p_a = tag_count as f64 / total_f;
    let mut scored: Vec<(String, f64, i64)> = Vec::with_capacity(pairs.len());
    for (canonical, pair_count) in pairs {
        if pair_count < 2 {
            continue;
        }
        let other_count = *by_tag.get(&canonical).unwrap_or(&0);
        if other_count <= 0 {
            continue;
        }
        let p_ab = pair_count as f64 / total_f;
        let p_b = other_count as f64 / total_f;
        // p_ab < 1 whenever the pair is not the whole space, so the denominator is defined.
        let denominator = -p_ab.ln();
        if denominator <= 0.0 {
            continue;
        }
        let npmi = ((p_ab / (p_a * p_b)).ln()) / denominator;
        scored.push((canonical, npmi, pair_count));
    }

    scored.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.2.cmp(&a.2))
            .then_with(|| a.0.cmp(&b.0))
    });
    scored.truncate(limit.clamp(1, 100) as usize);
    Ok(scored)
}

/// Tag names plus their work-tag weights for a work, ordered by canonical name.
pub async fn tag_weights_for_work(
    db: &Database,
    work_id: &str,
) -> Result<Vec<(String, i64)>, sqlx::Error> {
    let sql = db.sql(
        "SELECT tn.canonical, wt.prominence
         FROM work_tags wt
         JOIN canonical_entities tn ON tn.id = wt.node_id
         WHERE wt.work_id = ? AND tn.kind = 'tag'
         ORDER BY tn.canonical ASC",
        "SELECT tn.canonical, wt.prominence::bigint
         FROM work_tags wt
         JOIN canonical_entities tn ON tn.id = wt.node_id
         WHERE wt.work_id = $1::uuid AND tn.kind = 'tag'
         ORDER BY tn.canonical ASC",
    );
    let rows: Vec<(String, i64)> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(work_id)
                .fetch_all(db.sqlite_pool().expect("sqlite"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(work_id)
                .fetch_all(db.postgres_pool().expect("postgres"))
                .await?
        }
    };
    Ok(rows)
}

/// List all fandoms (taxonomy nodes of kind 'fandom') with work counts, for the Fandoms surface (spec §43.1).
pub async fn list_fandoms(
    db: &Database,
    limit: i64,
    offset: i64,
) -> Result<Vec<(String, String, i64)>> {
    let sql = db.sql(
        "SELECT id, canonical, (
            SELECT COUNT(*) FROM work_tags wt WHERE wt.node_id = canonical_entities.id
        ) AS work_count
         FROM canonical_entities
         WHERE kind = 'fandom'
         ORDER BY canonical ASC
         LIMIT ? OFFSET ?",
        "SELECT id::text, canonical, (
            SELECT COUNT(*) FROM work_tags wt WHERE wt.node_id = canonical_entities.id
        ) AS work_count
         FROM canonical_entities
         WHERE kind = 'fandom'
         ORDER BY canonical ASC
         LIMIT $1 OFFSET $2",
    );
    let rows = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as::<_, (String, String, i64)>(&sql)
                .bind(limit)
                .bind(offset)
                .fetch_all(db.sqlite_pool().expect("sqlite"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as::<_, (String, String, i64)>(&sql)
                .bind(limit)
                .bind(offset)
                .fetch_all(db.postgres_pool().expect("postgres"))
                .await?
        }
    };
    Ok(rows)
}

/// A warning declared on a work (spec §10.2).
///
/// **A declared attribute and never a detection.** The archive does not
/// detect generated content, and it does not detect warnings either: it
/// records what the author said about their own work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkWarning {
    /// The work the warning is on.
    pub work_id: String,
    /// The unit it is scoped to, when it is scoped to one.
    pub unit_id: Option<String>,
    /// The taxonomy node naming the warning.
    pub warning_node_id: String,
    /// The node's canonical name, so a reader sees "graphic violence" and not
    /// an opaque identifier.
    pub canonical: String,
    /// 1 = light, 2 = heavy.
    pub severity: i64,
    /// `on_page`, `referenced` or `implied`.
    pub depiction: String,
    /// Who said so, and in what capacity.
    pub declaration: String,
    /// The author's note for a reader who wants to avoid the detail.
    pub spoiler_safe_note: Option<String>,
}

/// What to declare on a work.
pub struct WorkWarningInput<'a> {
    /// The work.
    pub work_id: &'a str,
    /// The unit it is scoped to, when it is scoped to one.
    pub unit_id: Option<&'a str>,
    /// The taxonomy node naming the warning.
    pub warning_node_id: &'a str,
    /// 1 = light, 2 = heavy.
    pub severity: i64,
    /// `on_page`, `referenced` or `implied`.
    pub depiction: &'a str,
    /// Who said so, and in what capacity.
    pub declaration: &'a str,
    /// The author's note for a reader who wants to avoid the detail.
    pub spoiler_safe_note: Option<&'a str>,
}

/// Declare a warning on a work.
///
/// The warning is **taxonomy-typed**: the node named is an ordinary taxonomy
/// node of kind `warning`, so a warning is searchable with the rest of a
/// work's metadata rather than living in free text that no query can reach.
///
/// `declaration` is kept as the author said it. `none_apply` and
/// `creator_chose_not_to_say` are **different statements** and must not be
/// collapsed: the second is deliberate silence, and collapsing them would let
/// a creator's silence read as a clean bill of health.
pub async fn declare_work_warning(db: &Database, input: WorkWarningInput<'_>) -> Result<()> {
    let WorkWarningInput {
        work_id,
        unit_id,
        warning_node_id,
        severity,
        depiction,
        declaration,
        spoiler_safe_note,
    } = input;
    let now = crate::identity::now_rfc3339();
    let sql = db.sql(
        "INSERT INTO work_warnings
             (work_id, unit_id, warning_node_id, severity, depiction, declaration,
              spoiler_safe_note, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        "INSERT INTO work_warnings
             (work_id, unit_id, warning_node_id, severity, depiction, declaration,
              spoiler_safe_note, created_at, updated_at)
         VALUES ($1::uuid, $2, $3, $4, $5, $6, $7, $8, $9)",
    );
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(work_id)
                .bind(unit_id)
                .bind(warning_node_id)
                .bind(severity)
                .bind(depiction)
                .bind(declaration)
                .bind(spoiler_safe_note)
                .bind(&now)
                .bind(&now)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .bind(work_id)
                .bind(unit_id)
                .bind(warning_node_id)
                .bind(severity)
                .bind(depiction)
                .bind(declaration)
                .bind(spoiler_safe_note)
                .bind(&now)
                .bind(&now)
                .execute(db.postgres_pool().expect("postgres"))
                .await?;
        }
    }
    Ok(())
}

/// The warnings declared on a work, with the node's name.
///
/// **The warnings are attached to the work, not to the filter.** A reader who
/// filters nothing sees everything, *with the warnings attached* (spec §10.2) --
/// so the listing is what every reader sees, and the filter is what some
/// readers choose to do about it.
pub async fn list_work_warnings(db: &Database, work_id: &str) -> Result<Vec<WorkWarning>> {
    let sql = db.sql(
        "SELECT w.work_id, w.unit_id, w.warning_node_id, tn.canonical, w.severity,
                w.depiction, w.declaration, w.spoiler_safe_note
           FROM work_warnings w
           JOIN canonical_entities tn ON tn.id = w.warning_node_id
          WHERE w.work_id = ?
          ORDER BY w.severity DESC, tn.canonical ASC",
        "SELECT w.work_id::text, w.unit_id, w.warning_node_id, tn.canonical,
                CAST(w.severity AS BIGINT) AS severity,
                w.depiction, w.declaration, w.spoiler_safe_note
           FROM work_warnings w
           JOIN canonical_entities tn ON tn.id = w.warning_node_id
          WHERE w.work_id::text = $1
          ORDER BY w.severity DESC, tn.canonical ASC",
    );
    // The row is read through a named struct rather than a tuple of eight:
    // clippy's type-complexity rule is right that a tuple of eight elements
    // is not readable, and the alternative -- eight positional binds at the
    // call site -- is where a swapped pair would hide.
    #[derive(sqlx::FromRow)]
    struct WarningRow {
        work_id: String,
        unit_id: Option<String>,
        warning_node_id: String,
        canonical: String,
        severity: i64,
        depiction: String,
        declaration: String,
        spoiler_safe_note: Option<String>,
    }

    let rows: Vec<WarningRow> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(work_id)
                .fetch_all(db.sqlite_pool().expect("sqlite"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(work_id)
                .fetch_all(db.postgres_pool().expect("postgres"))
                .await?
        }
    };
    Ok(rows
        .into_iter()
        .map(|row| WorkWarning {
            work_id: row.work_id,
            unit_id: row.unit_id,
            warning_node_id: row.warning_node_id,
            canonical: row.canonical,
            severity: row.severity,
            depiction: row.depiction,
            declaration: row.declaration,
            spoiler_safe_note: row.spoiler_safe_note,
        })
        .collect())
}

/// A reader's content filter, as the pool query needs it.
///
/// **`(kind, norm)` and not a bare value.** The taxonomy's unique key is
/// `(kind, norm)` -- the same name is a tag under one kind and a fandom
/// under another -- so a filter that carried only its value could not say
/// which of them it meant, and would exclude the wrong works.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentFilter {
    /// The taxonomy kind: `tag`, `fandom` or `warning`.
    pub kind: String,
    /// The node's normalised name.
    pub norm: String,
}

/// The works a tag listing shows a reader, and how many there are in total.
///
/// The **total** is the count *after* the filter, not before it: a reader
/// who filters "graphic violence" sees a filtered list, and the count has to
/// agree with the list or the reader is told there are forty works and shown
/// twelve with no way to tell which twelve.
pub struct FilteredWorks {
    /// The page of works.
    pub works: Vec<(String, String, String, String)>,
    /// How many works match, before the page is cut.
    pub total: i64,
}

/// The node id for one canonical name, if the vocabulary has it.
///
/// `norm` rather than `canonical`: the browse surfaces take the normalised name (`/tags/{tag}` matches `tn.norm`), and matching
/// the display form instead would make `/tags/Angst` and `/tags/angst` two different questions with one answer.
pub async fn node_id_by_norm(db: &Database, kind: &str, norm: &str) -> Result<Option<String>> {
    let sql = db.sql(
        "SELECT id FROM canonical_entities WHERE kind = ? AND norm = ?",
        "SELECT id::text FROM canonical_entities WHERE kind = $1 AND norm = $2",
    );
    match db.backend() {
        Backend::Sqlite => sqlx::query_scalar(&sql)
            .bind(kind)
            .bind(norm)
            .fetch_optional(db.sqlite_pool().expect("sqlite"))
            .await
            .map_err(Into::into),
        Backend::Postgres => sqlx::query_scalar(&sql)
            .bind(kind)
            .bind(norm)
            .fetch_optional(db.postgres_pool().expect("postgres"))
            .await
            .map_err(Into::into),
    }
}

/// The normalised names for a set of node ids.
///
/// The `+children` expansion answers with **ids** (the closure is keyed by them) and the work listing matches **names**; this is
/// the one place the two vocabularies meet, and it is a query rather than string handling because a node's name is data.
pub async fn norms_for_node_ids(db: &Database, ids: &[String]) -> Result<Vec<String>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let sqlite_marks = vec!["?"; ids.len()].join(", ");
    let postgres_marks = (1..=ids.len())
        .map(|index| format!("${index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let sqlite_sql = format!("SELECT norm FROM canonical_entities WHERE id IN ({sqlite_marks})");
    let postgres_sql =
        format!("SELECT norm FROM canonical_entities WHERE id IN ({postgres_marks})");
    let sql = db.sql(&sqlite_sql, &postgres_sql);
    let rows: Vec<(String,)> = match db.backend() {
        Backend::Sqlite => {
            let mut query = sqlx::query_as(&sql);
            for id in ids {
                query = query.bind(id);
            }
            query.fetch_all(db.sqlite_pool().expect("sqlite")).await?
        }
        Backend::Postgres => {
            let mut query = sqlx::query_as(&sql);
            for id in ids {
                query = query.bind(id);
            }
            query
                .fetch_all(db.postgres_pool().expect("postgres"))
                .await?
        }
    };
    Ok(rows.into_iter().map(|row| row.0).collect())
}

/// List works tagged with a specific tag node, for the /tags/:tag surface (spec §43.1).
///
/// **The reader's filters are applied INSIDE the query** (spec §12.7): *"Filters
/// apply inside the pool query, before scoring. Filtering a finished result
/// list returns a subset of a *ranking*, which is not the same as a ranking
/// over a smaller pool."*
///
/// That is not a performance argument, it is a correctness one. A filter
/// applied after the list is built removes works from a *ranking* -- and the
/// ranking was computed over a pool that included them, so the order the
/// reader sees is the order of a pool they did not ask for. Filtering first
/// changes what is ranked.
///
/// Every filter becomes its own `NOT EXISTS`, so the exclusion is expressed
/// against the work rather than against a list of ids, and the count the
/// query reports is the count of what survived.
pub async fn works_by_tag_filtered(
    db: &Database,
    tag_canonical: &str,
    limit: i64,
    offset: i64,
    filters: &[ContentFilter],
) -> Result<FilteredWorks> {
    works_by_tags_filtered(db, &[tag_canonical.to_owned()], limit, offset, filters).await
}

/// The same listing for **several** tags at once, which is what §11.4's `+children` expansion needs.
///
/// One statement, not one call per tag: paging and `total` are properties of the *result*, and N calls would each page separately
/// (`limit` per tag, `total` per tag) — a reader asking for 20 with a parent and three children would get up to 80 rows and a
/// total that means nothing. The membership test is an `IN` subquery rather than a join, because a work carrying both the parent
/// tag and one of its children is one work and must be one row; a join would duplicate it and inflate `COUNT(*) OVER ()`.
pub async fn works_by_tags_filtered(
    db: &Database,
    tags: &[String],
    limit: i64,
    offset: i64,
    filters: &[ContentFilter],
) -> Result<FilteredWorks> {
    // An empty list is not "every work": it is a request that names no tag, and the honest answer is an empty page. Returning the
    // catalogue for `IN ()` (or for a malformed `IN` the builder cannot construct) would be a browse surface showing everything to
    // a reader who asked for something specific.
    if tags.is_empty() {
        return Ok(FilteredWorks {
            works: Vec::new(),
            total: 0,
        });
    }
    let sqlite_marks = vec!["?"; tags.len()].join(", ");
    let postgres_marks = (1..=tags.len())
        .map(|index| format!("${index}"))
        .collect::<Vec<_>>()
        .join(", ");

    // The two kinds of exclusion are different tables: a tag or a fandom is a
    // `work_tags` row, and a warning is a `work_warnings` row.
    //
    // **The clauses and the binds are built together, in one order.** A
    // statement's placeholders are bound by position, so a list of clauses
    // and a list of values that are not in the same order bind each clause to
    // the wrong value -- and the query still runs, and filters the wrong
    // works. Building them together is what makes that impossible.
    let mut sqlite_clauses: Vec<String> = Vec::new();
    let mut postgres_clauses: Vec<String> = Vec::new();
    let mut binds: Vec<String> = Vec::new();
    // The PostgreSQL numbering starts after the tag placeholders, which occupy `$1..$n`. It is written out here rather than
    // rewritten later, because the two dialects' clauses are different statements -- see the note below.
    let mut pg_index = tags.len() + 1;
    for filter in filters {
        match filter.kind.as_str() {
            "tag" | "fandom" => {
                sqlite_clauses.push(
                    "NOT EXISTS (SELECT 1 FROM work_tags fwt \
                       JOIN canonical_entities ftn ON ftn.id = fwt.node_id \
                      WHERE fwt.work_id = w.id AND ftn.kind = ? AND ftn.norm = ?)"
                        .to_owned(),
                );
                postgres_clauses.push(format!(
                    "NOT EXISTS (SELECT 1 FROM work_tags fwt \
                       JOIN canonical_entities ftn ON ftn.id = fwt.node_id \
                      WHERE fwt.work_id = w.id AND ftn.kind = ${pg_index} AND ftn.norm = ${})",
                    pg_index + 1
                ));
                pg_index += 2;
                binds.push(filter.kind.clone());
                binds.push(filter.norm.clone());
            }
            "warning" => {
                sqlite_clauses.push(
                    "NOT EXISTS (SELECT 1 FROM work_warnings fww \
                       JOIN canonical_entities ftn ON ftn.id = fww.warning_node_id \
                      WHERE fww.work_id = w.id AND ftn.kind = 'warning' AND ftn.norm = ?)"
                        .to_owned(),
                );
                postgres_clauses.push(format!(
                    "NOT EXISTS (SELECT 1 FROM work_warnings fww \
                       JOIN canonical_entities ftn ON ftn.id = fww.warning_node_id \
                      WHERE fww.work_id = w.id AND ftn.kind = 'warning' AND ftn.norm = ${pg_index})"
                ));
                pg_index += 1;
                binds.push(filter.norm.clone());
            }
            // An unknown filter type is a **no-op**, not an error and not a
            // blanket exclusion. The table's CHECK keeps the set closed, so
            // this arm is unreachable in practice; and if it were reached, the
            // honest reading of "a filter I do not understand" is "I do not
            // know how to apply it", not "hide everything".
            _ => {}
        }
    }

    let sqlite_clause_text = if sqlite_clauses.is_empty() {
        String::new()
    } else {
        format!(" AND {}", sqlite_clauses.join(" AND "))
    };
    let postgres_clause_text = if postgres_clauses.is_empty() {
        String::new()
    } else {
        format!(" AND {}", postgres_clauses.join(" AND "))
    };

    let sqlite = format!(
        "SELECT w.id, w.title, p.handle, w.updated_at, COUNT(*) OVER () AS total
         FROM works w
         JOIN pseuds p ON p.id = w.owner_pseud_id
         WHERE w.id IN (
                 SELECT wt.work_id FROM work_tags wt
                   JOIN canonical_entities tn ON tn.id = wt.node_id
                  WHERE tn.norm IN ({sqlite_marks}))
           AND w.lifecycle = 'published' AND w.visibility = 'public'{sqlite_clause_text}
         ORDER BY w.updated_at DESC
         LIMIT ? OFFSET ?"
    );
    let postgres = format!(
        "SELECT w.id::text, w.title, p.handle, w.updated_at, COUNT(*) OVER () AS total
         FROM works w
         JOIN pseuds p ON p.id = w.owner_pseud_id
         WHERE w.id IN (
                 SELECT wt.work_id FROM work_tags wt
                   JOIN canonical_entities tn ON tn.id = wt.node_id
                  WHERE tn.norm IN ({postgres_marks}))
           AND w.lifecycle = 'published' AND w.visibility = 'public'{postgres_clause_text}
         ORDER BY w.updated_at DESC
         LIMIT ${} OFFSET ${}",
        binds.len() + tags.len() + 1,
        binds.len() + tags.len() + 2
    );

    // The `?` in the SQLite arm and the `$n` in the PostgreSQL arm are
    // **different statements**: the filter clauses are built with `?`, and
    // the PostgreSQL arm's own numbering is written out. So the PostgreSQL
    // arm cannot go through `rewrite_placeholders` -- it would renumber the
    // `?`s and collide with the `$n`s already there, which is the fault
    // `scripts/check-pg-placeholder-mixing.py` exists to catch. The
    // PostgreSQL statement is therefore built with explicit numbers from the
    // start, and the SQLite one is rewritten as usual.
    let sql = match db.backend() {
        Backend::Sqlite => rewrite_placeholders(&sqlite),
        Backend::Postgres => postgres,
    };

    // The query is built **inside** each arm: the two statements differ, and
    // `sqlx`'s query type is tied to the executor it will run on.
    let rows: Vec<(String, String, String, String, i64)> = match db.backend() {
        Backend::Sqlite => {
            let mut query = sqlx::query_as::<_, (String, String, String, String, i64)>(&sql);
            for tag in tags {
                query = query.bind(tag);
            }
            for bind in &binds {
                query = query.bind(bind);
            }
            query
                .bind(limit)
                .bind(offset)
                .fetch_all(db.sqlite_pool().expect("sqlite"))
                .await?
        }
        Backend::Postgres => {
            let mut query = sqlx::query_as::<_, (String, String, String, String, i64)>(&sql);
            for tag in tags {
                query = query.bind(tag);
            }
            for bind in &binds {
                query = query.bind(bind);
            }
            query
                .bind(limit)
                .bind(offset)
                .fetch_all(db.postgres_pool().expect("postgres"))
                .await?
        }
    };

    let total = rows.first().map(|row| row.4).unwrap_or(0);
    Ok(FilteredWorks {
        works: rows
            .into_iter()
            .map(|row| (row.0, row.1, row.2, row.3))
            .collect(),
        total,
    })
}

/// List works tagged with a specific tag node, for the /tags/:tag surface (spec §43.1).
pub async fn works_by_tag(
    db: &Database,
    tag_canonical: &str,
    limit: i64,
    offset: i64,
) -> Result<Vec<(String, String, String, String)>> {
    let sql = db.sql(
        "SELECT w.id, w.title, p.handle, w.updated_at
         FROM works w
         JOIN work_tags wt ON wt.work_id = w.id
         JOIN canonical_entities tn ON tn.id = wt.node_id
         JOIN pseuds p ON p.id = w.owner_pseud_id
         WHERE tn.norm = ? AND w.lifecycle = 'published' AND w.visibility = 'public'
         ORDER BY w.updated_at DESC
         LIMIT ? OFFSET ?",
        "SELECT w.id::text, w.title, p.handle, w.updated_at
         FROM works w
         JOIN work_tags wt ON wt.work_id = w.id
         JOIN canonical_entities tn ON tn.id = wt.node_id
         JOIN pseuds p ON p.id = w.owner_pseud_id
         WHERE tn.norm = $1 AND w.lifecycle = 'published' AND w.visibility = 'public'
         ORDER BY w.updated_at DESC
         LIMIT $2 OFFSET $3",
    );
    let rows = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as::<_, (String, String, String, String)>(&sql)
                .bind(tag_canonical)
                .bind(limit)
                .bind(offset)
                .fetch_all(db.sqlite_pool().expect("sqlite"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as::<_, (String, String, String, String)>(&sql)
                .bind(tag_canonical)
                .bind(limit)
                .bind(offset)
                .fetch_all(db.postgres_pool().expect("postgres"))
                .await?
        }
    };
    Ok(rows)
}

/// List works tagged with a specific fandom node, for the /fandoms/:fandom surface (spec §43.1).
pub async fn works_by_fandom(
    db: &Database,
    fandom_canonical: &str,
    limit: i64,
    offset: i64,
) -> Result<Vec<(String, String, String, String)>> {
    let sql = db.sql(
        "SELECT w.id, w.title, p.handle, w.updated_at
         FROM works w
         JOIN work_tags wt ON wt.work_id = w.id
         JOIN canonical_entities tn ON tn.id = wt.node_id
         JOIN pseuds p ON p.id = w.owner_pseud_id
         WHERE tn.norm = ? AND w.lifecycle = 'published' AND w.visibility = 'public'
         ORDER BY w.updated_at DESC
         LIMIT ? OFFSET ?",
        "SELECT w.id::text, w.title, p.handle, w.updated_at
         FROM works w
         JOIN work_tags wt ON wt.work_id = w.id
         JOIN canonical_entities tn ON tn.id = wt.node_id
         JOIN pseuds p ON p.id = w.owner_pseud_id
         WHERE tn.norm = $1 AND w.lifecycle = 'published' AND w.visibility = 'public'
         ORDER BY w.updated_at DESC
         LIMIT $2 OFFSET $3",
    );
    let rows = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as::<_, (String, String, String, String)>(&sql)
                .bind(fandom_canonical)
                .bind(limit)
                .bind(offset)
                .fetch_all(db.sqlite_pool().expect("sqlite"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as::<_, (String, String, String, String)>(&sql)
                .bind(fandom_canonical)
                .bind(limit)
                .bind(offset)
                .fetch_all(db.postgres_pool().expect("postgres"))
                .await?
        }
    };
    Ok(rows)
}

/// Get all node_ids tagged on a work, in insertion order.
pub async fn tags_for_work(db: &Database, work_id: &str) -> Result<Vec<String>> {
    let sql = db.sql(
        "SELECT node_id FROM work_tags WHERE work_id = ? ORDER BY added_at",
        "SELECT node_id FROM work_tags WHERE work_id = $1::uuid ORDER BY added_at",
    );
    let rows: Vec<String> = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar(&sql)
                .bind(work_id)
                .fetch_all(db.sqlite_pool().expect("sqlite handle"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar(&sql)
                .bind(work_id)
                .fetch_all(db.postgres_pool().expect("postgres handle"))
                .await?
        }
    };
    Ok(rows)
}

/// How much of the catalogue one dimension actually covers (M71-13).
///
/// A tag page that says "12 works" invites the reader to think it is the whole of that tag, and a facet that offers a value offers it as if
/// everything had been measured. Both are the same missing fact: **how much of the archive this dimension describes**. So the numbers travel
/// with the answer — `works` in the view, `catalogue` published-and-public works in the archive, and the share between them — and the
/// sentence is built here rather than by each surface, because two pages phrasing the same gap differently is how a reader learns to
/// distrust both.
///
/// The catalogue count is `published` + `public` + not deleted, the same definition every listing uses; counting drafts would make every
/// share look smaller than the archive a reader can actually reach.
#[derive(Debug, Clone, Serialize)]
pub struct Coverage {
    /// What is being counted: `tag`, `fandom`, `collection`.
    pub dimension: String,
    /// The value the count is about, as the caller asked for it.
    pub value: String,
    /// Published, public, undeleted works carrying this dimension.
    pub works: i64,
    /// Published, public, undeleted works in the whole archive.
    pub catalogue: i64,
    /// `works / catalogue`, or `0.0` when the archive is empty — never a division by zero dressed as a measurement.
    pub share: f64,
    /// One sentence a page can print verbatim.
    pub sentence: String,
}

/// The coverage of one `(dimension, value)` pair.
pub async fn dimension_coverage(db: &Database, dimension: &str, value: &str) -> Result<Coverage> {
    let norm = darkhold_domain::exchange::normalise(value);
    // One statement, two scalar counts: a second round trip would let the catalogue change between the halves and report a share of two
    // different archives.
    let sqlite = "SELECT
            (SELECT COUNT(DISTINCT w.id) FROM works w
               JOIN work_tags wt ON wt.work_id = w.id
               JOIN canonical_entities tn ON tn.id = wt.node_id
              WHERE tn.norm = ?1 AND w.lifecycle = 'published' AND w.visibility = 'public' AND w.deleted_at IS NULL),
            (SELECT COUNT(*) FROM works w
              WHERE w.lifecycle = 'published' AND w.visibility = 'public' AND w.deleted_at IS NULL)";
    let postgres = "SELECT
            (SELECT COUNT(DISTINCT w.id) FROM works w
               JOIN work_tags wt ON wt.work_id = w.id
               JOIN canonical_entities tn ON tn.id = wt.node_id
              WHERE tn.norm = $1 AND w.lifecycle = 'published' AND w.visibility = 'public' AND w.deleted_at IS NULL),
            (SELECT COUNT(*) FROM works w
              WHERE w.lifecycle = 'published' AND w.visibility = 'public' AND w.deleted_at IS NULL)";
    let sql = db.sql(sqlite, postgres);
    let (works, catalogue): (i64, i64) = match db.backend() {
        Backend::Sqlite => {
            sqlx::query_as(&sql)
                .bind(&norm)
                .fetch_one(
                    db.sqlite_pool()
                        .ok_or_else(|| anyhow::anyhow!("no sqlite pool on a sqlite database"))?,
                )
                .await?
        }
        Backend::Postgres => {
            sqlx::query_as(&sql)
                .bind(&norm)
                .fetch_one(
                    db.postgres_pool().ok_or_else(|| {
                        anyhow::anyhow!("no postgres pool on a postgres database")
                    })?,
                )
                .await?
        }
    };
    let share = if catalogue > 0 {
        works as f64 / catalogue as f64
    } else {
        0.0
    };
    Ok(Coverage {
        dimension: dimension.to_owned(),
        value: value.to_owned(),
        works,
        catalogue,
        share,
        sentence: coverage_sentence(dimension, value, works, catalogue, share),
    })
}

fn coverage_sentence(
    dimension: &str,
    value: &str,
    works: i64,
    catalogue: i64,
    share: f64,
) -> String {
    if catalogue == 0 {
        // An empty archive: a percentage of nothing is not a measurement, and "0 of 0" reads as a fault.
        return "This archive has no published works yet, so there is nothing to measure against."
            .to_owned();
    }
    // A verb phrase, because "2 of 5 works are works tagged x" is what a template produces when nobody reads it back.
    let predicate = match dimension {
        "tag" => format!("tagged {value}"),
        "fandom" | "collection" => format!("in {value}"),
        other => format!("carrying this {other}"),
    };
    if works == 0 {
        // A dimension page exists because something carried it, so zero means the rows went away since the page was linked. Saying
        // that is more useful than a straight-faced "0.0%".
        return format!("No published work in this archive is {predicate} any more.");
    }
    format!(
        "{works} of {catalogue} works in this archive are {predicate} ({percent:.1}%).",
        percent = share * 100.0
    )
}

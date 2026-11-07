//! Trope clusters and the coverage checklist (M69-18).
//!
//! A tag count says how much a work names and never what it does not. A cluster is a named family of taxonomy nodes with an
//! operator-set priority; the checklist for one work is derived from the clusters and the work's own tags:
//!
//! * **named** — the nodes of that cluster the work carries. Reported, because the writer may not know two of them are the same
//!   family and that is useful on its own.
//! * **missing** — how many of the cluster's nodes the work does not carry. A count, not a list: the point is "this family is
//!   unknown to this work", and a list of specific gaps turns an advisory checklist into a to-do list somebody grades.
//!
//! **Advisory is a design constraint, not a tone.** Nothing here writes. `coverage_for_work` reads; the checklist never creates a
//! tag, never blocks publishing, and never appears in a ranking. A work lacking a trope knows it has none, which is all "coverage"
//! has ever meant.
//!
//! Priority orders the clusters. Ties break on slug so the checklist is stable between two reads — a list that reorders itself
//! when two rows share a priority reads as a suggestion that changed its mind.

use crate::{identity::now_rfc3339, Backend, Database, Result};
use serde::Serialize;

/// One cluster: a named family of nodes.
#[derive(Debug, Clone, Serialize)]
pub struct TropeCluster {
    /// Identifier.
    pub id: String,
    /// Stable address, lowercase with single hyphens.
    pub slug: String,
    /// Display name.
    pub label: String,
    /// What the family is, for the operator and the checklist.
    pub description: Option<String>,
    /// Lower sorts first.
    pub priority: i64,
    /// The member nodes, in insertion order.
    pub node_ids: Vec<String>,
}

/// One work's standing against one cluster.
#[derive(Debug, Clone, Serialize)]
pub struct ClusterCoverage {
    /// The cluster's slug.
    pub slug: String,
    /// Its display name.
    pub label: String,
    /// Its priority.
    pub priority: i64,
    /// The cluster's nodes this work carries. Empty means the family is unknown to the work.
    pub named: Vec<String>,
    /// How many of the cluster's nodes it does not carry.
    pub missing: i64,
}

impl ClusterCoverage {
    /// Whether the work names at least one node of this family.
    pub fn is_named(&self) -> bool {
        !self.named.is_empty()
    }
}

/// Make a cluster.
pub async fn create_cluster(
    db: &Database,
    slug: &str,
    label: &str,
    description: Option<&str>,
    priority: i64,
) -> Result<TropeCluster> {
    let slug = slug.trim();
    if !valid_slug(slug) {
        anyhow::bail!(
            "a cluster slug is lowercase letters, digits and single hyphens, up to 64 characters"
        );
    }
    if label.trim().is_empty() {
        anyhow::bail!("a cluster needs a label");
    }
    let id = uuid::Uuid::new_v4().to_string();
    let now = now_rfc3339();
    let sql = db.sql(
        "INSERT INTO trope_clusters (id, slug, label, description, priority, created_at, updated_at, version)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, 1)",
        "INSERT INTO trope_clusters (id, slug, label, description, priority, created_at, updated_at, version)
         VALUES ($1::uuid, $2, $3, $4, $5, $6, $6, 1)",
    );
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&sql)
                .bind(&id)
                .bind(slug)
                .bind(label.trim())
                .bind(description)
                .bind(priority)
                .bind(&now)
                .execute(db.sqlite_pool().expect("sqlite handle"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&sql)
                .bind(&id)
                .bind(slug)
                .bind(label.trim())
                .bind(description)
                .bind(priority)
                .bind(&now)
                .execute(db.postgres_pool().expect("postgres handle"))
                .await?;
        }
    }
    Ok(TropeCluster {
        id,
        slug: slug.to_owned(),
        label: label.trim().to_owned(),
        description: description.map(str::to_owned),
        priority,
        node_ids: Vec::new(),
    })
}

/// Slug rules: a refusal, never a normalisation. `Enemies to Lovers` is refused rather than quietly stored as `enemies-to-lovers`,
/// because two operators who wrote two spellings must learn that they did.
pub fn valid_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug.len() <= 64
        && !slug.starts_with('-')
        && !slug.ends_with('-')
        && !slug.contains("--")
        && slug
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Add a node to a cluster. Idempotent.
pub async fn add_node(db: &Database, slug: &str, node_id: &str) -> Result<bool> {
    let sql = db.sql(
        "INSERT INTO trope_cluster_nodes (cluster_id, node_id)
         SELECT id, ?2 FROM trope_clusters WHERE slug = ?1
         ON CONFLICT (cluster_id, node_id) DO NOTHING",
        "INSERT INTO trope_cluster_nodes (cluster_id, node_id)
         SELECT id, $2 FROM trope_clusters WHERE slug = $1
         ON CONFLICT (cluster_id, node_id) DO NOTHING",
    );
    let affected = match db.backend() {
        Backend::Sqlite => sqlx::query(&sql)
            .bind(slug)
            .bind(node_id)
            .execute(db.sqlite_pool().expect("sqlite handle"))
            .await?
            .rows_affected(),
        Backend::Postgres => sqlx::query(&sql)
            .bind(slug)
            .bind(node_id)
            .execute(db.postgres_pool().expect("postgres handle"))
            .await?
            .rows_affected(),
    };
    Ok(affected > 0)
}

/// One row of `all_clusters`, named so the tuple does not have to be read twice.
type ClusterRow = (String, String, String, Option<String>, i64, Option<String>);

/// Every cluster, priority-ordered, ties broken on slug.
pub async fn all_clusters(db: &Database) -> Result<Vec<TropeCluster>> {
    let sql = db.sql(
        "SELECT CAST(c.id AS TEXT), c.slug, c.label, c.description, c.priority,
                (SELECT GROUP_CONCAT(n.node_id, ',') FROM (
                    SELECT node_id FROM trope_cluster_nodes WHERE cluster_id = c.id ORDER BY node_id
                ) n)
           FROM trope_clusters c ORDER BY c.priority, c.slug",
        "SELECT c.id::text, c.slug, c.label, c.description, c.priority,
                (SELECT string_agg(n.node_id, ',' ORDER BY n.node_id)
                   FROM trope_cluster_nodes n WHERE n.cluster_id = c.id)
           FROM trope_clusters c ORDER BY c.priority, c.slug",
    );
    let rows: Vec<ClusterRow> = match db.backend() {
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
        .map(
            |(id, slug, label, description, priority, joined)| TropeCluster {
                id,
                slug,
                label,
                description,
                priority,
                node_ids: joined
                    .filter(|value| !value.is_empty())
                    .map(|value| value.split(',').map(str::to_owned).collect())
                    .unwrap_or_default(),
            },
        )
        .collect())
}

/// One work's checklist: every cluster, in priority order, with what it names and what it does not.
///
/// A cluster with no nodes is reported with `missing: 0` and `named: []` — it is empty, not unknown to the work, and saying
/// "this work lacks all zero of them" would be noise dressed as information.
pub async fn coverage_for_work(db: &Database, work_id: &str) -> Result<Vec<ClusterCoverage>> {
    let clusters = all_clusters(db).await?;
    if clusters.is_empty() {
        return Ok(Vec::new());
    }
    let carried = carried_nodes(db, work_id).await?;
    Ok(clusters
        .into_iter()
        .map(|cluster| {
            let named: Vec<String> = cluster
                .node_ids
                .iter()
                .filter(|node| carried.contains(node.as_str()))
                .cloned()
                .collect();
            ClusterCoverage {
                slug: cluster.slug,
                label: cluster.label,
                priority: cluster.priority,
                missing: if cluster.node_ids.is_empty() {
                    0
                } else {
                    cluster.node_ids.len() as i64 - named.len() as i64
                },
                named,
            }
        })
        .collect())
}

/// The nodes one work carries, as a set for the coverage walk. Read-facing tags only — a model's unconfirmed proposal is not
/// something a work "names", or the checklist would credit a guess.
async fn carried_nodes(db: &Database, work_id: &str) -> Result<std::collections::BTreeSet<String>> {
    let sql = db.sql(
        "SELECT node_id FROM work_tags
          WHERE work_id = ?1 AND NOT (source = 'inferred' AND confirmation = 'unconfirmed')",
        "SELECT node_id FROM work_tags
          WHERE work_id = $1::uuid AND NOT (source = 'inferred' AND confirmation = 'unconfirmed')",
    );
    let rows: Vec<(String,)> = match db.backend() {
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
    Ok(rows.into_iter().map(|(node,)| node).collect())
}

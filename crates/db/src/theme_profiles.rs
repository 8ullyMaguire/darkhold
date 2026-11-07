//! M16-13 — the theme store: profiles, previews, safe mode.
//!
//! Every decision lives in `darkhold_domain::theme_model` (what a token may hold, which pairs must pass
//! contrast, which slots exist, what safe mode overrides). This module stores what those decisions produced
//! and reconstructs a [`Theme`] from the rows. Two rules it does own, because they are about storage:
//!
//! * **Publishing is a move, not a second row.** A partial unique index allows one published theme per
//!   (scope, owner), and [`publish`] unpublishes the incumbent in the same breath — with two published rows,
//!   "the instance's appearance" would depend on row order.
//! * **A preview is session-scoped and expiring.** It is a row keyed by (theme, session) with an expiry
//!   checked at read, because a preview the client remembers is a preview the client can outlive.

use std::collections::BTreeMap;

use anyhow::Result;
use serde_json::{json, Value};
use sqlx::Row as _;

use darkhold_domain::theme_model::{Placement, SafeMode, Slot, Theme, Token};

use crate::{Backend, Database};

/// Where the instance's own theme lives: `owner` is the empty account for the instance scope.
pub const INSTANCE_OWNER: &str = "";

/// The single row key of the safe-mode table. One instance per database, so a literal key is enough — and a
/// key rather than a row id means `INSERT ... ON CONFLICT` can never make a second row.
const SAFE_MODE_KEY: &str = "default";

/// A stored theme plus the columns a caller needs to act on it.
#[derive(Debug, Clone, PartialEq)]
pub struct Stored {
    /// Row id.
    pub id: String,
    /// `instance` or `reader`.
    pub scope: String,
    /// Account id, or `""` for the instance.
    pub owner: String,
    /// Draft or published.
    pub state: String,
    /// The theme itself.
    pub theme: Theme,
    /// When it was last written.
    pub updated_at: String,
}

/// Parse a palette JSON object into tokens, skipping names the model does not define.
///
/// Skipping rather than failing is deliberate: a token removed from the vocabulary in a later version must
/// not make every stored theme unreadable. The value is still validated by the model on the way in.
fn parse_palette(text: &str) -> std::collections::BTreeMap<Token, String> {
    let parsed: std::collections::BTreeMap<String, String> =
        serde_json::from_str(text).unwrap_or_default();
    let mut palette = std::collections::BTreeMap::new();
    for (name, value) in parsed {
        if let Some(token) = Token::all().iter().find(|t| t.css_name() == name) {
            palette.insert(*token, value);
        }
    }
    palette
}

fn parse_layout(text: &str) -> std::collections::BTreeMap<Slot, Placement> {
    let parsed: std::collections::BTreeMap<String, String> =
        serde_json::from_str(text).unwrap_or_default();
    let mut layout = std::collections::BTreeMap::new();
    for (name, value) in parsed {
        if let (Some(slot), Some(placement)) = (
            Slot::all().iter().find(|s| s.name() == name),
            Placement::parse(&value),
        ) {
            layout.insert(*slot, placement);
        }
    }
    layout
}

fn palette_json(theme: &Theme) -> String {
    let map: std::collections::BTreeMap<&str, &str> = theme
        .palette
        .iter()
        .map(|(token, value)| (token.css_name(), value.as_str()))
        .collect();
    json!(map).to_string()
}

fn layout_json(theme: &Theme) -> String {
    let map: std::collections::BTreeMap<&str, &str> = theme
        .layout
        .iter()
        .map(|(slot, placement)| (slot.name(), placement.name()))
        .collect();
    json!(map).to_string()
}

/// Save (or overwrite) a draft by name.
///
/// Saving never publishes: a draft that published itself would make "save" and "publish" the same act, and
/// then a contrast failure would be the difference between a saved form and a live instance appearance.
pub async fn save(
    db: &Database,
    scope: &str,
    owner: &str,
    theme: &Theme,
    now: &str,
) -> Result<String> {
    let id = uuid_like(scope, owner, &theme.name);
    let (palette, layout) = (palette_json(theme), layout_json(theme));
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO theme_profiles (id, scope, owner, name, palette, layout, state, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, 'draft', ?, ?)
                 ON CONFLICT(id) DO UPDATE SET palette = excluded.palette, layout = excluded.layout, updated_at = excluded.updated_at, state = 'draft'",
            )
            .bind(&id)
            .bind(scope)
            .bind(owner)
            .bind(&theme.name)
            .bind(&palette)
            .bind(&layout)
            .bind(now)
            .bind(now)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await?;
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO theme_profiles (id, scope, owner, name, palette, layout, state, created_at, updated_at)
                 VALUES ($1, $2, $3, $4, $5, $6, 'draft', $7, $8)
                 ON CONFLICT(id) DO UPDATE SET palette = excluded.palette, layout = excluded.layout, updated_at = excluded.updated_at, state = 'draft'",
            )
            .bind(&id)
            .bind(scope)
            .bind(owner)
            .bind(&theme.name)
            .bind(&palette)
            .bind(&layout)
            .bind(now)
            .bind(now)
            .execute(db.postgres_pool().expect("postgres"))
            .await?;
        }
    }

    // Saving demotes a published theme: `state = 'draft'` is part of the upsert, because the bytes that were
    // published are not the bytes just written, and a reader must never see an unapproved draft.
    Ok(id)
}

/// A deterministic id from (scope, owner, name).
///
/// Deterministic so `save` is an upsert rather than an insert with a duplicate trap: the name is how a person
/// refers to the theme, and two rows with one name is a console bug rather than two themes.
fn uuid_like(scope: &str, owner: &str, name: &str) -> String {
    // FNV-1a, written out: the id has to be the same bytes on every build and every restart, and a
    // `DefaultHasher` makes no such promise.
    let digest = format!("{scope}\u{1f}{owner}\u{1f}{name}");
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in digest.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("theme-{hash:016x}")
}

/// Publish a draft, unpublishing whatever was published for the same (scope, owner).
pub async fn publish(db: &Database, scope: &str, owner: &str, id: &str, now: &str) -> Result<bool> {
    let affected = match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "UPDATE theme_profiles SET state = 'draft'
                 WHERE scope = ? AND owner = ? AND state = 'published' AND id <> ?",
            )
            .bind(scope)
            .bind(owner)
            .bind(id)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await?
            .rows_affected()
            + sqlx::query(
                "UPDATE theme_profiles SET state = 'published', updated_at = ?
                 WHERE id = ? AND scope = ? AND owner = ?",
            )
            .bind(now)
            .bind(id)
            .bind(scope)
            .bind(owner)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await?
            .rows_affected()
        }
        Backend::Postgres => {
            sqlx::query(
                "UPDATE theme_profiles SET state = 'draft'
                 WHERE scope = $1 AND owner = $2 AND state = 'published' AND id <> $3",
            )
            .bind(scope)
            .bind(owner)
            .bind(id)
            .execute(db.postgres_pool().expect("postgres"))
            .await?
            .rows_affected()
            + sqlx::query(
                "UPDATE theme_profiles SET state = 'published', updated_at = $1
                 WHERE id = $2 AND scope = $3 AND owner = $4",
            )
            .bind(now)
            .bind(id)
            .bind(scope)
            .bind(owner)
            .execute(db.postgres_pool().expect("postgres"))
            .await?
            .rows_affected()
        }
    };
    // Read back rather than trust a count: a theme published twice is still one published theme, and
    // "did it work" means "is this row the published one now".
    let _ = affected;
    Ok(published(db, scope, owner).await?.is_some_and(|t| t.id == id))
}

/// The published theme for a scope and owner, if any.
pub async fn published(db: &Database, scope: &str, owner: &str) -> Result<Option<Stored>> {
    let sql = "SELECT id, scope, owner, state, palette, layout, updated_at FROM theme_profiles WHERE scope = ? AND owner = ? AND state = 'published' LIMIT 1";
    let sql_pg = "SELECT id, scope, owner, state, palette, layout, updated_at FROM theme_profiles WHERE scope = $1 AND owner = $2 AND state = 'published' LIMIT 1";
    match db.backend() {
        Backend::Sqlite => {
            let row = sqlx::query(sql)
                .bind(scope)
                .bind(owner)
                .fetch_optional(db.sqlite_pool().expect("sqlite"))
                .await?;
            Ok(row.as_ref().map(row_to_stored))
        }
        Backend::Postgres => {
            let row = sqlx::query(sql_pg)
                .bind(scope)
                .bind(owner)
                .fetch_optional(db.postgres_pool().expect("postgres"))
                .await?;
            Ok(row.as_ref().map(row_to_stored_pg))
        }
    }
}

fn row_to_stored(row: &sqlx::sqlite::SqliteRow) -> Stored {
    Stored {
        id: row.get::<String, _>("id"),
        scope: row.get::<String, _>("scope"),
        owner: row.get::<String, _>("owner"),
        state: row.get::<String, _>("state"),
        theme: Theme {
            name: String::new(),
            palette: parse_palette(&row.get::<String, _>("palette")),
            layout: parse_layout(&row.get::<String, _>("layout")),
        },
        updated_at: row.get::<String, _>("updated_at"),
    }
}

fn row_to_stored_pg(row: &sqlx::postgres::PgRow) -> Stored {
    Stored {
        id: row.get::<String, _>("id"),
        scope: row.get::<String, _>("scope"),
        owner: row.get::<String, _>("owner"),
        state: row.get::<String, _>("state"),
        theme: Theme {
            name: String::new(),
            palette: parse_palette(&row.get::<String, _>("palette")),
            layout: parse_layout(&row.get::<String, _>("layout")),
        },
        updated_at: row.get::<String, _>("updated_at"),
    }
}

/// Reset: delete what is published for a scope and owner.
///
/// A delete rather than a write of the core values, because "reset" means "there is no theme", not "there is
/// a theme that happens to equal the default". The second state has to be migrated whenever the default
/// changes; the first never does.
pub async fn reset(db: &Database, scope: &str, owner: &str) -> Result<i64> {
    let affected = match db.backend() {
        Backend::Sqlite => {
            sqlx::query("DELETE FROM theme_profiles WHERE scope = ? AND owner = ? AND state = 'published'")
                .bind(scope)
                .bind(owner)
                .execute(db.sqlite_pool().expect("sqlite"))
                .await?
                .rows_affected()
        }
        Backend::Postgres => {
            sqlx::query("DELETE FROM theme_profiles WHERE scope = $1 AND owner = $2 AND state = 'published'")
                .bind(scope)
                .bind(owner)
                .execute(db.postgres_pool().expect("postgres"))
                .await?
                .rows_affected()
        }
    };
    Ok(i64::try_from(affected).unwrap_or(0))
}

/// Every theme stored for a scope and owner, drafts included.
pub async fn list(db: &Database, scope: &str, owner: &str) -> Result<Vec<Value>> {
    match db.backend() {
        Backend::Sqlite => {
            let rows = sqlx::query(
                "SELECT id, scope, owner, name, state, palette, layout, updated_at FROM theme_profiles WHERE scope = ? AND owner = ? ORDER BY updated_at DESC",
            )
            .bind(scope)
            .bind(owner)
            .fetch_all(db.sqlite_pool().expect("sqlite"))
            .await?;
            Ok(rows.iter().map(list_row).collect())
        }
        Backend::Postgres => {
            let rows = sqlx::query(
                "SELECT id, scope, owner, name, state, palette, layout, updated_at FROM theme_profiles WHERE scope = $1 AND owner = $2 ORDER BY updated_at DESC",
            )
            .bind(scope)
            .bind(owner)
            .fetch_all(db.postgres_pool().expect("postgres"))
            .await?;
            Ok(rows.iter().map(list_row_pg).collect())
        }
    }
}

fn list_row(row: &sqlx::sqlite::SqliteRow) -> Value {
    json!({
        "id": row.get::<String, _>("id"),
        "scope": row.get::<String, _>("scope"),
        "name": row.get::<String, _>("name"),
        "state": row.get::<String, _>("state"),
        "palette": wire_palette(&row.get::<String, _>("palette")),
        "layout": wire_layout(&row.get::<String, _>("layout")),
        "updated_at": row.get::<String, _>("updated_at"),
    })
}

/// The palette as a wire map (`--dh-ink` -> value), which is how a form and a stylesheet both read it.
fn wire_palette(text: &str) -> BTreeMap<String, String> {
    parse_palette(text)
        .into_iter()
        .map(|(token, value)| (token.css_name().to_owned(), value))
        .collect()
}

fn wire_layout(text: &str) -> BTreeMap<String, String> {
    parse_layout(text)
        .into_iter()
        .map(|(slot, placement)| (slot.name().to_owned(), placement.name().to_owned()))
        .collect()
}

fn list_row_pg(row: &sqlx::postgres::PgRow) -> Value {
    json!({
        "id": row.get::<String, _>("id"),
        "scope": row.get::<String, _>("scope"),
        "name": row.get::<String, _>("name"),
        "state": row.get::<String, _>("state"),
        "palette": wire_palette(&row.get::<String, _>("palette")),
        "layout": wire_layout(&row.get::<String, _>("layout")),
        "updated_at": row.get::<String, _>("updated_at"),
    })
}

/// Start a preview for one session.
///
/// Upsert on (theme, session): asking twice for the same preview extends it rather than stacking rows, so
/// "how long has this been previewed" has one answer.
pub async fn start_preview(
    db: &Database,
    theme_id: &str,
    session_id: &str,
    expires_at: &str,
) -> Result<()> {
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO theme_previews (theme_id, session_id, expires_at) VALUES (?, ?, ?)
                 ON CONFLICT(theme_id, session_id) DO UPDATE SET expires_at = excluded.expires_at",
            )
            .bind(theme_id)
            .bind(session_id)
            .bind(expires_at)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await?;
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO theme_previews (theme_id, session_id, expires_at) VALUES ($1, $2, $3)
                 ON CONFLICT(theme_id, session_id) DO UPDATE SET expires_at = excluded.expires_at",
            )
            .bind(theme_id)
            .bind(session_id)
            .bind(expires_at)
            .execute(db.postgres_pool().expect("postgres"))
            .await?;
        }
    }
    Ok(())
}

/// The live preview for a session, if any.
///
/// `now` is passed in rather than read here so a test can stand at any instant; the comparison is the
/// domain's `preview_is_live`, so the rule has one home.
pub async fn preview_for(db: &Database, session_id: &str, now_epoch: i64) -> Result<Option<Stored>> {
    let sql = "SELECT p.id, p.scope, p.owner, p.name, p.state, p.palette, p.layout, p.updated_at, v.expires_at
               FROM theme_previews v JOIN theme_profiles p ON p.id = v.theme_id
               WHERE v.session_id = ? ORDER BY v.expires_at DESC";
    let sql_pg = "SELECT p.id, p.scope, p.owner, p.name, p.state, p.palette, p.layout, p.updated_at, v.expires_at
               FROM theme_previews v JOIN theme_profiles p ON p.id = v.theme_id
               WHERE v.session_id = $1 ORDER BY v.expires_at DESC";

    let found: Option<(Stored, String)> = match db.backend() {
        Backend::Sqlite => {
            let row = sqlx::query(sql)
                .bind(session_id)
                .fetch_optional(db.sqlite_pool().expect("sqlite"))
                .await?;
            row.as_ref().map(|r| {
                let mut stored = row_to_stored(r);
                stored.theme.name = r.get::<String, _>("name");
                let expires = r.get::<String, _>("expires_at");
                (stored, expires)
            })
        }
        Backend::Postgres => {
            let row = sqlx::query(sql_pg)
                .bind(session_id)
                .fetch_optional(db.postgres_pool().expect("postgres"))
                .await?;
            row.as_ref().map(|r| {
                let mut stored = row_to_stored_pg(r);
                stored.theme.name = r.get::<String, _>("name");
                let expires = r.get::<String, _>("expires_at");
                (stored, expires)
            })
        }
    };

    let Some((stored, expires_at)) = found else {
        return Ok(None);
    };
    let Some(expires_epoch) = epoch_seconds(&expires_at) else {
        // An unparseable expiry is not a live preview: the safe reading of "unknown" is "not previewing".
        return Ok(None);
    };
    Ok(darkhold_domain::theme_model::preview_is_live(expires_epoch, now_epoch).then_some(stored))
}

/// The instance's safe-mode state.
pub async fn safe_mode(db: &Database) -> Result<SafeMode> {
    let row: Option<(i64, Option<String>)> = match db.backend() {
        Backend::Sqlite => sqlx::query_as(
            "SELECT active, reason FROM theme_safe_mode WHERE instance_key = ?",
        )
        .bind(SAFE_MODE_KEY)
        .fetch_optional(db.sqlite_pool().expect("sqlite"))
        .await?,
        Backend::Postgres => sqlx::query_as(
            "SELECT CAST(active AS BIGINT) AS active, reason FROM theme_safe_mode WHERE instance_key = $1",
        )
        .bind(SAFE_MODE_KEY)
        .fetch_optional(db.postgres_pool().expect("postgres"))
        .await?,
    };
    Ok(match row {
        Some((active, reason)) if active != 0 => SafeMode {
            active: true,
            reason: reason.or_else(|| Some("safe mode is on".to_owned())),
        },
        _ => SafeMode::off(),
    })
}

/// Set the instance's safe mode.
///
/// Entering requires a reason (the domain type refuses an empty one); leaving is allowed to carry none.
pub async fn set_safe_mode(db: &Database, mode: &SafeMode, now: &str) -> Result<()> {
    let active = i64::from(mode.active);
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(
                "INSERT INTO theme_safe_mode (instance_key, active, reason, updated_at) VALUES (?, ?, ?, ?)
                 ON CONFLICT(instance_key) DO UPDATE SET active = excluded.active, reason = excluded.reason, updated_at = excluded.updated_at",
            )
            .bind(SAFE_MODE_KEY)
            .bind(active)
            .bind(mode.reason.as_deref())
            .bind(now)
            .execute(db.sqlite_pool().expect("sqlite"))
            .await?;
        }
        Backend::Postgres => {
            sqlx::query(
                "INSERT INTO theme_safe_mode (instance_key, active, reason, updated_at) VALUES ($1, $2, $3, $4)
                 ON CONFLICT(instance_key) DO UPDATE SET active = excluded.active, reason = excluded.reason, updated_at = excluded.updated_at",
            )
            .bind(SAFE_MODE_KEY)
            .bind(active)
            .bind(mode.reason.as_deref())
            .bind(now)
            .execute(db.postgres_pool().expect("postgres"))
            .await?;
        }
    }
    Ok(())
}

/// Parse an RFC 3339 stamp into Unix seconds.
fn epoch_seconds(stamp: &str) -> Option<i64> {
    use time::format_description::well_known::Rfc3339;
    time::OffsetDateTime::parse(stamp, &Rfc3339)
        .ok()
        .map(|t| t.unix_timestamp())
}

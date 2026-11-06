//! Experiment bucket assignment (M64-02).
//!
//! A bucket is **assigned once and then read**. That is the whole difference between an experiment and a random number:
//! if the bucket were recomputed per request, a reader would see both arms inside one session; if it were derived from
//! the current algorithm list, adding one algorithm would move every reader at once and invalidate the measurement the
//! assignment exists for.
//!
//! # What is stored
//!
//! `(pseud, surface) -> bucket`, unique per pair, written on first use. `surface` is the registry's canonical dotted id
//! (`discover.feed`), not a route path: a route rename must not split one experiment into two cohorts.
//!
//! # What is not
//!
//! The impressions. Those already live in `recommendation_slots`, one append-only row per `(surface, algorithm,
//! position, item)` with `chosen_at` recording the event that followed. Joining the two on `(pseud, surface)` is what
//! turns a served slot into a per-arm observation; copying the bucket onto every slot would be a second source of truth
//! for the same fact, free to drift.

use anyhow::Result;

use crate::sql_owned;
use crate::Backend;
use crate::Database;

/// The bucket this reader is in for this surface, creating the assignment on first use.
///
/// `buckets` is the arm count **at assignment time**. A later change to it does not move an existing reader — that is the
/// point — so the caller must handle a stored bucket that is no longer in range, and the honest handling is to treat it
/// as the control arm rather than to remap it silently.
pub async fn bucket_for(
    db: &Database,
    pseud_id: &str,
    surface_id: &str,
    buckets: i64,
) -> Result<i64> {
    if buckets <= 0 {
        return Ok(0);
    }
    let read = sql_owned(
        db,
        "SELECT bucket FROM experiment_assignments WHERE pseud_id = ?1 AND surface_id = ?2".into(),
        "SELECT bucket FROM experiment_assignments WHERE pseud_id = $1::uuid AND surface_id = $2"
            .into(),
    );

    let existing = read_bucket(db, &read, pseud_id, surface_id).await?;
    if let Some(bucket) = existing {
        return Ok(bucket);
    }

    // Deterministic in (pseud, surface) so two concurrent first requests agree, and so the loser of an insert race
    // computes the same value the winner stored. FNV-1a rather than `DefaultHasher`: the latter's algorithm is
    // explicitly unspecified across Rust versions, and while the value is persisted once, a test that pins fairness
    // should not change meaning on a toolchain upgrade.
    let bucket = (fnv1a(pseud_id, surface_id) % buckets.unsigned_abs()) as i64;

    let insert = sql_owned(
        db,
        "INSERT INTO experiment_assignments (id, pseud_id, surface_id, bucket, assigned_at, version) \
         VALUES (?1, ?2, ?3, ?4, datetime('now'), 1) ON CONFLICT (pseud_id, surface_id) DO NOTHING"
            .into(),
        "INSERT INTO experiment_assignments (id, pseud_id, surface_id, bucket, assigned_at, version) \
         VALUES ($1, $2::uuid, $3, $4, now(), 1) ON CONFLICT (pseud_id, surface_id) DO NOTHING"
            .into(),
    );
    let id = uuid::Uuid::new_v4().to_string();
    match db.backend() {
        Backend::Sqlite => {
            sqlx::query(&insert)
                .bind(&id)
                .bind(pseud_id)
                .bind(surface_id)
                .bind(bucket)
                .execute(db.sqlite_pool().expect("sqlite pool for sqlite backend"))
                .await?;
        }
        Backend::Postgres => {
            sqlx::query(&insert)
                .bind(&id)
                .bind(pseud_id)
                .bind(surface_id)
                .bind(bucket)
                .execute(
                    db.postgres_pool()
                        .expect("postgres pool for postgres backend"),
                )
                .await?;
        }
    }

    // Read back rather than returning the computed value: if another request won the race, its bucket is the assignment
    // and this one must agree with it, not with what it computed.
    Ok(read_bucket(db, &read, pseud_id, surface_id)
        .await?
        .unwrap_or(bucket))
}

async fn read_bucket(
    db: &Database,
    read: &str,
    pseud_id: &str,
    surface_id: &str,
) -> Result<Option<i64>> {
    // Each arm builds its own query: sqlx's `Executor` is generic over the database, so one query value cannot be
    // executed against both pools.
    Ok(match db.backend() {
        Backend::Sqlite => {
            sqlx::query_scalar::<_, i64>(read)
                .bind(pseud_id)
                .bind(surface_id)
                .fetch_optional(db.sqlite_pool().expect("sqlite pool for sqlite backend"))
                .await?
        }
        Backend::Postgres => {
            sqlx::query_scalar::<_, i64>(read)
                .bind(pseud_id)
                .bind(surface_id)
                .fetch_optional(
                    db.postgres_pool()
                        .expect("postgres pool for postgres backend"),
                )
                .await?
        }
    })
}

fn fnv1a(a: &str, b: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in a.as_bytes().iter().chain(b"\x1f").chain(b.as_bytes()) {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::fnv1a;

    #[test]
    fn the_hash_is_stable_for_a_pair() {
        assert_eq!(
            fnv1a("pseud-a", "discover.feed"),
            fnv1a("pseud-a", "discover.feed")
        );
        assert_ne!(
            fnv1a("pseud-a", "discover.feed"),
            fnv1a("pseud-b", "discover.feed")
        );
        assert_ne!(
            fnv1a("pseud-a", "discover.feed"),
            fnv1a("pseud-a", "home.trending")
        );
    }

    #[test]
    fn the_separator_stops_pairs_colliding_by_concatenation() {
        // Without the 0x1f separator, "ab"+"c" and "a"+"bc" would hash the same bytes and land in one arm.
        assert_ne!(fnv1a("ab", "c"), fnv1a("a", "bc"));
    }
}

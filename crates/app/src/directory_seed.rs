//! The directory's built-in categories, seeded at boot (§39.2, §45.1).
//!
//! `category_governance::seed_categories` and `upsert_category` had no caller, so a fresh instance had an **empty**
//! category list: the directory's category filter offered nothing, and `create_proposal` answered "category not found"
//! for every §39.2 slug a reader would try. Nothing was broken enough to log — a list with no rows is a valid list.
//!
//! The seed is idempotent by construction rather than by guard: `upsert_category` is `ON CONFLICT(slug) DO NOTHING`, which
//! is §45.1's rule for a config upsert — it adds slugs the instance is missing and never resurrects a category the
//! community removed. A boot therefore costs eight no-op inserts after the first one.
//!
//! **A failed seed does not fail the boot.** An instance with no categories is visibly misconfigured, while an instance
//! that refuses to start is an outage; the log line is the difference between the two.

use darkhold_db::category_governance;
use darkhold_db::identity::now_rfc3339;

use crate::state::AppState;

/// Seed the §39.2 slugs and report how many categories are active afterwards.
///
/// The return value is the count rather than `()`, because the caller logs it: "seeded eight" and "seeded none, thirty-two
/// were already there" are the same code path and different facts.
pub async fn seed_directory_categories(state: &AppState) -> anyhow::Result<i64> {
    let now = now_rfc3339();
    category_governance::seed_categories(state.db(), &now).await?;
    category_governance::count_active_categories(state.db()).await
}

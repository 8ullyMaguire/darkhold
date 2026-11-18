//! M64-09: apply taste gravity to the short reader rails that already have candidate pipelines.
//!
//! The feed has its own, richer ordering path. These surfaces only have an ordered list of `SurfaceWork`, so the layer here
//! converts position into a base score, asks the shared mixture for a new order, and puts the same structs back. It never changes a
//! work's own `similarity` and never drops a work.

use std::collections::HashMap;

use darkhold_db::reader_surface::SurfaceWork;
use darkhold_domain::discovery::{apply_taste_gravity, Candidate};
use darkhold_domain::{AccountId, PseudId};

use crate::state::AppState;

/// Reorder `works` by the same bounded mixture the feed uses. `signals` is keyed by work id.
#[must_use]
pub fn reorder(
    works: Vec<SurfaceWork>,
    signals: HashMap<String, f64>,
    strength: f64,
    mode: &str,
) -> Vec<SurfaceWork> {
    if works.len() < 2 {
        return works;
    }
    let original_order: Vec<String> = works.iter().map(|work| work.id.clone()).collect();
    let total = works.len();
    let candidates: Vec<Candidate> = works
        .iter()
        .enumerate()
        .filter_map(|(index, work)| {
            let work_id = work.id.parse().ok()?;
            Some(Candidate {
                work_id,
                score: (total - index) as i64,
                reason: "surface".to_owned(),
                taste_signal: signals.get(&work.id).copied().unwrap_or(0.0),
                diversity_class: 0.5,
            })
        })
        .collect();
    if candidates.is_empty() {
        return works;
    }
    let (ranked, _) = apply_taste_gravity(candidates, strength, mode);
    let mut by_id: HashMap<String, SurfaceWork> = works
        .into_iter()
        .map(|work| (work.id.clone(), work))
        .collect();
    let mut out = Vec::with_capacity(total);
    for candidate in ranked {
        let id = candidate.work_id.to_string();
        if let Some(work) = by_id.remove(&id) {
            out.push(work);
        }
    }
    // An id that could not be parsed was never a candidate. It keeps its place after the ranked ones rather than disappearing.
    for id in original_order {
        if let Some(work) = by_id.remove(&id) {
            out.push(work);
        }
    }
    out
}

/// Fetch the surface's knobs and the reader's own signal/dial, then reorder. A failed read leaves the order alone.
pub async fn apply(
    state: &AppState,
    surface: &str,
    works: Vec<SurfaceWork>,
    account: Option<AccountId>,
    pseud: Option<PseudId>,
) -> Vec<SurfaceWork> {
    if works.len() < 2 {
        return works;
    }
    let knobs = crate::taste_knobs::effective(state, surface).await;
    let reader_strength = match pseud.as_ref() {
        Some(pseud) => darkhold_db::gravity_prefs::read(state.db(), pseud.as_uuid())
            .await
            .ok()
            .flatten(),
        None => None,
    };
    let strength = polaris_gravity::effective(knobs.strength, reader_strength);
    let signals = match account {
        Some(account) => {
            let ids: Vec<uuid::Uuid> = works
                .iter()
                .filter_map(|work| work.id.parse::<darkhold_domain::WorkId>().ok())
                .map(|work| work.as_uuid())
                .collect();
            match darkhold_db::taste_signal::for_account(state.db(), &account.to_string(), &ids)
                .await
            {
                Ok(Some(found)) => found
                    .into_iter()
                    .map(|(id, signal)| (id.to_string(), signal))
                    .collect(),
                Ok(None) => HashMap::new(),
                Err(error) => {
                    tracing::warn!(%error, "taste signal unavailable; serving the rail unpulled");
                    HashMap::new()
                }
            }
        }
        None => HashMap::new(),
    };
    reorder(works, signals, strength, &knobs.signal_weight_mode)
}

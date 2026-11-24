//! The daily automatic WIP/abandoned sweep (M45-76).
//!
//! The pure decision lives in the domain crate, the clocks and transitions in
//! the store crate. This module is the only place that walks a page of works.

use darkhold_domain::work_lifecycle::{
    decide, parse_activity_timestamp, CompletionSource, LifecycleAction, Reading,
};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

#[derive(Debug, serde::Serialize)]
pub struct SweepReport {
    pub scanned: i64,
    pub abandoned: i64,
    pub revived: i64,
    pub concluded: i64,
    pub raced: i64,
    pub unmeasured: i64,
    pub next_cursor: Option<String>,
    pub done: bool,
    pub dry_run: bool,
}

pub async fn sweep_page(
    state: &crate::AppState,
    cursor: Option<String>,
    limit: i64,
    dry_run: bool,
) -> Result<SweepReport, darkhold_domain::AppError> {
    let limit = limit.clamp(1, 500);
    let candidates = darkhold_db::work_lifecycle::candidates(state.db(), cursor.as_deref(), limit)
        .await
        .map_err(|e| darkhold_domain::AppError::Internal(e.into()))?;
    let now = OffsetDateTime::now_utc();
    let now_text = now.format(&Rfc3339).expect("UTC formats as RFC 3339");
    let months = state.config().works.auto_abandon_after_months;

    let mut report = SweepReport {
        scanned: candidates.len() as i64,
        abandoned: 0,
        revived: 0,
        concluded: 0,
        raced: 0,
        unmeasured: 0,
        next_cursor: None,
        done: candidates.len() < usize::try_from(limit).unwrap_or(0),
        dry_run,
    };

    for candidate in &candidates {
        let last = [
            &candidate.last_event,
            &candidate.last_source,
            &candidate.published_at,
        ]
        .into_iter()
        .flatten()
        .filter_map(|raw| parse_activity_timestamp(raw))
        .max();
        let marked = candidate
            .completion_marked_at
            .as_deref()
            .and_then(parse_activity_timestamp);
        let action = decide(
            Reading {
                completion: &candidate.completion,
                source_says_complete: candidate.source_says_complete,
                completion_marked_at: marked,
                last_activity: last,
            },
            now,
            months,
        );
        let observed = last.map(|value| value.format(&Rfc3339).expect("RFC 3339"));

        let (to, source) = match action {
            LifecycleAction::None => continue,
            LifecycleAction::Unmeasured => {
                report.unmeasured += 1;
                continue;
            }
            LifecycleAction::Abandon => ("abandoned", CompletionSource::Sweep),
            LifecycleAction::Revive => ("in_progress", CompletionSource::Update),
            LifecycleAction::Conclude => ("complete", CompletionSource::Update),
        };

        if dry_run {
            match action {
                LifecycleAction::Abandon => report.abandoned += 1,
                LifecycleAction::Revive => report.revived += 1,
                LifecycleAction::Conclude => report.concluded += 1,
                LifecycleAction::None | LifecycleAction::Unmeasured => {}
            }
            continue;
        }

        let changed = darkhold_db::work_lifecycle::apply_transition(
            state.db(),
            &candidate.work_id,
            &candidate.completion,
            to,
            source,
            None,
            None,
            observed.as_deref(),
            &now_text,
        )
        .await
        .map_err(|e| darkhold_domain::AppError::Internal(e.into()))?;
        if changed {
            match action {
                LifecycleAction::Abandon => report.abandoned += 1,
                LifecycleAction::Revive => report.revived += 1,
                LifecycleAction::Conclude => report.concluded += 1,
                LifecycleAction::None | LifecycleAction::Unmeasured => {}
            }
        } else {
            report.raced += 1;
        }
    }

    if !report.done {
        report.next_cursor = candidates.last().map(|candidate| candidate.work_id.clone());
    }
    Ok(report)
}

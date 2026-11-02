//! M45-38 — checking a note link, through the same door as every other fetch (§32.7, gaps review D3).
//!
//! A note link is an outbound request the instance makes on an author's behalf, so it goes through
//! [`crate::media_fetch`] exactly like a media mirror does: the same scheme-and-host refusal, the same
//! DNS resolution with every answer checked, and the same "redirects are not followed" client. A
//! second fetch path would be a second place to forget that `127.0.0.1` is not a public host.
//!
//! ## What a check means
//!
//! - **One failure is not rot.** A link that fails once is [`Degraded`](LinkStatus::Degraded); the
//!   third consecutive failure is [`Dead`](LinkStatus::Dead). Sites have bad minutes, and a monitor
//!   that declares a link dead on one timeout is a monitor authors learn to ignore.
//! - **A success resets the count**, which is the whole point of counting.
//! - **The body is not read.** A link is monitored for answering, not for content, so the response is
//!   classified from its status line and headers and then dropped — no bytes are pulled over the wire
//!   for a 400-page PDF in someone's author note.
//! - **A check never fails the caller.** The result is recorded and returned; a URL that times out is
//!   the answer, not an error. Only a storage failure is an error, because that is the instance's
//!   fault rather than the link's.

use std::net::IpAddr;
use std::time::Duration;

use darkhold_db::note_links;
use darkhold_domain::media_resilience::LinkStatus;

use crate::media_fetch::plan_fetch_allowing;
use crate::state::AppState;
use crate::worker::HandlerError;

/// How long a note link may take to answer.
///
/// Shorter than a media fetch: nothing is being downloaded, so a link that cannot manage a status line
/// in this window is a link a reader would also wait on.
pub const NOTE_LINK_TIMEOUT: Duration = Duration::from_secs(8);

/// Consecutive failures that make a note link dead. The same number the availability pipeline uses,
/// reused rather than re-chosen: two thresholds for "dead" would be two definitions.
pub const DEAD_AFTER: i64 = 3;

/// Check one stored note link and record what happened.
///
/// `trusted_address` exists only so a test can point this at a loopback server, and is passed to the
/// guard rather than around it — see `handle_media_fetch` for why that distinction matters.
///
/// Returns the status that was recorded.
pub async fn check_note_link(
    state: &AppState,
    link_id: &str,
    trusted_address: Option<IpAddr>,
) -> Result<LinkStatus, HandlerError> {
    let link = note_links::find_by_id(state.db(), link_id)
        .await
        .map_err(|error| HandlerError::Transient(format!("reading the note link: {error}")))?
        .ok_or_else(|| HandlerError::Fatal(format!("note link {link_id} not found")))?;

    let failures = link.consecutive_failures + 1;

    // Refuse a URL that should never have been stored, and a URL that now resolves somewhere private.
    let parsed = match reqwest::Url::parse(&link.url) {
        Ok(url) => url,
        Err(error) => {
            // A URL this build cannot parse cannot be checked, and it will not become checkable. That
            // is a dead link as far as a reader is concerned, and saying so is more useful than
            // leaving it "pending" forever.
            let reason = format!("the url cannot be parsed: {error}");
            note_links::mark_checked(state.db(), link_id, LinkStatus::Dead, failures, Some(&reason))
                .await
                .map_err(|error| HandlerError::Transient(format!("recording the check: {error}")))?;
            return Ok(LinkStatus::Dead);
        }
    };
    let allow: &[IpAddr] = trusted_address.as_slice();
    let plan = match plan_fetch_allowing(&parsed, allow, NOTE_LINK_TIMEOUT) {
        Ok(plan) => plan,
        Err(reason) => {
            // A refusal here is permanent for this URL (a scheme, a host, a port this instance will
            // not talk to), so it is recorded as dead immediately rather than escalating over three
            // checks. Retrying a decision does not change it.
            note_links::mark_checked(state.db(), link_id, LinkStatus::Dead, failures, Some(&reason))
                .await
                .map_err(|error| HandlerError::Transient(format!("recording the check: {error}")))?;
            return Ok(LinkStatus::Dead);
        }
    };

    if allow.is_empty() {
        let resolved = darkhold_scrapers::safety::resolve_public(&plan.host, NOTE_LINK_TIMEOUT).await;
        match resolved {
            Ok(addrs) => {
                if let Some(bad) = addrs
                    .iter()
                    .find(|addr| darkhold_scrapers::safety::is_forbidden_ip(addr.ip()))
                {
                    let reason = format!("{} resolves to a non-public address ({})", plan.host, bad.ip());
                    note_links::mark_checked(
                        state.db(),
                        link_id,
                        LinkStatus::Dead,
                        failures,
                        Some(&reason),
                    )
                    .await
                    .map_err(|error| HandlerError::Transient(format!("recording the check: {error}")))?;
                    return Ok(LinkStatus::Dead);
                }
            }
            Err(error) => {
                let reason = format!("{} does not resolve: {error}", plan.host);
                return settle(state, link_id, failures, Some(&reason)).await;
            }
        }
    }

    let response = state
        .reqwest_client()
        .get(plan.url.clone())
        .timeout(NOTE_LINK_TIMEOUT)
        .send()
        .await;
    let response = match response {
        Ok(response) => response,
        Err(error) => {
            let reason = format!("fetching {} failed: {error}", plan.host);
            return settle(state, link_id, failures, Some(&reason)).await;
        }
    };

    let status = response.status().as_u16();

    // Dropped before the body is read: a link that answers is the finding, and the bytes are not.
    // This is also why `media_fetch::classify_with_length` is *not* used here — it classifies media,
    // and HTML is a refusal to it. A note link to a Tumblr post is HTML and perfectly healthy.
    drop(response);

    match verdict(status) {
        Verdict::Answers => {
            note_links::mark_checked(state.db(), link_id, LinkStatus::Healthy, 0, None)
                .await
                .map_err(|error| HandlerError::Transient(format!("recording the check: {error}")))?;
            Ok(LinkStatus::Healthy)
        }
        Verdict::MayRecover => {
            // A bot wall, a rate limit, a 5xx: the link exists and may answer next time, so the
            // three-strike rule applies rather than an immediate verdict.
            settle(state, link_id, failures, Some(&format!("http {status}"))).await
        }
        Verdict::Gone => {
            // 404 and 410 are the site saying the thing is not there. Waiting for three of those
            // would leave a dead link reported as "degraded" for three sweeps, which is time the
            // reader spends clicking it.
            note_links::mark_checked(
                state.db(),
                link_id,
                LinkStatus::Dead,
                failures,
                Some(&format!("http {status}")),
            )
            .await
            .map_err(|error| HandlerError::Transient(format!("recording the check: {error}")))?;
            Ok(LinkStatus::Dead)
        }
    }
}

/// What an HTTP status says about whether a link is worth clicking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    /// It answered: 2xx, or a 3xx the client did not follow (redirects are disabled, so a 3xx is the
    /// final answer this instance will see and the site still recognises the URL).
    Answers,
    /// It did not answer, in a way that may change.
    MayRecover,
    /// It answered that the thing is gone, and it will keep saying so.
    Gone,
}

fn verdict(status: u16) -> Verdict {
    match status {
        200..=399 => Verdict::Answers,
        404 | 410 => Verdict::Gone,
        _ => Verdict::MayRecover,
    }
}

/// Record a failure, escalating to dead at [`DEAD_AFTER`] consecutive ones.
async fn settle(
    state: &AppState,
    link_id: &str,
    failures: i64,
    reason: Option<&str>,
) -> Result<LinkStatus, HandlerError> {
    let status = if failures >= DEAD_AFTER {
        LinkStatus::Dead
    } else {
        LinkStatus::Degraded
    };
    note_links::mark_checked(state.db(), link_id, status, failures, reason)
        .await
        .map_err(|error| HandlerError::Transient(format!("recording the check: {error}")))?;
    Ok(status)
}

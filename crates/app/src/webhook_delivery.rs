//! Webhook delivery orchestration: fetch matching endpoints, send, record.
//!
//! Bridges the outbox (`publish.notify` topic) to the webhook sender
//! (`crate::webhook_sender`). The worker calls `deliver_notification` for each
//! outbox event; this module finds every active webhook subscribed to the
//! corresponding event type, signs and POSTs the payload, and records each
//! attempt in `webhook_deliveries`.

use anyhow::{Context, Result};
use serde_json::Value;
use tracing::{info, warn};

use darkhold_db::outbox::OutboxEvent;
use darkhold_domain::webhook::WebhookEvent;

use crate::state::AppState;
use crate::webhook_sender::{self, DeliveryStatus};

/// Deliver a notification outbox event to all matching webhook endpoints.
///
/// Looks up the event type from the outbox payload's `event_type` field (or
/// derives it from the topic), finds active webhooks subscribed to that event,
/// and sends the payload to each. Deliveries are recorded regardless of
/// success/failure; a single failing endpoint does not block the others.
pub async fn deliver_notification(state: &AppState, event: &OutboxEvent) -> Result<()> {
    let payload: Value =
        serde_json::from_str(&event.payload).context("outbox event payload is not valid JSON")?;

    // Derive the event type. The content service sets `event_type` in the
    // payload for `publish.notify`; fall back to the topic minus the prefix.
    let event_type = payload
        .get("event_type")
        .and_then(|v| v.as_str())
        .map(str::to_owned)
        .or_else(|| derive_event_type(&event.topic))
        .unwrap_or_else(|| event.topic.clone());

    // Fetch all active webhooks and filter by subscribed event type.
    let all_webhooks = darkhold_db::marketplace::list_all_active_webhooks(state.db())
        .await
        .context("listing active webhooks")?;

    let matching: Vec<&Value> = all_webhooks
        .iter()
        .filter(|wh| webhook_subscribed(wh, &event_type))
        .collect();

    if matching.is_empty() {
        tracing::debug!(event_type = %event_type, "no webhooks subscribed to event");
        return Ok(());
    }

    info!(
        event_type = %event_type,
        endpoints = matching.len(),
        "delivering webhook event"
    );

    let client = &state.reqwest_client();
    let config = state.webhook_config();

    for wh in matching {
        let endpoint_id = wh["id"].as_str().unwrap_or("").to_owned();
        let endpoint_url = wh["url"].as_str().unwrap_or("").to_owned();
        let secret = wh["secret"].as_str().unwrap_or("").to_owned();

        // ---- M16-14: the three operational gates, checked in the order that matters.
        //
        // 1. The rate limit is checked before anything else because a throttled delivery must not cost the
        //    receiver anything and must not reset the breaker: a burst is not a failure of the endpoint.
        let limit = darkhold_domain::webhook::RateLimit {
            per_minute: u32::try_from(wh["rate_per_minute"].as_i64().unwrap_or(60))
                .unwrap_or(u32::MAX),
        };
        let now = time::OffsetDateTime::now_utc();
        let since = (now - time::Duration::minutes(1))
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();
        let recent = darkhold_db::webhooks::delivered_since(state.db(), &endpoint_id, &since)
            .await
            .unwrap_or(0);
        if !limit.allows(u64::try_from(recent).unwrap_or(u64::MAX)) {
            // Recorded rather than dropped: a delivery that was never attempted is exactly the kind of
            // thing whose absence looks like an outage from the subscriber's side.
            if let Err(err) = darkhold_db::marketplace::record_delivery(
                state.db(),
                &endpoint_id,
                &event.id,
                &event.payload,
                "",
                "throttled",
            )
            .await
            {
                warn!(%err, "recording throttled webhook failed");
            }
            info!(
                endpoint_id = %endpoint_id,
                per_minute = limit.per_minute,
                recent,
                "webhook delivery throttled"
            );
            continue;
        }

        // 2. Sign with every admissible key. During a rotation's grace window that is two keys, so a
        //    subscriber that has not deployed the new secret yet keeps verifying instead of failing
        //    twenty times and switching itself off.
        let rotated_at_epoch = wh["rotated_at"].as_str().and_then(epoch_seconds);
        let previous = wh["previous_secret"].as_str();
        let keys: Vec<(darkhold_domain::webhook::KeyId, String)> =
            darkhold_domain::webhook::admissible_keys(
                &secret,
                previous,
                rotated_at_epoch,
                now.unix_timestamp(),
            )
            .into_iter()
            .map(|(id, s)| (id, s.to_owned()))
            .collect();

        let webhook_event = WebhookEvent {
            event_type: event_type.clone(),
            event_id: event.id.clone(),
            created_at: payload
                .get("created_at")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned(),
            payload: payload.clone(),
        };

        let attempt = 1u32; // First attempt; retries handled by outbox
        match webhook_sender::send_with_keys(
            client,
            config,
            &endpoint_url,
            &keys,
            &webhook_event,
            &endpoint_id,
            attempt,
        )
        .await
        {
            Ok((record, status_code)) => {
                let status_str = match record.status {
                    DeliveryStatus::Ok => "ok",
                    DeliveryStatus::Retrying => "retrying",
                    DeliveryStatus::Failed => "failed",
                };
                if let Err(err) = darkhold_db::marketplace::record_delivery(
                    state.db(),
                    &record.endpoint_id,
                    &record.event_id,
                    &record.payload.to_string(),
                    &record.signature,
                    status_str,
                )
                .await
                {
                    warn!(%err, "recording webhook delivery failed");
                }
                // 3. The breaker: a 2xx is the only thing that resets the consecutive count.
                if record.status == DeliveryStatus::Ok {
                    if let Err(err) =
                        darkhold_db::webhooks::record_success(state.db(), &endpoint_id).await
                    {
                        warn!(%err, "resetting webhook failure count failed");
                    }
                } else if let Ok(Some(reason)) = darkhold_db::webhooks::record_failure(
                    state.db(),
                    &endpoint_id,
                    darkhold_domain::webhook::DEFAULT_FAILURE_THRESHOLD,
                    &now.format(&time::format_description::well_known::Rfc3339)
                        .unwrap_or_default(),
                )
                .await
                {
                    warn!(endpoint_id = %endpoint_id, reason = %reason, "webhook endpoint switched off");
                }
                info!(
                    endpoint_id = %endpoint_id,
                    status_code,
                    status = status_str,
                    "webhook delivered"
                );
            }
            Err(err) => {
                warn!(
                    endpoint_id = %endpoint_id,
                    %err,
                    "webhook delivery failed"
                );
                // Record the failure so it shows in the delivery log.
                if let Err(rec_err) = darkhold_db::marketplace::record_delivery(
                    state.db(),
                    &endpoint_id,
                    &event.id,
                    &event.payload,
                    "",
                    "failed",
                )
                .await
                {
                    warn!(%rec_err, "recording webhook failure failed");
                }
                if let Ok(Some(reason)) = darkhold_db::webhooks::record_failure(
                    state.db(),
                    &endpoint_id,
                    darkhold_domain::webhook::DEFAULT_FAILURE_THRESHOLD,
                    &now.format(&time::format_description::well_known::Rfc3339)
                        .unwrap_or_default(),
                )
                .await
                {
                    warn!(endpoint_id = %endpoint_id, reason = %reason, "webhook endpoint switched off");
                }
            }
        }
    }

    Ok(())
}

/// Parse an RFC 3339 timestamp into Unix seconds.
///
/// Kept here rather than in the store because the store must not decide what "recent" means: it stores
/// what happened and this worker compares it against the clock.
fn epoch_seconds(stamp: &str) -> Option<i64> {
    time::OffsetDateTime::parse(stamp, &time::format_description::well_known::Rfc3339)
        .ok()
        .map(|t| t.unix_timestamp())
}

/// Check if a webhook is subscribed to a given event type.
fn webhook_subscribed(webhook: &Value, event_type: &str) -> bool {
    let events_str = webhook["events"].as_str().unwrap_or("[]");
    let events: Vec<String> = serde_json::from_str(events_str).unwrap_or_default();
    events.iter().any(|e| e == event_type || e == "*")
}

/// Derive a human-readable event type from an outbox topic.
///
/// `publish.notify` → `work.published`; `comment.posted` stays as-is.
fn derive_event_type(topic: &str) -> Option<String> {
    match topic {
        "publish.notify" => Some("work.published".to_owned()),
        "withdraw.notify" => Some("work.withdrawn".to_owned()),
        "unit.revised" => Some("unit.revised".to_owned()),
        _ => Some(topic.to_owned()),
    }
}

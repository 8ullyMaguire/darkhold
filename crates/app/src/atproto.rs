//! Outbound ATProto: one `app.bsky.feed.post` per published work, pointing at the canonical page.
//!
//! Two rules from `docs/FEDERATION.md` shape this file. The body is **not copied** — the record carries a link card,
//! because the archive is the site and a second copy would be a second thing to keep true. And the record key is derived
//! from the work id, so a **replayed outbox event rewrites the same record** instead of minting a second one; that is what
//! makes replaying forty thousand undelivered `work.published` rows safe.
//!
//! The worker defers rather than delivers while the bridge is unconfigured: see the handler in `server.rs`. Delivering
//! with `enabled = false` would mark the backlog done and lose the events the day the bridge is switched on.

use anyhow::{Context, Result};
use serde_json::json;

use crate::AppState;

/// The record key. Deterministic in the work id, hex, 24 characters — an rkey may be any `[A-Za-z0-9.-]{1,512}`, and a
/// hash rather than the raw uuid keeps it inside the collection's own namespace without a TID clock.
#[must_use]
pub fn rkey_for(work_id: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(work_id.as_bytes());
    digest[..12].iter().map(|b| format!("{b:02x}")).collect()
}

/// The canonical page. `/w/<uuid>` is the reader route the export bundle already treats as canonical.
#[must_use]
pub fn canonical_url(base: &str, work_id: &str) -> String {
    format!("{}/w/{}", base.trim_end_matches('/'), work_id)
}

/// The post body: title, a link facet over the whole text, and an external card pointing at the page.
#[must_use]
pub fn record_for(work: &darkhold_db::atproto::WorkForRecord, site_base: &str, created_at: &str) -> serde_json::Value {
    let url = canonical_url(site_base, &work.id);
    let text = format!("{} — darkhold", work.title);
    json!({
        "$type": "app.bsky.feed.post",
        "text": text,
        "createdAt": created_at,
        "langs": [work.language],
        "facets": [{
            "index": { "byteStart": 0, "byteEnd": text.len() },
            "features": [{ "$type": "app.bsky.richtext.facet#link", "uri": url }]
        }],
        "embed": {
            "$type": "app.bsky.embed.external",
            "external": {
                "uri": url,
                "title": work.title,
                "description": first_sentence(&work.summary, 200)
            }
        }
    })
}

/// Bluesky truncates the card description; cutting here keeps the JSON the estate's own rather than the PDS's.
#[must_use]
fn first_sentence(text: &str, max: usize) -> String {
    let cut = text
        .char_indices()
        .take_while(|(i, _)| *i < max)
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(0);
    text[..cut.min(text.len())].trim_end().to_string()
}

/// Deliver one `work.published` event.
///
/// `Ok(())` means "this event is done": either a record was written, or the work is not public any more and there was
/// nothing to publish. An error means "try again later" and leaves the row claimable with backoff.
pub async fn publish_work(state: &AppState, payload: &str) -> Result<()> {
    let event: serde_json::Value = serde_json::from_str(payload).context("work.published payload is not JSON")?;
    let work_id = event["work_id"].as_str().context("work.published payload has no work_id")?;

    // Defer, never deliver. Reaching here with the bridge off can only happen if `enabled` was turned off under a
    // running worker; returning `Ok` would mark the event done and silently drop a publication nobody got.
    let Some(config) = state.config().atproto.as_ref().filter(|c| c.enabled) else {
        anyhow::bail!("atproto is not configured; deferring work.published");
    };
    let Some(work) = darkhold_db::atproto::work_for_record(state.db(), work_id).await? else {
        // Withdrawn, private or deleted between the event and the delivery. Nothing to publish, and nothing to retry.
        tracing::debug!(work = %work_id, "work.published: the work is not public any more; nothing to publish");
        return Ok(());
    };
    let password = std::env::var(&config.password_env)
        .with_context(|| format!("{} is not set in this unit's environment", config.password_env))?;

    let http = state.reqwest_client();
    let session = create_session(http, config, &password).await?;
    let record = record_for(&work, &state.config().site.base_url, &work.created_at);
    let rkey = rkey_for(&work.id);

    let response = http
        .post(format!("{}/xrpc/com.atproto.repo.putRecord", config.pds.trim_end_matches('/')))
        .bearer_auth(&session.access_jwt)
        .json(&json!({
            "repo": session.did,
            "collection": "app.bsky.feed.post",
            "rkey": rkey,
            "record": record,
        }))
        .send()
        .await
        .context("putRecord did not complete")?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("putRecord answered {status}: {}", body.chars().take(300).collect::<String>());
    }

    let at_uri = format!("at://{}/app.bsky.feed.post/{}", session.did, rkey);
    darkhold_db::atproto::set_at_uri(state.db(), &work.id, &at_uri).await?;
    tracing::info!(work = %work.id, %at_uri, "published an ATProto record");
    Ok(())
}

/// Delete the record for a work that was withdrawn (W6 §5: "a tombstone never leaves a dangling `at://`").
///
/// Three properties, each of which a purge needs to be safe to run repeatedly:
///   * no `at_uri` means nothing was ever published — `Ok(())`, not an error, so a withdrawal of an unbridged work is free;
///   * a record the PDS says is already gone (`RecordNotFound`) is treated as deleted, because the estate's goal is the
///     absence of the record, not the history of how it went away;
///   * the column is cleared **after** the PDS confirms, so a failed delete leaves the `at_uri` behind for the next attempt
///     instead of forgetting a record that is still live.
pub async fn withdraw_work(state: &AppState, work_id: &str) -> Result<()> {
    let Some(at_uri) = darkhold_db::atproto::at_uri_for(state.db(), work_id).await? else {
        return Ok(());
    };
    // `at://did/collection/rkey` — the rkey is everything after the last slash, and the collection is the segment before it.
    let mut parts = at_uri.rsplitn(3, '/');
    let rkey = parts.next().unwrap_or_default().to_owned();
    let collection = parts.next().unwrap_or_default().to_owned();
    let repo = at_uri.trim_start_matches("at://").split('/').next().unwrap_or_default().to_owned();
    if rkey.is_empty() || collection.is_empty() || repo.is_empty() {
        anyhow::bail!("cannot parse an at:// URI: {at_uri}");
    }

    // Withdrawn records are deleted even when the bridge has since been switched off: leaving one behind because the
    // publish path is disabled is exactly the dangling `at://` this function exists to prevent.
    let Some(config) = state.config().atproto.as_ref() else {
        return Ok(());
    };
    let password = std::env::var(&config.password_env)
        .with_context(|| format!("{} is not set in this unit's environment", config.password_env))?;
    let http = state.reqwest_client();
    let session = create_session(http, config, &password).await?;

    let response = http
        .post(format!("{}/xrpc/com.atproto.repo.deleteRecord", config.pds.trim_end_matches('/')))
        .bearer_auth(&session.access_jwt)
        .json(&json!({ "repo": repo, "collection": collection, "rkey": rkey }))
        .send()
        .await
        .context("deleteRecord did not complete")?;

    let status = response.status();
    if status.is_success() {
        darkhold_db::atproto::clear_at_uri(state.db(), work_id).await?;
        tracing::info!(work = %work_id, %at_uri, "deleted the ATProto record for a withdrawn work");
        return Ok(());
    }
    let body = response.text().await.unwrap_or_default();
    if body.contains("RecordNotFound") {
        darkhold_db::atproto::clear_at_uri(state.db(), work_id).await?;
        tracing::info!(work = %work_id, %at_uri, "the ATProto record was already gone");
        return Ok(());
    }
    anyhow::bail!("deleteRecord answered {status}: {}", body.chars().take(300).collect::<String>());
}

/// A session on the PDS. Created per delivery, not cached: a cached JWT is a second thing that expires, and at this
/// volume the extra round trip costs nothing.
async fn create_session(
    http: &reqwest::Client,
    config: &crate::config::AtprotoConfig,
    password: &str,
) -> Result<Session> {
    let response = http
        .post(format!("{}/xrpc/com.atproto.server.createSession", config.pds.trim_end_matches('/')))
        .json(&json!({ "identifier": config.handle, "password": password }))
        .send()
        .await
        .context("createSession did not complete")?;
    let status = response.status();
    let body: serde_json::Value = response.json().await.unwrap_or_else(|_| json!({}));
    if !status.is_success() {
        anyhow::bail!("createSession answered {status}: {}", body.to_string().chars().take(200).collect::<String>());
    }
    Ok(Session {
        did: body["did"].as_str().context("createSession returned no did")?.to_owned(),
        access_jwt: body["accessJwt"].as_str().context("createSession returned no accessJwt")?.to_owned(),
    })
}

struct Session {
    did: String,
    access_jwt: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use darkhold_db::atproto::WorkForRecord;

    fn work() -> WorkForRecord {
        WorkForRecord {
            id: "67430ccc-9945-4145-9503-e602c0d91186".into(),
            title: "The Long Way Round".into(),
            summary: "A time loop, slowly. ".repeat(40),
            language: "en".into(),
            created_at: "2026-11-06T10:00:00Z".into(),
        }
    }

    #[test]
    fn the_same_work_always_gets_the_same_record_key() {
        assert_eq!(rkey_for(&work().id), rkey_for(&work().id));
        assert_ne!(rkey_for(&work().id), rkey_for("another-work-id"));
        assert_eq!(rkey_for(&work().id).len(), 24);
    }

    #[test]
    fn the_record_points_at_the_canonical_page_and_copies_no_body() {
        let r = record_for(&work(), "https://darkhold.polarisocial.xyz", "2026-11-06T10:00:00Z");
        assert_eq!(
            r["embed"]["external"]["uri"],
            "https://darkhold.polarisocial.xyz/w/67430ccc-9945-4145-9503-e602c0d91186"
        );
        // 300 bytes is the post text budget; the body must never be in there.
        assert!(r["text"].as_str().unwrap().len() < 300, "the post text is a pointer, not the work");
        assert!(r["embed"]["external"]["description"].as_str().unwrap().len() <= 200);
        assert_eq!(r["facets"][0]["features"][0]["uri"], r["embed"]["external"]["uri"]);
    }

    #[test]
    fn a_multi_byte_summary_is_cut_on_a_character_boundary() {
        let mut w = work();
        w.summary = "é".repeat(400);
        let r = record_for(&w, "https://darkhold.polarisocial.xyz", "2026-11-06T10:00:00Z");
        let description = r["embed"]["external"]["description"].as_str().unwrap();
        assert!(description.len() <= 400, "cut in bytes, not characters: {} bytes", description.len());
        assert!(description.chars().all(|c| c == 'é'));
    }
}

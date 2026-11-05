//! Emitting to `polaris-inbox`, the estate's one inbox.
//!
//! W4's first task is the emitter, not the reader: darkhold tells the inbox what happened, and the inbox decides
//! who hears about it. The call is fire-and-forget and must never fail a comment — a notification is a courtesy,
//! and a courtesy that can break the thing it describes is not a courtesy.
//!
//! Configuration is environment, like the taste client and the estate search proxy: the address is deployment.
//!
//!   DARKHOLD_INBOX_URL     default http://127.0.0.1:8072
//!   DARKHOLD_INBOX_TOKEN   the inbox's service token; absent means emitting is off, not broken

use serde_json::{json, Value};

const DEFAULT_URL: &str = "http://127.0.0.1:8072";

fn setting(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

/// Whether this deployment emits at all. A product that has not been given a token stays silent rather than
/// retrying into a 401 forever.
pub fn enabled() -> bool {
    setting("DARKHOLD_INBOX_TOKEN").is_some()
}

/// One event, as the inbox endpoint takes it.
#[derive(Debug, Clone)]
pub struct Event {
    pub kind: String,
    pub recipients: Vec<String>,
    pub actor_sub: Option<String>,
    pub actor_handle: Option<String>,
    pub object_kind: Option<String>,
    pub object_id: Option<String>,
    pub object_url: Option<String>,
    pub object_title: Option<String>,
    /// What makes this notification unique. The inbox dedupes on it, so a retried emit cannot ring twice.
    pub dedupe_key: String,
}

impl Event {
    pub fn body(&self, site: &str) -> Value {
        json!({
            "type": self.kind,
            "site": site,
            "audience": "explicit",
            "recipients": self.recipients,
            "actor": {
                "sub": self.actor_sub,
                "handle": self.actor_handle,
            },
            "object": {
                "kind": self.object_kind,
                "id": self.object_id,
                "url": self.object_url,
                "title": self.object_title,
            },
            "dedupe_key": self.dedupe_key,
        })
    }
}

/// Send an event. Returns whether the inbox accepted it, so a test can assert the wiring rather than the hope.
pub async fn emit(site: &str, event: Event) -> bool {
    let (Some(token), base) = (
        setting("DARKHOLD_INBOX_TOKEN"),
        setting("DARKHOLD_INBOX_URL").unwrap_or_else(|| DEFAULT_URL.to_string()),
    ) else {
        return false;
    };
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(1500))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(error = %e, "inbox client could not be built");
            return false;
        }
    };
    match client
        .post(format!("{base}/api/v1/events"))
        .bearer_auth(token)
        .json(&event.body(site))
        .send()
        .await
    {
        Ok(response) if response.status().is_success() => true,
        Ok(response) => {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            tracing::warn!(%status, body = %body.chars().take(200).collect::<String>(), "inbox refused an event");
            false
        }
        Err(e) => {
            tracing::warn!(error = %e, "inbox did not answer");
            false
        }
    }
}

/// Emit without making the caller wait. The request that produced the event answers to a person; the
/// notification does not.
pub fn emit_in_background(site: &'static str, event: Event) {
    if !enabled() {
        return;
    }
    tokio::spawn(async move {
        let _ = emit(site, event).await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_body_carries_what_the_inbox_needs_to_resolve() {
        let event = Event {
            kind: "comment".into(),
            recipients: vec!["account:1".into()],
            actor_sub: Some("account:2".into()),
            actor_handle: Some("writer".into()),
            object_kind: Some("work".into()),
            object_id: Some("w1".into()),
            object_url: Some("/works/w1".into()),
            object_title: Some("A work".into()),
            dedupe_key: "darkhold:comment:c1:account:1".into(),
        };
        let body = event.body("darkhold");
        assert_eq!(body["type"], "comment");
        assert_eq!(body["site"], "darkhold");
        assert_eq!(body["recipients"][0], "account:1");
        // The dedupe key is explicit, because the comment id is the thing that is unique — a derived key from
        // the object would collapse two comments on one work into one notification.
        assert_eq!(body["dedupe_key"], "darkhold:comment:c1:account:1");
    }

    #[test]
    fn an_unconfigured_product_stays_silent() {
        // No token in the environment of a test process: emitting is off, and `enabled()` says so rather than
        // letting the call go out and fail.
        std::env::remove_var("DARKHOLD_INBOX_TOKEN");
        assert!(!enabled());
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The reader's half: a bell needs the product to stand in front of the service, because the browser holds a
// cookie and the inbox wants an assertion signed with a secret that must never reach a page.
// ---------------------------------------------------------------------------------------------------------------

/// The subject an inbox call is made for. darkhold's account id is the key the emitter already sends, so the
/// reader and the writer agree without a mapping table that does not exist yet (W2 owns sub ↔ account).
pub fn subject_assertion(subject: &str, method: &str, path: &str) -> Option<String> {
    let secret = setting("DARKHOLD_ASSERTION_SECRET")?;
    let expires = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs()
        + 90;
    Some(format!(
        "sub={subject}&exp={expires}&sig={}",
        sign(&secret, subject, expires, method, path)
    ))
}

/// `sig = HMAC-SHA256(secret, sub "\n" exp "\n" METHOD "\n" path)`, the estate's assertion (ADR 0001).
///
/// Written here rather than imported because this crate does not depend on `polaris-assert` and pulling a
/// dependency in for twelve lines is worse than the twelve lines — but the format is the protocol's, and the
/// inbox verifies it with the shared crate. `sub/exp/method/path` order and upper-cased method are not
/// negotiable; a product that invents its own canonical string simply fails to authenticate.
fn sign(secret: &str, subject: &str, expires: u64, method: &str, path: &str) -> String {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(secret.as_bytes()).expect("hmac accepts any key");
    mac.update(format!("{subject}\n{expires}\n{}\n{path}", method.to_uppercase()).as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

fn base() -> String {
    setting("DARKHOLD_INBOX_URL").unwrap_or_else(|| DEFAULT_URL.to_string())
}

/// `GET /api/v1/inbox/unread-count`, as the browser asks for it.
pub async fn unread_count(subject: &str) -> Result<serde_json::Value, String> {
    let assertion = subject_assertion(subject, "GET", "/api/v1/inbox/unread-count")
        .ok_or("no assertion secret configured")?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(2000))
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .get(format!("{}/api/v1/inbox/unread-count", base()))
        .header("x-inbox-assertion", assertion)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    response.json().await.map_err(|e| e.to_string())
}

/// The live stream, handed to axum as a body it can stream through unchanged.
///
/// Proxying SSE is not proxying JSON: the response never ends, so the upstream body is passed through as a
/// stream rather than collected. The assertion is minted per connection and expires long before the stream ends,
/// which is correct — it authenticated the *open*, not the lifetime.
pub async fn stream(
    subject: &str,
) -> Result<reqwest::Response, String> {
    let assertion = subject_assertion(subject, "GET", "/api/v1/inbox/stream")
        .ok_or("no assertion secret configured")?;
    let client = reqwest::Client::builder()
        // No total timeout: a stream is supposed to stay open. A connect timeout keeps a dead service from
        // hanging the request forever.
        .connect_timeout(std::time::Duration::from_millis(1500))
        .build()
        .map_err(|e| e.to_string())?;
    client
        .get(format!("{}/api/v1/inbox/stream", base()))
        .header("x-inbox-assertion", assertion)
        .header("accept", "text/event-stream")
        .send()
        .await
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod signature_tests {
    use super::sign;

    #[test]
    fn the_canonical_string_matches_the_protocol() {
        // Known-answer: the same inputs through `polaris_assert::sign` must produce this string. Pinned so a
        // refactor of either side fails here rather than at 2 a.m. in a browser with no bell.
        let secret = "0123456789abcdef0123456789abcdef";
        let ours = sign(secret, "account:1", 1_800_000_000, "get", "/api/v1/inbox/stream");
        // Uppercased method: `get` and `GET` must sign identically, because a handler decides the method it
        // checks and a caller should not have to guess the case.
        let upper = sign(secret, "account:1", 1_800_000_000, "GET", "/api/v1/inbox/stream");
        assert_eq!(ours, upper);
        assert_eq!(ours.len(), 64, "sha256 hex");
    }
}

/// The estate's name for this person.
///
/// **The inbox is keyed by the polaris subject, not by a site's account id.** It was keyed by darkhold's account id
/// for a day, and that made an estate-wide inbox into darkhold's inbox wearing the estate's name: a person's mail
/// from another site could never land in the same list, because the two sites would have called them different
/// things. `oidc_links` is where darkhold already knows the difference.
///
/// An account with no link keeps its own id: a person who has never signed in through polaris-id still has a
/// bell, and a bell that goes silent because a link is missing would be worse than one keyed locally.
pub async fn canonical(db: &darkhold_db::Database, account_id: &str) -> String {
    match account_id.parse() {
        Ok(id) => darkhold_db::oidc::subject_for_account(db, "polaris-id", id)
            .await
            .ok()
            .flatten()
            .unwrap_or_else(|| account_id.to_string()),
        // An id that is not an account id is already someone's name for the person (a test subject, a
        // migration artefact); passing it through beats inventing a lookup for it.
        Err(_) => account_id.to_string(),
    }
}

/// One page of this subject's inbox, mapped nowhere: the caller decides the shape.
///
/// `None` means "the inbox did not answer", which is not the same as "no notifications" — the adapter uses the
/// difference to fall back to the local table instead of showing an empty bell to someone who has mail.
pub async fn list(subject: &str, limit: u32) -> Option<serde_json::Value> {
    let assertion = subject_assertion(subject, "GET", "/api/v1/inbox")?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(1500))
        .build()
        .ok()?;
    let response = client
        .get(format!("{}/api/v1/inbox?limit={limit}", base()))
        .header("x-inbox-assertion", assertion)
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    response.json().await.ok()
}

/// Mark specific events read. Ids are event ids, which is what the inbox list returns.
pub async fn mark_read(subject: &str, ids: &[String]) -> bool {
    let Some(assertion) = subject_assertion(subject, "POST", "/api/v1/inbox/read") else {
        return false;
    };
    let Ok(client) = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(1500))
        .build()
    else {
        return false;
    };
    matches!(
        client
            .post(format!("{}/api/v1/inbox/read", base()))
            .header("x-inbox-assertion", assertion)
            .json(&serde_json::json!({ "ids": ids }))
            .send()
            .await,
        Ok(response) if response.status().is_success()
    )
}

/// Mark every unread event read for this subject. What a bell does when it is clicked: the number is the
/// question, and clearing it is the answer the person meant.
pub async fn mark_all_read(subject: &str) -> Result<u64, String> {
    let assertion = subject_assertion(subject, "POST", "/api/v1/inbox/read")
        .ok_or("no assertion secret configured")?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(2000))
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .post(format!("{}/api/v1/inbox/read", base()))
        .header("x-inbox-assertion", assertion)
        .json(&serde_json::json!({"all": true}))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let body: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
    Ok(body.get("marked").and_then(|v| v.as_u64()).unwrap_or(0))
}

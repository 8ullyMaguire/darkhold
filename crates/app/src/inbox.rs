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

//! XP, from this product's side (W4 task 5).
//!
//! **The award is a claim about something that happened, and only this product knows it happened.** That is why the
//! loops service takes a service token: a browser saying "I posted" is not evidence of a post.
//!
//! Two rules the code enforces rather than documents:
//!
//! * **the amount is not sent.** The service owns the weights, so a product cannot inflate its own site's score by
//!   deciding a reply is worth fifty.
//! * **the object key is the thing acted on, not the action.** The service's unique constraint is
//!   `(sub, site, verb, object_key)`, so a retry, a resend or a double click cannot pay twice.

use std::time::Duration;

fn endpoint() -> Option<(String, String)> {
    let url = std::env::var("DARKHOLD_LOOPS_URL").ok()?;
    let token = std::env::var("DARKHOLD_LOOPS_TOKEN").ok()?;
    if url.is_empty() || token.is_empty() {
        return None;
    }
    Some((url, token))
}

/// Award, without making the writer wait.
///
/// A person publishing a comment waits for the comment, not for a number to move. A failed award is a log line: XP
/// can be reconciled from the ledger later, and a comment that failed because a gamification service was down is a
/// comment that should have been saved anyway.
pub fn award_in_background(site: &'static str, sub: String, verb: &'static str, object_key: String) {
    let Some((url, token)) = endpoint() else {
        return;
    };
    tokio::spawn(async move {
        let Ok(client) = reqwest::Client::builder()
            .timeout(Duration::from_millis(1500))
            .build()
        else {
            return;
        };
        match client
            .post(format!("{url}/api/v1/xp/award"))
            .bearer_auth(&token)
            .json(&serde_json::json!({
                "sub": sub, "site": site, "verb": verb, "object_key": object_key,
            }))
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => {
                if let Ok(body) = response.json::<serde_json::Value>().await {
                    if body.get("awarded").and_then(|v| v.as_bool()) == Some(false) {
                        // A refusal is not an error: "already awarded" is the anti-farming rule working.
                        tracing::debug!(reason = ?body.get("reason"), "xp not awarded");
                    }
                }
            }
            Ok(response) => tracing::warn!(status = %response.status(), "the loops service refused an award"),
            Err(error) => tracing::warn!(%error, "the loops service is unreachable; the comment is unaffected"),
        }
    });
}

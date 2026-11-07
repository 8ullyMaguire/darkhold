//! M16 — Webhook domain: event envelope, HMAC signing, bounded payload.

use hmac::{Hmac, Mac};
use rand::RngCore as _;
use sha2::Sha256;

/// Webhook event envelope.
#[derive(Debug, Clone)]
pub struct WebhookEvent {
    pub event_type: String,
    pub event_id: String,
    pub created_at: String,
    pub payload: serde_json::Value,
}

impl WebhookEvent {
    /// Serialize the envelope for signing.
    pub fn canonical_string(&self) -> String {
        format!("{}:{}:{}", self.event_type, self.event_id, self.created_at)
    }

    /// Compute HMAC-SHA256 signature over the canonical string + payload.
    ///
    /// Uses proper HMAC-SHA256 (RFC 2104) with a 32-byte output, encoded as hex.
    /// The signing input is `canonical_string + payload.to_string()`.
    pub fn sign(&self, secret: &str) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
            .expect("HMAC can take a key of any size");
        mac.update(self.canonical_string().as_bytes());
        mac.update(self.payload.to_string().as_bytes());
        let result = mac.finalize();
        hex::encode(result.into_bytes())
    }

    /// Verify a signature against the envelope.
    pub fn verify(&self, secret: &str, signature: &str) -> bool {
        self.sign(secret) == signature
    }
}

/// Bound a payload to a maximum size (in bytes). Returns the original if within bounds,
/// or a truncated version with a marker if over.
pub fn bound_payload(payload: &serde_json::Value, max_bytes: usize) -> serde_json::Value {
    let serialized = payload.to_string();
    if serialized.len() <= max_bytes {
        payload.clone()
    } else {
        serde_json::json!({
            "_truncated": true,
            "_original_bytes": serialized.len(),
            "_max_bytes": max_bytes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn webhook_sign_and_verify() {
        let event = WebhookEvent {
            event_type: "work.published".to_string(),
            event_id: "evt-123".to_string(),
            created_at: "2026-09-14T12:00:00Z".to_string(),
            payload: serde_json::json!({"work_id": "w-1"}),
        };

        let secret = "whsec_test";
        let sig = event.sign(secret);
        assert!(event.verify(secret, &sig));
        assert!(!event.verify("wrong-secret", &sig));
    }

    /// RFC 4231 test case 2: key = "Jefe", data = "what do ya want for nothing?".
    /// Tested directly since `sign()` prepends the event envelope.
    #[test]
    fn hmac_matches_rfc4231_test_case_2() {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;

        let mut mac = Hmac::<Sha256>::new_from_slice(b"Jefe").unwrap();
        mac.update(b"what do ya want for nothing?");
        let result = mac.finalize();
        assert_eq!(
            hex::encode(result.into_bytes()),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn payload_within_bounds() {
        let payload = serde_json::json!({"key": "value"});
        let bounded = bound_payload(&payload, 1000);
        assert_eq!(bounded, payload);
    }

    #[test]
    fn payload_truncated() {
        let payload = serde_json::json!({"large": "x".repeat(1000)});
        let bounded = bound_payload(&payload, 100);
        assert!(bounded["_truncated"].as_bool().unwrap());
    }
}

// ---------------------------------------------------------------------------
// M16-14: the operational half — rotation, rate limiting, auto-disable.
// ---------------------------------------------------------------------------

/// How long a rotated-away secret stays admissible, in seconds.
///
/// A rotation that takes effect instantly breaks every receiver mid-deploy: the subscriber has to deploy the new secret to its own
/// configuration, and a window shorter than its deploy is a window where deliveries fail for a reason that looks like an outage. A day is
/// long enough for a human to act and short enough that a leaked secret stops working.
pub const ROTATION_GRACE_SECONDS: i64 = 24 * 60 * 60;

/// A signing key, named by which generation it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyId {
    /// The secret currently published to the subscriber.
    Current,
    /// The secret it replaced, admissible for [`ROTATION_GRACE_SECONDS`].
    Previous,
}

impl KeyId {
    /// The wire prefix: `v1` is current, `v0` is the one it replaced.
    ///
    /// Numbered the way a receiver reads it — a verifier tries the keys it holds and accepts if either matches — rather than by age, because
    /// a name that says "previous" would become wrong the second time somebody rotates.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Current => "v1",
            Self::Previous => "v0",
        }
    }
}

/// A fresh signing secret.
///
/// 32 random bytes, hex-encoded, with a `whsec_` prefix so a secret pasted into a log is recognisable as one. This is the only place a secret
/// is generated, so "how strong is a webhook secret" has one answer.
#[must_use]
pub fn new_secret() -> String {
    let mut bytes = [0u8; 32];
    // rand 0.8: `thread_rng()` is the portable source here; the crate is pinned below 0.9 in the workspace.
    rand::thread_rng().fill_bytes(&mut bytes);
    format!("whsec_{}", hex::encode(bytes))
}

/// The secrets admissible at a given instant, current first.
///
/// `rotated_at` is a Unix timestamp. A `None` previous secret, or a rotation older than the grace window, yields only the current key — which
/// is what makes the grace a window rather than a permanent second key.
#[must_use]
pub fn admissible_keys<'a>(
    current: &'a str,
    previous: Option<&'a str>,
    rotated_at_epoch: Option<i64>,
    now_epoch: i64,
) -> Vec<(KeyId, &'a str)> {
    let mut keys = vec![(KeyId::Current, current)];
    if let (Some(previous), Some(rotated_at)) = (previous, rotated_at_epoch) {
        if now_epoch - rotated_at < ROTATION_GRACE_SECONDS && !previous.is_empty() {
            keys.push((KeyId::Previous, previous));
        }
    }
    keys
}

/// The `X-Signature` header value for an event, signed by every admissible key.
///
/// `v1=<hex>` always, plus `v0=<hex>` during a rotation's grace window. One header rather than two so a receiver needs no special case after
/// a rotation: it verifies against the keys it holds and accepts a match on either.
#[must_use]
pub fn signing_header(event: &WebhookEvent, keys: &[(KeyId, &str)]) -> String {
    keys.iter()
        .map(|(id, secret)| format!("{}={}", id.as_str(), event.sign(secret)))
        .collect::<Vec<String>>()
        .join(",")
}

/// How many deliveries an endpoint may receive in one minute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimit {
    pub per_minute: u32,
}

impl RateLimit {
    /// Whether another delivery fits in this minute.
    ///
    /// A limit of zero is refused at the call site rather than read as "unlimited": a subscription with a limit of nothing is a
    /// configuration mistake, and treating it as unlimited makes that mistake invisible.
    #[must_use]
    pub fn allows(self, recent_deliveries: u64) -> bool {
        recent_deliveries < u64::from(self.per_minute)
    }
}

/// When a failing endpoint should be switched off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Breaker {
    /// How many *consecutive* failures switch the endpoint off.
    pub threshold: u32,
}

/// The default breaker: twenty consecutive failures.
///
/// Consecutive rather than cumulative on purpose. A subscriber that fails one delivery a day for a year is a subscriber with a flaky
/// receiver, and switching it off would be the host deciding that its own retries matter more than the subscriber's uptime. Twenty in a row
/// is a receiver that is gone.
pub const DEFAULT_FAILURE_THRESHOLD: u32 = 20;

/// What the breaker decided after a delivery outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BreakerDecision {
    /// Nothing changed, or a success reset the count.
    Continue { consecutive_failures: u32 },
    /// Switch the endpoint off, with the reason a reader will see.
    Disable { consecutive_failures: u32, reason: String },
}

impl Breaker {
    /// Apply a successful delivery: the count goes to zero.
    #[must_use]
    pub fn after_success(self) -> BreakerDecision {
        BreakerDecision::Continue {
            consecutive_failures: 0,
        }
    }

    /// Apply a failure, given the count *before* it.
    #[must_use]
    pub fn after_failure(self, previous_failures: u32) -> BreakerDecision {
        let consecutive_failures = previous_failures.saturating_add(1);
        if self.threshold > 0 && consecutive_failures >= self.threshold {
            BreakerDecision::Disable {
                consecutive_failures,
                reason: format!(
                    "switched off after {consecutive_failures} consecutive failed deliveries; its receiver has not answered"
                ),
            }
        } else {
            BreakerDecision::Continue {
                consecutive_failures,
            }
        }
    }
}

#[cfg(test)]
mod operational_tests {
    use super::*;

    fn event() -> WebhookEvent {
        WebhookEvent {
            event_type: "work.published".to_owned(),
            event_id: "evt-1".to_owned(),
            created_at: "2026-11-07T00:00:00Z".to_owned(),
            payload: serde_json::json!({"work_id": "w-1"}),
        }
    }

    #[test]
    fn a_rotation_keeps_the_old_secret_admissible_only_for_the_grace_window() {
        let current = "whsec_new";
        let previous = "whsec_old";
        let rotated = 1_000_000;

        let inside = admissible_keys(current, Some(previous), Some(rotated), rotated + 60);
        assert_eq!(
            inside,
            vec![(KeyId::Current, current), (KeyId::Previous, previous)],
            "both keys during the window, current first"
        );

        let after = admissible_keys(
            current,
            Some(previous),
            Some(rotated),
            rotated + ROTATION_GRACE_SECONDS,
        );
        assert_eq!(after, vec![(KeyId::Current, current)], "the grace is a window");
    }

    #[test]
    fn a_first_secret_has_no_previous_and_an_empty_one_is_not_a_key() {
        assert_eq!(
            admissible_keys("whsec_only", None, None, 0),
            vec![(KeyId::Current, "whsec_only")]
        );
        assert_eq!(
            admissible_keys("whsec_only", Some(""), Some(1), 2),
            vec![(KeyId::Current, "whsec_only")],
            "an empty secret would sign every payload identically"
        );
    }

    #[test]
    fn the_header_carries_both_keys_during_a_rotation() {
        let single = signing_header(&event(), &[(KeyId::Current, "whsec_a")]);
        assert!(single.starts_with("v1="));
        assert!(!single.contains("v0="));

        let rotated = signing_header(
            &event(),
            &[(KeyId::Current, "whsec_a"), (KeyId::Previous, "whsec_b")],
        );
        assert!(rotated.contains("v1=") && rotated.contains("v0="), "{rotated}");
        // The two signatures differ, which is the point: a receiver can tell which key verified.
        let parts: Vec<&str> = rotated.split(',').collect();
        assert_eq!(parts.len(), 2);
        assert_ne!(parts[0], parts[1]);
    }

    #[test]
    fn a_generated_secret_is_long_random_and_recognisable() {
        let first = new_secret();
        let second = new_secret();
        assert!(first.starts_with("whsec_"));
        assert_eq!(first.len(), "whsec_".len() + 64, "32 bytes of hex");
        assert_ne!(first, second);
    }

    #[test]
    fn the_rate_limit_counts_within_the_minute_it_names() {
        let limit = RateLimit { per_minute: 60 };
        assert!(limit.allows(59));
        assert!(!limit.allows(60));
        assert!(!limit.allows(61));
        // A limit of zero allows nothing rather than everything.
        assert!(!RateLimit { per_minute: 0 }.allows(0));
    }

    #[test]
    fn the_breaker_counts_consecutive_failures_and_success_resets_them() {
        let breaker = Breaker { threshold: 3 };
        assert_eq!(
            breaker.after_failure(0),
            BreakerDecision::Continue {
                consecutive_failures: 1
            }
        );
        assert_eq!(
            breaker.after_success(),
            BreakerDecision::Continue {
                consecutive_failures: 0
            }
        );
        match breaker.after_failure(2) {
            BreakerDecision::Disable { consecutive_failures, reason } => {
                assert_eq!(consecutive_failures, 3);
                assert!(reason.contains("consecutive"), "{reason}");
            }
            other => panic!("a third consecutive failure must switch it off: {other:?}"),
        }
    }

    #[test]
    fn a_breaker_with_no_threshold_disables_nothing() {
        let breaker = Breaker { threshold: 0 };
        assert!(matches!(
            breaker.after_failure(1_000),
            BreakerDecision::Continue { .. }
        ));
    }
}

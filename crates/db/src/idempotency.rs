//! One rule for idempotency keys: what a key may look like, what a caller gets when they send none, and what a duplicate
//! means. (ADR 0025)
//!
//! The product had three styles before this module, and a caller who sent no key got three different guarantees by route:
//! a purchase was deduplicated by a key derived from buyer and subject, a publication was **not deduplicated at all**, and a
//! job enqueue could not happen without one. Two of the three also capped nothing, so the same over-long key was a field error
//! on one route and a stored string on another.
//!
//! What stays per-call is [`OnDuplicate`]: a purchase retry must return the **first receipt**, a publication retry must return
//! `AlreadyApplied` without writing, and a duplicate job must be **skipped**. Those are product decisions, not a rule the
//! module may choose; naming them here is what stops a fourth style appearing silently.

/// The most a caller-supplied key may be, in **characters**.
///
/// Characters, not bytes: the payment route counted `str::len()`, so a 150-character key written in a non-latin script (300
/// bytes) was refused while a 199-character one was accepted — a cap that depends on the alphabet is not a cap.
pub const MAX_KEY_CHARS: usize = 200;

/// What a duplicate key means on this route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnDuplicate {
    /// Return the row the first call wrote. A purchase retry is a request for the receipt, not a second sale.
    Replay,
    /// Change nothing and report that nothing changed.
    AlreadyApplied,
    /// Do not enqueue a second time.
    Skip,
}

/// The two decisions a route makes about keys: how long one may be, and what a duplicate means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyPolicy {
    pub max_len: usize,
    pub on_duplicate: OnDuplicate,
}

impl KeyPolicy {
    /// `routes/payments.rs`: derive from buyer and subject when the caller sends none, and replay the original payment.
    pub const PAYMENT: KeyPolicy = KeyPolicy {
        max_len: MAX_KEY_CHARS,
        on_duplicate: OnDuplicate::Replay,
    };

    /// `content::publish_work`: **no** key means no dedupe — a caller who wants a replay-safe publish has to ask for one.
    pub const PUBLICATION: KeyPolicy = KeyPolicy {
        max_len: MAX_KEY_CHARS,
        on_duplicate: OnDuplicate::AlreadyApplied,
    };

    /// The job queue: keys are derived from the source item, and a duplicate enqueue is skipped.
    pub const JOB: KeyPolicy = KeyPolicy {
        max_len: MAX_KEY_CHARS,
        on_duplicate: OnDuplicate::Skip,
    };
}

/// Why a key was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyRefusal {
    /// Longer than the policy allows. Carries the measured length so the error can say what it saw.
    TooLong { len: usize, max: usize },
}

impl KeyRefusal {
    /// The sentence a person reads. One wording, because three routes inventing three ways to say "too long" is how a field
    /// error becomes three field errors.
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            KeyRefusal::TooLong { len, max } => {
                format!("an idempotency key is at most {max} characters (this one is {len})")
            }
        }
    }
}

/// The key a caller supplied, normalised: trimmed, and `None` when it was absent or only whitespace.
///
/// `None` in means `None` out, which is a real answer for [`KeyPolicy::PUBLICATION`] — publication does not invent a key.
///
/// # Errors
/// Returns [`KeyRefusal::TooLong`] when the trimmed key exceeds `policy.max_len` characters.
pub fn normalise(raw: Option<&str>, policy: KeyPolicy) -> Result<Option<String>, KeyRefusal> {
    let Some(trimmed) = raw.map(str::trim).filter(|key| !key.is_empty()) else {
        return Ok(None);
    };
    let len = trimmed.chars().count();
    if len > policy.max_len {
        return Err(KeyRefusal::TooLong {
            len,
            max: policy.max_len,
        });
    }
    Ok(Some(trimmed.to_owned()))
}

/// The key a caller actually gets: their own when they sent one, otherwise the derived one.
///
/// # Errors
/// Returns [`KeyRefusal::TooLong`] when the *caller's* key exceeds the cap. A derived key is machine-made and is not capped
/// here: refusing one would turn a construction bug into a failed request, and the derivation is the code that must change.
pub fn derive_or(
    raw: Option<&str>,
    derive: impl FnOnce() -> String,
    policy: KeyPolicy,
) -> Result<String, KeyRefusal> {
    match normalise(raw, policy)? {
        Some(key) => Ok(key),
        None => Ok(derive()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_caller_key_is_trimmed_and_kept() {
        assert_eq!(
            normalise(Some("  abc-123  "), KeyPolicy::PAYMENT),
            Ok(Some("abc-123".to_owned()))
        );
    }

    #[test]
    fn an_absent_or_blank_key_is_none_not_an_empty_string() {
        // This is the difference that made publication skip dedupe: "" and None must behave the same.
        assert_eq!(normalise(None, KeyPolicy::PUBLICATION), Ok(None));
        assert_eq!(normalise(Some("   "), KeyPolicy::PUBLICATION), Ok(None));
        assert_eq!(normalise(Some(""), KeyPolicy::PUBLICATION), Ok(None));
    }

    #[test]
    fn the_cap_counts_characters_not_bytes() {
        // 150 characters in a script where each is three bytes: 450 bytes, 150 characters. The old `len() > 200` refused it.
        let key: String = "鍵".repeat(150);
        assert!(key.len() > 200, "the fixture must be longer than 200 bytes");
        assert_eq!(
            normalise(Some(&key), KeyPolicy::PAYMENT),
            Ok(Some(key.clone()))
        );
    }

    #[test]
    fn a_key_over_the_cap_is_refused_with_both_numbers() {
        let long = "a".repeat(201);
        let refusal = normalise(Some(&long), KeyPolicy::PAYMENT).unwrap_err();
        assert_eq!(refusal, KeyRefusal::TooLong { len: 201, max: 200 });
        assert!(refusal.message().contains("201") && refusal.message().contains("200"));
    }

    #[test]
    fn exactly_the_cap_is_accepted() {
        let key = "a".repeat(200);
        assert_eq!(normalise(Some(&key), KeyPolicy::PAYMENT), Ok(Some(key)));
    }

    #[test]
    fn a_payment_without_a_key_derives_one_from_what_was_bought() {
        let key = derive_or(
            None,
            || "extension:buyer-1:slug-1".to_owned(),
            KeyPolicy::PAYMENT,
        )
        .unwrap();
        assert_eq!(key, "extension:buyer-1:slug-1");
    }

    #[test]
    fn a_payment_with_a_key_ignores_the_derivation() {
        // The closure must not run: a derived key would silently replace the caller's retry token.
        let key = derive_or(
            Some("caller-token"),
            || panic!("derivation ran with a key present"),
            KeyPolicy::PAYMENT,
        )
        .unwrap();
        assert_eq!(key, "caller-token");
    }

    #[test]
    fn the_duplicate_behaviour_is_declared_per_route_and_differs() {
        assert_eq!(KeyPolicy::PAYMENT.on_duplicate, OnDuplicate::Replay);
        assert_eq!(
            KeyPolicy::PUBLICATION.on_duplicate,
            OnDuplicate::AlreadyApplied
        );
        assert_eq!(KeyPolicy::JOB.on_duplicate, OnDuplicate::Skip);
    }

    #[test]
    fn every_policy_shares_one_cap() {
        // Three routes, one number. The defect this closes was two routes with no cap at all.
        assert_eq!(KeyPolicy::PAYMENT.max_len, MAX_KEY_CHARS);
        assert_eq!(KeyPolicy::PUBLICATION.max_len, MAX_KEY_CHARS);
        assert_eq!(KeyPolicy::JOB.max_len, MAX_KEY_CHARS);
    }
}

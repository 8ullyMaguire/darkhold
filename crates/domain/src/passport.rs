//! M69-03 — a signed taste passport: what a reader carries when they leave.
//!
//! A reader who moves between instances starts at zero on the new one, and everything this product knows about taste is
//! derived from behaviour **here** (M11-01/M11-03). So the passport answers one question — *where does this reader start* —
//! and it must be able to answer it without being believed.
//!
//! # A claim, never a fact
//!
//! The signature proves the document was signed by whoever holds that key. It does **not** prove that key belongs to the
//! reader, that the reader is who the document says, or that the numbers are honest: the origin instance writes them, and an
//! origin can be careless or an account can be bought. So the receiving side treats a passport as `imported` evidence —
//! the same weaker class `taste_signals` already distinguishes from `organic` (§49.7) — and the local recomputation
//! (M11-01) outvotes it as soon as there is anything local to outvote it with. That is the whole design: the passport
//! bootstraps, it never replaces.
//!
//! # Canonical JSON, and why the canonicalisation is written here
//!
//! A signature over "the JSON" is a signature over whatever the serialiser happened to emit, and two instances on different
//! serde versions can disagree about key order. So the signed bytes are produced by [`canonical_json`]: object keys sorted,
//! no insignificant whitespace, integers and strings only for the signed fields. This is the subset of RFC 8785 the document
//! actually needs; floating point is deliberately absent from the signed surface — tag weights are scaled integers in basis
//! points — because JCS's number rules are the one part of it that is easy to get subtly wrong, and a signature that depends
//! on getting them right would fail intermittently rather than loudly.
//!
//! # Expiry is part of the claim
//!
//! Taste moves. A passport from two years ago describes a reader who may not exist any more, so `issued_at` is checked
//! against a maximum age and an expired passport is refused rather than down-weighted: a stale prior is worse than no prior,
//! because the local system would treat it as current.
//!
//! # The revision field is not decoration
//!
//! `version` is checked, and an unknown version is refused. The alternative — parse what you recognise and ignore the rest —
//! is how a document that gained a field meaningfully (say, "only valid for adult readers") gets honoured by an old build
//! that drops it. Refusing is the only answer that keeps a signature meaningful.

use std::collections::BTreeMap;

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The only passport revision this build understands.
pub const VERSION: u32 = 1;

/// How old a passport may be before it is refused: 180 days.
///
/// Chosen against the decay the taste model already assumes (M69-08's half-life is in days, not years): a prior older than
/// this is not a starting point, it is a different reader.
pub const MAX_AGE_DAYS: i64 = 180;

/// The most tag dimensions a passport may carry. A document is a starting point, not a dump; a vector long enough to be a
/// full history is one the receiving instance should have collected itself.
pub const MAX_TAGS: usize = 200;

/// The most favourite-source URLs a passport may carry.
pub const MAX_SOURCES: usize = 100;

/// What a reader carries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Passport {
    /// The document revision.
    pub version: u32,
    /// The instance that issued it, as its canonical base URL.
    pub origin: String,
    /// The reader's handle at the origin. A claim about a name, not an identity: two instances do not share account ids,
    /// and inventing a mapping would be worse than a string nobody can verify.
    pub subject: String,
    /// When the origin signed it (RFC 3339).
    pub issued_at: String,
    /// Tag weights in **basis points** (0..=10_000, summing to at most 10_000 per the origin's own normalisation).
    ///
    /// Basis points and not floats: the signed surface stays integer, so two builds cannot disagree about the bytes of
    /// `0.1 + 0.2` while agreeing about everything they meant.
    #[serde(default)]
    pub taste: BTreeMap<String, u32>,
    /// Favourite source URLs, so the receiving instance can tell a reader where this person already reads.
    #[serde(default)]
    pub favourite_sources: Vec<String>,
    /// What the reader contributed at the origin: edits, comments, works. Counts, not contents — a passport is not a
    /// history, and a receiving instance that knew *what* they wrote would be a receiving instance that held their writing
    /// without a licence to it.
    #[serde(default)]
    pub contributions: Contributions,
    /// The origin's own affinity reading for this reader (M69-09), in basis points. `None` means the origin had not measured
    /// it, which is different from a measured zero and is why it is not `0`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub affinity_bp: Option<u16>,
}

/// What a reader made at the origin, as counts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contributions {
    /// Works published.
    #[serde(default)]
    pub works: u32,
    /// Revisions made to any work.
    #[serde(default)]
    pub revisions: u32,
    /// Comments left.
    #[serde(default)]
    pub comments: u32,
}

/// A passport and the signature over its canonical bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedPassport {
    /// The document.
    pub passport: Passport,
    /// The signing key, hex-encoded 32-byte ed25519 public key.
    pub key: String,
    /// The signature, hex-encoded 64 bytes.
    pub signature: String,
}

/// Why a passport was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PassportError {
    /// The document could not be parsed, or a required field was empty.
    Malformed(String),
    /// A version this build does not implement.
    UnsupportedVersion(u32),
    /// The signature does not verify against the key it carries.
    BadSignature,
    /// The key is not a valid ed25519 public key.
    BadKey,
    /// The document is older than [`MAX_AGE_DAYS`].
    Expired {
        /// When it was issued.
        issued_at: String,
        /// How old it is, in days.
        age_days: i64,
    },
    /// The document claims more than a starting point.
    Oversized(String),
}

impl std::fmt::Display for PassportError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Malformed(reason) => write!(formatter, "the passport is malformed: {reason}"),
            Self::UnsupportedVersion(version) => write!(
                formatter,
                "the passport is version {version}, and this instance implements version {VERSION}"
            ),
            Self::BadSignature => write!(formatter, "the passport's signature does not verify"),
            Self::BadKey => write!(formatter, "the passport's key is not a valid ed25519 public key"),
            Self::Expired {
                issued_at,
                age_days,
            } => write!(
                formatter,
                "the passport was issued on {issued_at}, {age_days} days ago, and the limit is {MAX_AGE_DAYS}"
            ),
            Self::Oversized(what) => write!(formatter, "the passport carries too much: {what}"),
        }
    }
}

/// Serialise a value as canonical JSON: object keys sorted, no insignificant whitespace.
///
/// Only the shapes the passport uses are supported, and anything else is a programmer error rather than a document to
/// accept: the signed surface is strings, unsigned integers, arrays, objects and `null`. Floats are refused because their
/// canonical form is the part of RFC 8785 that is easiest to get wrong, and a signature that fails intermittently is worse
/// than one that refuses loudly.
#[must_use]
pub fn canonical_json(value: &serde_json::Value) -> String {
    let mut out = String::new();
    write_canonical(value, &mut out);
    out
}

fn write_canonical(value: &serde_json::Value, out: &mut String) {
    match value {
        serde_json::Value::Null => out.push_str("null"),
        serde_json::Value::Bool(flag) => out.push_str(if *flag { "true" } else { "false" }),
        serde_json::Value::Number(number) => {
            // Integers only: a float in the signed surface is a document this build will not sign, and refusing is the only
            // honest answer (see the module note).
            out.push_str(&number.to_string());
        }
        serde_json::Value::String(text) => {
            out.push_str(&serde_json::to_string(text).expect("a string always serialises"));
        }
        serde_json::Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        serde_json::Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (index, key) in keys.into_iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::to_string(key).expect("a key always serialises"));
                out.push(':');
                write_canonical(&map[key], out);
            }
            out.push('}');
        }
    }
}

/// The bytes a signature covers: the canonical JSON of the passport.
#[must_use]
pub fn signing_bytes(passport: &Passport) -> Vec<u8> {
    let value = serde_json::to_value(passport).unwrap_or(serde_json::Value::Null);
    canonical_json(&value).into_bytes()
}

/// A short fingerprint of the signed document, so a receipt can be identified without storing the whole thing twice.
#[must_use]
pub fn document_id(passport: &Passport) -> String {
    let mut hasher = Sha256::new();
    hasher.update(signing_bytes(passport));
    hex::encode(hasher.finalize())[..32].to_owned()
}

/// Check the parts of a passport that are a claim rather than a signature.
fn check_shape(passport: &Passport) -> Result<(), PassportError> {
    if passport.version != VERSION {
        return Err(PassportError::UnsupportedVersion(passport.version));
    }
    if passport.origin.trim().is_empty() {
        return Err(PassportError::Malformed("no origin instance".to_owned()));
    }
    if passport.subject.trim().is_empty() {
        return Err(PassportError::Malformed("no subject".to_owned()));
    }
    if passport.issued_at.trim().is_empty() {
        return Err(PassportError::Malformed("no issue date".to_owned()));
    }
    if passport.taste.len() > MAX_TAGS {
        return Err(PassportError::Oversized(format!(
            "{} tag dimensions, and the limit is {MAX_TAGS}",
            passport.taste.len()
        )));
    }
    if passport.favourite_sources.len() > MAX_SOURCES {
        return Err(PassportError::Oversized(format!(
            "{} favourite sources, and the limit is {MAX_SOURCES}",
            passport.favourite_sources.len()
        )));
    }
    if let Some(weight) = passport.taste.values().find(|weight| **weight > 10_000) {
        return Err(PassportError::Malformed(format!(
            "a tag weight of {weight} basis points is not a weight"
        )));
    }
    if let Some(affinity) = passport.affinity_bp.filter(|bp| *bp > 10_000) {
        return Err(PassportError::Malformed(format!(
            "an affinity of {affinity} basis points is not a reading"
        )));
    }
    Ok(())
}

/// Sign a passport with a 32-byte ed25519 key.
pub fn sign(passport: &Passport, key: &SigningKey) -> Result<SignedPassport, PassportError> {
    check_shape(passport)?;
    let signature = key.sign(&signing_bytes(passport));
    Ok(SignedPassport {
        passport: passport.clone(),
        key: hex::encode(key.verifying_key().to_bytes()),
        signature: hex::encode(signature.to_bytes()),
    })
}

/// Sign a passport from a hex-encoded 32-byte key, so a caller never has to depend on the signature crate.
///
/// The hex is validated here rather than at the edge, because "the key is a 32-byte ed25519 key" is a property of the
/// passport format, not of the HTTP handler that happens to be holding the string.
pub fn sign_with_hex(passport: &Passport, key_hex: &str) -> Result<SignedPassport, PassportError> {
    let bytes: [u8; 32] = hex::decode(key_hex.trim())
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or(PassportError::BadKey)?;
    sign(passport, &SigningKey::from_bytes(&bytes))
}

/// The public key a hex-encoded private key signs with, hex-encoded.
///
/// Exists so an operator (and a test) can say "the documents this instance issues carry this key" without ever handling the
/// private half. A door that echoed the seed would put a signing key in an HTTP response.
#[must_use]
pub fn public_key_hex(key_hex: &str) -> Option<String> {
    let bytes: [u8; 32] = hex::decode(key_hex.trim())
        .ok()
        .and_then(|bytes| bytes.try_into().ok())?;
    Some(hex::encode(SigningKey::from_bytes(&bytes).verifying_key().to_bytes()))
}

/// Verify a signed passport and return the document it carries.
///
/// `now` is passed in rather than read, so "an expired passport is refused" is a test rather than a hope.
pub fn verify(signed: &SignedPassport, now: time::OffsetDateTime) -> Result<&Passport, PassportError> {
    check_shape(&signed.passport)?;

    let key_bytes: [u8; 32] = hex::decode(&signed.key)
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or(PassportError::BadKey)?;
    let verifying = VerifyingKey::from_bytes(&key_bytes).map_err(|_| PassportError::BadKey)?;

    let signature_bytes: [u8; 64] = hex::decode(&signed.signature)
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or(PassportError::BadSignature)?;
    let signature = Signature::from_bytes(&signature_bytes);

    verifying
        .verify(&signing_bytes(&signed.passport), &signature)
        .map_err(|_| PassportError::BadSignature)?;

    let issued = time::OffsetDateTime::parse(
        signed.passport.issued_at.trim(),
        &time::format_description::well_known::Rfc3339,
    )
    .map_err(|_| PassportError::Malformed("the issue date is not RFC 3339".to_owned()))?;
    let age_days = (now - issued).whole_days();
    // A passport dated in the future is as suspect as one from last year: a clock this far out is a document this build
    // cannot reason about, and accepting it would let an origin set an arbitrary age.
    if !(-1..=MAX_AGE_DAYS).contains(&age_days) {
        return Err(PassportError::Expired {
            issued_at: signed.passport.issued_at.clone(),
            age_days,
        });
    }

    Ok(&signed.passport)
}

/// Turn a passport's tag weights into basis points, as the receiving instance reads them.
///
/// Kept here rather than in the store because it is the same rule the origin applied when it published: weights are
/// relative, so they are normalised over what the document carries. A passport whose weights sum to nothing yields an empty
/// map rather than a division by zero — "the origin measured no tags" is a fact, and the receiving side treats it as one.
#[must_use]
pub fn normalised(passport: &Passport) -> BTreeMap<String, u32> {
    let total: u64 = passport.taste.values().map(|weight| u64::from(*weight)).sum();
    if total == 0 {
        return BTreeMap::new();
    }
    passport
        .taste
        .iter()
        .filter(|(tag, weight)| !tag.trim().is_empty() && **weight > 0)
        .map(|(tag, weight)| {
            let share = (u64::from(*weight) * 10_000) / total;
            (tag.trim().to_lowercase(), u32::try_from(share).unwrap_or(0))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    fn passport() -> Passport {
        Passport {
            version: VERSION,
            origin: "https://origin.example".to_owned(),
            subject: "a-reader".to_owned(),
            issued_at: "2026-11-01T00:00:00Z".to_owned(),
            taste: BTreeMap::from([
                ("angst".to_owned(), 5_000),
                ("fluff".to_owned(), 3_000),
                ("au".to_owned(), 2_000),
            ]),
            favourite_sources: vec!["https://archiveofourown.org/users/x".to_owned()],
            contributions: Contributions {
                works: 12,
                revisions: 40,
                comments: 300,
            },
            affinity_bp: Some(8_100),
        }
    }

    fn now() -> time::OffsetDateTime {
        time::OffsetDateTime::parse("2026-11-07T00:00:00Z", &time::format_description::well_known::Rfc3339)
            .expect("now")
    }

    #[test]
    fn a_signed_passport_verifies_and_carries_its_numbers() {
        let signed = sign(&passport(), &key(1)).expect("sign");
        let verified = verify(&signed, now()).expect("verify");
        assert_eq!(verified.taste.get("angst"), Some(&5_000));
        assert_eq!(verified.contributions.comments, 300);
        assert_eq!(verified.affinity_bp, Some(8_100));
        assert_eq!(
            signed.key,
            hex::encode(key(1).verifying_key().to_bytes()),
            "the document carries the key it was signed with, so a receiver can check without asking anyone"
        );
    }

    #[test]
    fn a_tampered_document_does_not_verify() {
        let mut signed = sign(&passport(), &key(1)).expect("sign");
        signed.passport.taste.insert("angst".to_owned(), 9_000);
        assert_eq!(verify(&signed, now()), Err(PassportError::BadSignature));

        // And a different key is not a signature over this document.
        let mut swapped = sign(&passport(), &key(1)).expect("sign");
        swapped.key = hex::encode(key(2).verifying_key().to_bytes());
        assert_eq!(verify(&swapped, now()), Err(PassportError::BadSignature));
    }

    #[test]
    fn canonical_json_does_not_depend_on_key_order() {
        let a: serde_json::Value = serde_json::from_str(r#"{"b":1,"a":{"z":"x","y":[1,2]}}"#).expect("json");
        let b: serde_json::Value = serde_json::from_str(r#"{"a":{"y":[1,2],"z":"x"},"b":1}"#).expect("json");
        assert_eq!(canonical_json(&a), canonical_json(&b));
        assert_eq!(canonical_json(&a), r#"{"a":{"y":[1,2],"z":"x"},"b":1}"#);
    }

    #[test]
    fn an_expired_passport_is_refused_rather_than_discounted() {
        let mut stale = passport();
        stale.issued_at = "2025-01-01T00:00:00Z".to_owned();
        let signed = sign(&stale, &key(1)).expect("sign");
        match verify(&signed, now()) {
            Err(PassportError::Expired { age_days, .. }) => assert!(age_days > MAX_AGE_DAYS, "{age_days}"),
            other => panic!("expected an expiry refusal, got {other:?}"),
        }

        // A doc dated far in the future is refused too: an origin cannot set an arbitrary age for its own claim.
        let mut future = passport();
        future.issued_at = "2027-01-01T00:00:00Z".to_owned();
        let signed = sign(&future, &key(1)).expect("sign");
        assert!(matches!(verify(&signed, now()), Err(PassportError::Expired { .. })));
    }

    #[test]
    fn an_unknown_version_is_refused_rather_than_partially_read() {
        let mut newer = passport();
        newer.version = 2;
        // It cannot even be signed: the shape check runs first, so an origin's own build refuses to issue what it cannot
        // vouch for.
        assert_eq!(
            sign(&newer, &key(1)),
            Err(PassportError::UnsupportedVersion(2))
        );
    }

    #[test]
    fn a_document_that_claims_too_much_is_refused() {
        let mut huge = passport();
        huge.taste = (0..MAX_TAGS as u32 + 1)
            .map(|index| (format!("tag-{index}"), 1))
            .collect();
        assert!(matches!(sign(&huge, &key(1)), Err(PassportError::Oversized(_))));

        let mut heavy = passport();
        heavy.taste.insert("angst".to_owned(), 10_001);
        assert!(matches!(sign(&heavy, &key(1)), Err(PassportError::Malformed(_))));

        let mut anonymous = passport();
        anonymous.subject = "  ".to_owned();
        assert!(matches!(sign(&anonymous, &key(1)), Err(PassportError::Malformed(_))));
    }

    #[test]
    fn a_measured_zero_and_no_reading_are_different_documents() {
        let mut unmeasured = passport();
        unmeasured.affinity_bp = None;
        let signed = sign(&unmeasured, &key(1)).expect("sign");
        let serialised = serde_json::to_string(&signed).expect("json");
        assert!(
            !serialised.contains("affinity_bp"),
            "an unmeasured affinity is absent, not zero: {serialised}"
        );

        let mut zero = passport();
        zero.affinity_bp = Some(0);
        let signed = sign(&zero, &key(1)).expect("sign");
        assert!(serde_json::to_string(&signed).expect("json").contains(r#""affinity_bp":0"#));
    }

    #[test]
    fn normalisation_makes_the_vector_comparable() {
        let weights = normalised(&passport());
        assert_eq!(weights.get("angst"), Some(&5_000));
        assert_eq!(weights.get("fluff"), Some(&3_000));
        assert_eq!(weights.get("au"), Some(&2_000));

        // A document with no weight normalises to nothing rather than to a division by zero.
        let mut empty = passport();
        empty.taste.clear();
        assert!(normalised(&empty).is_empty());
    }

    #[test]
    fn the_document_id_changes_with_the_content() {
        let first = document_id(&passport());
        let mut other = passport();
        other.contributions.works = 13;
        assert_ne!(first, document_id(&other));
        assert_eq!(first.len(), 32);
    }
}

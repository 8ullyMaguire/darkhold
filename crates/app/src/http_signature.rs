//! HTTP Signatures on the ActivityPub inbox (draft-cavage-http-signatures, `rsa-sha256`).
//!
//! # Why this is its own module
//!
//! An inbox that accepts anything is not an inbox, it is a public write endpoint with a federation-shaped path. Until this
//! module existed `POST /federation/inbox` stored whatever arrived and answered `received`, and the intake ledger recorded the
//! peer's *claim* of identity (a host parsed out of a string anybody can type). The ledger said `quarantined` for every row,
//! which was honest and useless: nothing could ever move a row out except an operator's opinion.
//!
//! # The rules this enforces, and why each one
//!
//! * **`(request-target)`, `host`, `date` and `digest` must all be covered.** A signature over a subset is a signature over
//!   the parts a proxy or an attacker can leave alone: without `digest` the body can be swapped, without `date` a captured
//!   request replays forever, without `(request-target)` it is valid on every path on the host.
//! * **The date must be within [`MAX_CLOCK_SKEW`].** A signature with no freshness bound is a bearer token with no expiry.
//! * **The `Digest` header must match the body we received.** The signature covers the header; only the hash ties the header to
//!   the bytes.
//! * **A key is fetched once and cached.** Fetching per request is a request amplification vector: one post costs the peer
//!   host a key document read, and a peer can be made to serve that on our behalf by anyone who can reach us.
//!
//! # What this does **not** do
//!
//! It does not check that the key belongs to the actor claiming it beyond the actor document itself, and it does not follow
//! `keyId` to a separate key document when the actor document names a different `publicKey.id`. Both are the same trust
//! assumption every ActivityPub implementation makes — the actor document is fetched over TLS from the host that signed — and
//! neither is a substitute for it. It also does not implement `hs2019` with Ed25519: a peer using it is refused by algorithm
//! name rather than by a wrong verification, so the refusal is reportable.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use base64::Engine as _;
use rsa::pkcs1v15::{Signature as RsaSignature, VerifyingKey};
use rsa::pkcs8::DecodePublicKey;
use rsa::signature::Verifier;
use rsa::RsaPublicKey;
use sha2::{Digest as _, Sha256};

/// How far a request's `Date` may be from ours. Five minutes: long enough for a peer with a sloppy clock, short enough that a
/// captured request is not reusable tomorrow.
pub const MAX_CLOCK_SKEW: i64 = 300;

/// Headers a signature must cover for this module to consider it a signature over the request rather than over a coincidence.
pub const REQUIRED_COVERED: [&str; 4] = ["(request-target)", "host", "date", "digest"];

/// The parsed `Signature` header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureHeader {
    pub key_id: String,
    pub algorithm: String,
    pub headers: Vec<String>,
    /// The signature bytes.
    pub signature: Vec<u8>,
}

impl SignatureHeader {
    /// Parse `keyId="...",algorithm="rsa-sha256",headers="...",signature="..."`.
    ///
    /// Deliberately strict: an unparseable header is a refusal, not a request that skips verification. A lenient parser here
    /// would be the whole vulnerability — every malformed header would fall through to the unverified path.
    pub fn parse(raw: &str) -> Result<Self, String> {
        let mut fields: HashMap<String, String> = HashMap::new();
        for part in split_params(raw) {
            let Some((name, value)) = part.split_once('=') else {
                return Err(format!("unparsed parameter: {part}"));
            };
            let value = value.trim().trim_matches('"');
            fields.insert(name.trim().to_ascii_lowercase(), value.to_owned());
        }
        let key_id = fields.remove("keyid").ok_or("no keyId")?;
        let algorithm = fields.remove("algorithm").ok_or("no algorithm")?;
        let headers = fields
            .remove("headers")
            .ok_or("no headers")?
            .split_whitespace()
            .map(|header| header.to_ascii_lowercase())
            .collect();
        let signature_raw = fields.remove("signature").ok_or("no signature")?;
        let signature = base64::engine::general_purpose::STANDARD
            .decode(signature_raw.as_bytes())
            .map_err(|error| format!("signature is not base64: {error}"))?;
        Ok(Self {
            key_id,
            algorithm,
            headers,
            signature,
        })
    }

    /// The covered headers, or a sentence naming the first the request cannot be verified without.
    pub fn covered(&self) -> Result<&[String], String> {
        for required in REQUIRED_COVERED {
            if !self.headers.iter().any(|header| header == required) {
                return Err(format!(
                    "the signature does not cover {required}; a signature over a subset is a signature over what a proxy can leave alone"
                ));
            }
        }
        Ok(&self.headers)
    }
}

/// Split on commas that are not inside quotes.
fn split_params(raw: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for ch in raw.chars() {
        match ch {
            '"' => {
                quoted = !quoted;
                current.push(ch);
            }
            ',' if !quoted => {
                parts.push(std::mem::take(&mut current));
            }
            _ => current.push(ch),
        }
    }
    if !current.trim().is_empty() {
        parts.push(current);
    }
    parts
        .into_iter()
        .map(|part| part.trim().to_owned())
        .filter(|part| !part.is_empty())
        .collect()
}

/// The `Digest` header value for a body: `SHA-256=<base64>`.
#[must_use]
pub fn digest_of(body: &[u8]) -> String {
    let hash = Sha256::digest(body);
    format!(
        "SHA-256={}",
        base64::engine::general_purpose::STANDARD.encode(hash)
    )
}

/// Whether the request's `Digest` header carries a `SHA-256` value that matches the body it arrived with.
///
/// A `Digest` header may list several algorithms (RFC 3230), so the value is split on commas and each entry compared **whole** —
/// `SHA-256=<hash>` must equal an entry, not appear inside one. The signature covers this header, so a peer that lists extra
/// algorithms is describing what it sent; one that lists a *wrong* SHA-256 cannot pass by burying it beside a right one, because
/// no entry would match.
pub fn digest_matches(header: &str, body: &[u8]) -> Result<(), String> {
    let expected = digest_of(body);
    let mut seen = false;
    for candidate in header.split(',') {
        let candidate = candidate.trim();
        if candidate.eq_ignore_ascii_case(&expected) {
            seen = true;
            break;
        }
    }
    if seen {
        Ok(())
    } else {
        Err(format!(
            "Digest does not match the body that arrived (expected {expected})"
        ))
    }
}

/// The exact string the peer signed, from the headers they say they covered.
pub fn signing_string(
    method: &str,
    target: &str,
    headers: &axum::http::HeaderMap,
    covered: &[String],
) -> Result<String, String> {
    let mut lines = Vec::with_capacity(covered.len());
    for name in covered {
        let value = if name == "(request-target)" {
            format!("{} {}", method.to_ascii_lowercase(), target)
        } else {
            let value = headers.get(name.as_str()).ok_or_else(|| {
                format!("the signature covers {name}, which the request does not carry")
            })?;
            value
                .to_str()
                .map_err(|_| format!("{name} is not valid UTF-8"))?
                .to_owned()
        };
        lines.push(format!("{name}: {value}"));
    }
    Ok(lines.join("\n"))
}

/// Whether `date` is within [`MAX_CLOCK_SKEW`] of now.
pub fn fresh(date: &str) -> Result<(), String> {
    let parsed = time::OffsetDateTime::parse(date, &time::format_description::well_known::Rfc2822)
        .or_else(|_| {
            time::OffsetDateTime::parse(date, &time::format_description::well_known::Rfc3339)
        })
        .map_err(|_| format!("Date is not a date this build can read: {date}"))?;
    let skew = (time::OffsetDateTime::now_utc() - parsed).whole_seconds();
    if skew.abs() > MAX_CLOCK_SKEW {
        return Err(format!(
            "Date is {skew} seconds from now, outside the {MAX_CLOCK_SKEW}-second window"
        ));
    }
    Ok(())
}

/// Verify `signature` over `signing_string` with an RSA public key in PEM form.
pub fn verify_rsa(
    public_key_pem: &str,
    signing_string: &str,
    signature: &[u8],
) -> Result<(), String> {
    let key = RsaPublicKey::from_public_key_pem(public_key_pem)
        .map_err(|error| format!("the peer's public key is not readable: {error}"))?;
    let verifying = VerifyingKey::<Sha256>::new(key);
    let signature = RsaSignature::try_from(signature)
        .map_err(|error| format!("the signature is not an RSA signature: {error}"))?;
    verifying
        .verify(signing_string.as_bytes(), &signature)
        .map_err(|_| "the signature does not verify against the peer's key".to_owned())
}

/// Peer signing keys, fetched at most once each.
///
/// Cloneable and shared: one cache per instance, not one per request. A fetch failure is **not** cached — a peer whose host was
/// briefly down would otherwise be unverifiable until the process restarted, and every retry they make would be a request we
/// refuse for a reason that has expired.
#[derive(Clone)]
pub struct KeyCache {
    client: reqwest::Client,
    keys: Arc<RwLock<HashMap<String, Arc<String>>>>,
}

impl KeyCache {
    #[must_use]
    pub fn new(client: reqwest::Client) -> Self {
        Self {
            client,
            keys: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Insert a key directly, under the `keyId` a peer will name in its `Signature` header. Used by tests, and by an operator
    /// "trust this key" door when one exists.
    pub fn insert(&self, key_id: &str, pem: &str) {
        if let Ok(mut keys) = self.keys.write() {
            keys.insert(key_id.to_owned(), Arc::new(pem.to_owned()));
        }
    }

    /// How many keys are held. Reported so a test can prove a fetch happened once rather than per request.
    #[must_use]
    pub fn len(&self) -> usize {
        self.keys.read().map(|keys| keys.len()).unwrap_or(0)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The actor's `publicKey.publicKeyPem`, from the cache or from the actor document.
    ///
    /// The key is cached under the **`keyId`** the peer sent, which is normally `https://host/users/name#main-key`, and the
    /// document is fetched from the URL without the fragment: a fragment is not sent to a server, and asking for
    /// `.../name#main-key` sends `.../name` while the cache key keeps the distinction between two keys of one actor.
    pub async fn public_key(&self, key_id: &str) -> Result<Arc<String>, String> {
        if let Ok(keys) = self.keys.read() {
            if let Some(pem) = keys.get(key_id) {
                return Ok(pem.clone());
            }
        }
        let document_url = key_id.split('#').next().unwrap_or(key_id);
        let response = self
            .client
            .get(document_url)
            .header(
                reqwest::header::ACCEPT,
                "application/activity+json, application/ld+json",
            )
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .map_err(|error| format!("the actor document could not be fetched: {error}"))?;
        if !response.status().is_success() {
            return Err(format!("the actor document answered {}", response.status()));
        }
        let document: serde_json::Value = response
            .json()
            .await
            .map_err(|error| format!("the actor document is not JSON: {error}"))?;
        let pem = document
            .get("publicKey")
            .and_then(|key| key.get("publicKeyPem"))
            .and_then(|pem| pem.as_str())
            .ok_or("the actor document carries no publicKey.publicKeyPem")?;
        let pem = Arc::new(pem.to_owned());
        if let Ok(mut keys) = self.keys.write() {
            keys.insert(key_id.to_owned(), pem.clone());
        }
        Ok(pem)
    }
}

/// The outcome of checking a request's signature: what was verified, or the sentence explaining what was not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// The request carried a signature and it verified. Carries the key id.
    Verified(String),
    /// The request carried no signature.
    Unsigned,
    /// The request carried a signature that did not verify, with the reason.
    Refused(String),
}

/// Check an inbox request's signature, fetching the peer's key when the cache does not hold it.
///
/// `target` is the path **and** query, as the peer sent it: `(request-target)` is `post /federation/inbox`, and a signature that
/// covered the query has to be checked against the same string or a valid peer is refused for having a query string.
pub async fn check(
    keys: &KeyCache,
    method: &str,
    target: &str,
    headers: &axum::http::HeaderMap,
    body: &[u8],
) -> Verdict {
    let Some(raw) = headers
        .get("signature")
        .and_then(|value| value.to_str().ok())
    else {
        return Verdict::Unsigned;
    };
    let Ok(parsed) = SignatureHeader::parse(raw) else {
        // A header that cannot be parsed is refused, never treated as absent: "absent" is a supported answer, and a malformed
        // one must not fall through to it.
        return Verdict::Refused("the Signature header could not be parsed".to_owned());
    };
    if !matches!(parsed.algorithm.as_str(), "rsa-sha256" | "hs2019") {
        return Verdict::Refused(format!(
            "algorithm {} is not one this build verifies",
            parsed.algorithm
        ));
    }
    let covered = match parsed.covered() {
        Ok(covered) => covered,
        Err(reason) => return Verdict::Refused(reason),
    };
    let date = headers
        .get("date")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if let Err(reason) = fresh(date) {
        return Verdict::Refused(reason);
    }
    let digest = headers
        .get("digest")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if let Err(reason) = digest_matches(digest, body) {
        return Verdict::Refused(reason);
    }
    let Ok(signing) = signing_string(method, target, headers, covered) else {
        return Verdict::Refused("a covered header is missing from the request".to_owned());
    };
    let pem = match keys.public_key(&parsed.key_id).await {
        Ok(pem) => pem,
        Err(reason) => return Verdict::Refused(reason),
    };
    match verify_rsa(&pem, &signing, &parsed.signature) {
        Ok(()) => Verdict::Verified(parsed.key_id),
        Err(reason) => Verdict::Refused(reason),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_digest_covers_the_whole_header_value() {
        let body = b"{\"type\":\"Create\"}";
        let digest = digest_of(body);
        assert_eq!(digest_matches(&digest, body), Ok(()));
        // A peer may list several algorithms (RFC 3230); the SHA-256 entry is the one compared, whole.
        assert_eq!(
            digest_matches(&format!("SHA-256=AAAA,{digest}"), body),
            Ok(())
        );
        assert_eq!(
            digest_matches(&format!("UNIXsum=1234,{digest}"), body),
            Ok(())
        );
        // The right hash is not the right hash for *these* bytes.
        assert!(digest_matches(&digest, b"{\"type\":\"Delete\"}").is_err());
        // Nor is a hash that merely contains it as a substring.
        assert!(digest_matches(&format!("{digest}AAAA"), body).is_err());
    }

    #[test]
    fn parse_refuses_a_header_it_cannot_read() {
        assert!(SignatureHeader::parse("garbage").is_err());
        assert!(SignatureHeader::parse(r#"keyId="x",algorithm="rsa-sha256""#).is_err());
        let parsed = SignatureHeader::parse(
            r#"keyId="https://peer.example/u/a#main-key",algorithm="rsa-sha256",headers="(request-target) host date digest",signature="AAAA""#,
        )
        .expect("a well-formed header");
        assert_eq!(parsed.key_id, "https://peer.example/u/a#main-key");
        assert_eq!(parsed.headers.len(), 4);
        assert_eq!(parsed.signature, vec![0, 0, 0]);
    }

    #[test]
    fn a_subset_of_headers_is_refused_by_name() {
        let parsed = SignatureHeader::parse(
            r#"keyId="k",algorithm="rsa-sha256",headers="host date",signature="AAAA""#,
        )
        .expect("parses");
        let reason = parsed.covered().expect_err("a subset is refused");
        assert!(
            reason.contains("(request-target)"),
            "the first missing header is named: {reason}"
        );
    }

    #[test]
    fn a_date_outside_the_window_is_stale_and_one_just_inside_is_not() {
        let now = time::OffsetDateTime::now_utc();
        let inside = (now - time::Duration::seconds(MAX_CLOCK_SKEW - 5))
            .format(&time::format_description::well_known::Rfc2822)
            .expect("format");
        assert_eq!(fresh(&inside), Ok(()));
        let outside = (now - time::Duration::seconds(MAX_CLOCK_SKEW + 60))
            .format(&time::format_description::well_known::Rfc2822)
            .expect("format");
        assert!(fresh(&outside).is_err());
        assert!(fresh("not a date").is_err());
    }
}

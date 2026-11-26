# docs/plans/calibrated-decision-models.md — plan card

Source: `docs/plans/calibrated-decision-models.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

Implements `docs/spec-amendments/calibrated-decision-models.md`. Every step
New file: `crates/decisions/Cargo.toml` and `crates/decisions/src/lib.rs`.
Register in the workspace `Cargo.toml` members.
New file: `crates/decisions/src/reconcile.rs`. This is the load-bearing file of
> **`ImportQuality` has no consumer outside `crates/domain`.**
> `crates/app/src/` never calls `classify_quality` at all — neither the
`crates/domain/src/imports.rs`: `ImportWork::classify_quality` stays exactly as
The import worker is the only caller. `crates/app/src/imports.rs`, where
`crates/app/src/config.rs`, beside `RetentionConfig`:
`migrations/sqlite/0088_decision_audit.sql` and the Postgres twin:
`crates/app/src/routes/meta.rs` gains the provider name and the two
Sep 25 and its `migrations/postgres/` has 72 files where this branch has 87.

## Copy-pasteable code / commands

$ ssh thinkcentre 'curl -s -X POST http://localhost:8888/v1/systemone \
    -H "Authorization: Bearer $(cat ~/.unsloth/systemone.key)" \
    -H "Content-Type: application/json" \
    -d "{\"model\":\"laya\",\"state\":\"charged twice, need a refund today\",
         \"questions\":{\"refund\":{\"type\":\"noul\",
         \"instructions\":\"Does the customer ask for a refund?\"}}}"'
{"model":"laya-multilingual","answers":{"refund":{"type":"noul","noul":0.9965}},
 "usage":{"input_tokens":41,"output_tokens":0}}

[package]
name = "darkhold-decisions"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
reqwest = { workspace = true, default-features = false, features = ["json", "rustls-tls"] }
tokio = { workspace = true, features = ["time"] }

pub struct ClientConfig { pub base_url: String, pub model: String,
                          pub api_key: Option<String>, pub timeout_ms: u64 }

/// The question kinds Unsloth's Decision API accepts.
pub enum Question { YesNo { key: String, instructions: String },
                    Choice { key: String, instructions: String, criteria: BTreeMap<String, String> },
                    Score  { key: String, instructions: String, levels: Vec<String> } }

/// One answer, as the model returns it. The three shapes are distinct because
/// they are distinct, not because a sum type needs variety.
pub enum Answer { YesNo { probability: f64 },
                  Choice { choice: String, probabilities: BTreeMap<String, f64>, confidence: f64 },
                  Score  { score: f64, confidence: f64 } }

pub struct Decision { pub key: String, pub answer: Answer }

#[derive(Debug, thiserror::Error)]
pub enum DecisionError {
    #[error("the decision model at {url} did not answer: {source}")]
    Unreachable { url: String, #[source] source: reqwest::Error },
    #[error("the decision model answered {status}")]
    Status { status: u16 },
    #[error("the decision model answered without {key}")]
    MissingKey { key: String },
    #[error("the model returned {0}, which is not a probability")]
    NotAProbability(f64),
}

impl Client {
    pub fn new(config: ClientConfig) -> Self;
    /// Ask. `state` is a string or any JSON — the API takes either, and which
    /// one is used must not change the answer, so it is always a JSON value.
    pub async fn ask(&self, state: &serde_json::Value,
                     questions: &[Question]) -> Result<Vec<Decision>, DecisionError>;
}

## Verification command and expected output

carries the exact command and its expected output, so the whole plan can be
Expected for the build's own smoke test: `"noul":0.9965`. The exact float is
# expected: no errors
$ cargo test -p darkhold-decisions
# expected: N passed; the tests use a local mock server, never the real model
direction — a test that accepts a wrong monotone behaviour is worse than no
test:
$ cargo test -p darkhold-decisions reconcile
# expected: 8 passed
**Mutation gate — run this before the change is considered done.** Each of
these must make a test fail:
$ cargo test -p darkhold-decisions reconcile   # MUST fail
$ cargo test -p darkhold-decisions reconcile   # MUST fail
$ cargo test -p darkhold-decisions reconcile   # MUST fail
$ cargo test -p darkhold-domain --lib imports
# expected: all pass, including a new test that a disabled provider is a no-op
$ cargo test -p darkhold-app --lib config
# expected: a test that a NaN threshold is refused, not clamped
$ cargo test -p darkhold-db --lib decision
# expected: pass on BOTH backends. The multi-backend comparison is

## RED/GREEN evidence

# expected: N passed; the tests use a local mock server, never the real model
# expected: 8 passed
A test bug worth recording, because the first version of it failed and the
# expected: every suite green, provider deterministic
# expected: green, and the calibrated cases actually consulted the model
#           (assert the posterior is Some, not None — a green run where
# expected: green, every posterior None, and the deterministic answers intact
#            be green for the wrong reason
# expected: green
green because the provider silently did nothing — rather than because the

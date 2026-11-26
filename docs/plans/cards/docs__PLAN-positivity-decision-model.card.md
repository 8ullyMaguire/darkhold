# docs/PLAN-positivity-decision-model.md — plan card

Source: `docs/PLAN-positivity-decision-model.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

Companion: `~/code/rust/ficnexus/docs/PLAN-laya-decision-backend.md` (the same idea applied to a
| `FeedbackClass` | `crates/domain/src/positivity.rs:21` | `Positive` / `Constructive` / `Ambiguous` / `Negative` |
| `Classification` | `crates/domain/src/positivity.rs:53` | `{ class, confidence_bp: i64, signals: Vec<String> }` |
| `classify(text) -> Classification` | `crates/domain/src/positivity.rs:254` | deterministic keyword-marker rule engine |
| `record_classification` | `crates/db/src/positivity.rs:316` | upsert into `review_classifications` |
| `FeedbackPreferences` | `crates/domain/src/positivity.rs:61` | `accept_constructive`, `ambiguous_auto`, `comments_enabled` |
`migrations/sqlite/0010_positivity.sql:57` and `migrations/postgres/0010_positivity.sql:40` both carry
`crates/app/tests/milestone_9.rs`. Nothing in the app calls it.
* `docs/PLAN-laya-decision-backend.md` — the same pattern in ficnexus, where the classifier *is* wired

## Copy-pasteable code / commands

/// Classify one text. Deterministic: same input always yields same class.
pub fn classify(text: &str) -> Classification {
    let folded = text.to_lowercase();
    let trimmed = folded.trim();
    if trimmed.is_empty() {
        return Classification { class: FeedbackClass::Ambiguous, confidence_bp: 3000,
                               signals: vec!["no-signal".to_owned()] };
    }
    let hostile = contains_any(trimmed, HOSTILE_MARKERS);
    if !hostile.is_empty() { /* Negative, confidence_bp: 8500, signals "hostility-pattern" + hits */ }
    /* ... otherwise Positive/Constructive, confidence_bp: 4500 */
}

FeedbackClass::Positive      => Delivered,
FeedbackClass::Constructive  if prefs.accept_constructive => Delivered,
FeedbackClass::Constructive  => Held,
FeedbackClass::Ambiguous     if prefs.ambiguous_auto      => Delivered,
FeedbackClass::Ambiguous     => Held,
FeedbackClass::Negative      => Held,

# the zero-caller claim, re-runnable
rg -n "record_classification" --type rust -g '!target/**' .
# expect: the definition, the row mapper, and one test reference — no call from app code

## Verification command and expected output

a unit test with a fixture so a model swap cannot silently move it.
3. Wire the existing `classify` first, behind whatever gate the spec intended. Measure the class
   test.
# expect: the definition, the row mapper, and one test reference — no call from app code

## RED/GREEN evidence

No RED/GREEN evidence was recorded in the source document.

# M70-13 — the orphan ratchet, 12 functions from the 2026-11-08 run

`orphan_queries::no_new_database_function_is_reachable_only_from_a_test` is red. It scans every `pub fn` in
`crates/db/src` and requires the name to appear somewhere in a **non-test, non-db** source — i.e. some route must name it.
Twelve functions fail that test today. None is from M69-21/M69-22; all are from features delivered earlier
(M16-06/07/11/12/14, M29, M69-13), and every one was measured on 2026-11-08 by grepping call sites in `crates/` and
`crates/db/src`. Two more (`accepted_holder`, `fork_manifest`) had no caller anywhere and were deleted in `85fc1f3b`'s
successor; the list went 14 → 12.

## The three kinds, and what each needs

The test treats "internal" and "dead" as the same thing, and they are not. Classifying them is the work, because the fix
differs per class.

### A — internal helper: the `pub` is wrong, not the function

| function | file | internal caller |
|---|---|---|
| `accepted_holder_anywhere` | `evacuation.rs` | `settle` (the cross-notice fork check) |
| `find_notice` | `evacuation.rs` | `announce`, `close` |
| `find_assignment` | `evacuation.rs` | `settle`, `receipt` |
| `arm_totals` | `experiment_decisions.rs` | `evaluate_surface` |
| `clear_override` | `experiment_decisions.rs` | `evaluate_surface`, `override_for` |
| `install_override` | `experiment_decisions.rs` | `evaluate_surface`, `override_for` |
| `velocity_signals` | `governance.rs` | `recompute_trust` |

Fix: `pub(crate)`, and move whatever database test names them onto the public door that already exists (`receipt`,
`open_notice`, `override_for`, `recompute_trust`). **Do not** delete them: each is a named step inside a behaviour that
works, and deleting the step to satisfy a scanner is the scanner winning.

### B — a writer with no door: the feature is half-wired

| function | file | what is missing |
|---|---|---|
| `assert_tag` | `tag_assertions.rs` | `confirm`, `retract`, `assertions_for_work` are all routed; **nothing lets a reader assert a tag in the first place**. The proposal path is the feature's front door. |
| `seller_net` | `payments.rs` | the operator's payout view: payments and refunds are recorded, and no surface says what a seller is owed. |
| `holds_grant` | `marketplace.rs` | used by `list_my_grants`? No — by nothing in `db`: the capability check exists and no route asks it, so an extension's declared permissions are never actually enforced at a door. |
| `snapshot_day` | `experiment_snapshots.rs` | the daily snapshot has no scheduler; it is written only by tests, so an experiment's history is empty on a live instance. |
| `active_grants` | `marketplace.rs` | `list_my_grants` reads a different query; this one answers "what is live right now" and nothing serves it. |

Fix: a route each, behind the module that owns them, and a test asserting the door's audience. This is the class that
represents missing features rather than untidy visibility — correcting the `pub` on these would be a lie about the product.

## What "wired" has to mean here

A function is wired when a **reader-facing or operator-facing door calls it**, with a test that requests the door over HTTP
and asserts both the answer and the audience. A function called from another `db` function is not wired; a function called
from an app test is not wired (that is what the scan already excludes). That is the same standard
`route_inventory` applies to routes and `reachability` applies to tables.

## Order

B before A. Class B is missing product; class A is a visibility correction, and doing it first would make the list greener
without adding anything a reader can use.

## Correction, and the measurement that replaced it (2026-11-08)

I first wrote here that the scan used a plain `contains`. It did not — it already required an identifier boundary. The real
weakness was the *other* end: it accepted any whole-identifier occurrence, so `name(` was not required and a name in prose
counted. Measured cases:

| function | what satisfied the scan | why that was wrong |
|---|---|---|
| `assertions_for_work` | a doc comment in `tag_inference.rs` | a comment is not a caller |
| `confirm` | a local variable in `erasure.rs` (`if let Some(confirm) = …`) | a binding is not a call |
| `retract` | `cta.rs` declares its own `retract` route handler | same word, unrelated function |

The scan now requires `(` after the name. **That took the list from 11 to 30**, and the nineteen it added are not new debt;
they are debt the old rule was hiding. Each was measured: the name exists in `crates/app` only as a **local variable, a JSON
key, or a route function that shares the word** — never as a call:

| db function | what looks like a caller | what the route actually calls |
|---|---|---|
| `work_mirrors::active_count` | `let active_count = …` in `lending.rs` | `lending::count_active_loans` |
| `storage::usage` | `"usage": []` and `let usage = …` in `economy.rs` | `economy::usage_for` |
| `whole_work::total_words` | `"total_words": lengths.iter().sum()` | computed inline |
| `peer_fingerprints::forget` | the word in three route files | nothing |

That pattern — a db function written for a feature, then the route computing the same thing inline next to it — is the most
common shape in the nineteen, and it is a drift risk rather than a tidy-up: two implementations of one number, one tested and
one served.

### What the scan still cannot see

`name(` matches a call *and a declaration*, so a route that declares its own `fn retract` still satisfies `retract`. Closing
that needs module paths (`darkhold_db::<mod>::retract(`) plus the caller's `use` imports; the scan is a ratchet, and the
triage file is what makes its over-reporting safe.

## Class A executed (2026-11-08)

Seven internal helpers were candidates; four took `pub(crate)` with no call-site change, because every caller was already inside
`crates/db`: `accepted_holder_anywhere`, `find_assignment` (evacuation), `arm_totals`, `clear_override` (experiment decisions).

Three are held back, and the reason is worth stating because it is the ratchet working: an **integration test in `crates/db/tests`
or `crates/app/tests` is a separate crate, so it sees only `pub`**. `find_notice` (m69_07), `install_override` (m29) and
`velocity_signals` (trust_velocity) are each read by a test and by nothing else. Narrowing them to `pub(crate)` would only move the
problem into a compile error, and the honest fix is the one this file already prescribed: **give the behaviour a door, then assert
the door**. `velocity_signals` is the cheapest of the three — `recompute_trust` already records the appraisal it computes into the
trust event's `velocity_basis`, so its two test assertions can be made about that stored basis, which is the reader-visible fact.

## Also reversed: `[discovery] explanations`

The previous session put the reason chip and `match_percent` behind a default-off switch, on the argument that a reason chip is an
influence disclosure (§16.3, §20). Three M29 tests disagreed, and they are right: the handler is the **served** path — it records the
slot it is about to hand back — and §29.2 requires a served recommendation to carry its reason and its relative score. Hiding them
made the served row less accountable than the explanation door that describes it. The switch is gone from `config.rs` and
`docs/config-reference.md`; the fields are unconditional again. The §16.3 concern stays answered where it belongs — in what the
sentence may *say* (the closed vocabulary) and in the fact that a reader asked for the recommendation at all.

## Second pass: 30 -> 17

Wired rather than narrowed, because each had a reader who should have seen it:

| function | what the door now does |
|---|---|
| `work_mirrors::active_count` | `/works/{id}/mirrors` asks the store instead of counting the rows it just read |
| `whole_work::total_words` | the chapter list asks the store instead of summing its own spans |
| `source_engagement::likes_per_view` | `/metrics/{id}` serves the ratio; the only other copy was a doc example dividing by zero |
| `directory::my_vote` | the vote response reads the row back instead of echoing the request |
| `reasons_store::reason_bearing_kudos` | the work page reports `kudos` and `kudos_with_reason` |
| `peer_fingerprints::forget` | new `DELETE /operator/peering/fingerprints/{instance}` |
| `experiment_decisions::install_override`, `clear_override` | **a real defect**: `/operator/experiments/{surface}/promote` recorded the decision and never installed the override, while its own doc comment said it did. An operator's promotion changed nothing and the feed kept serving the configured set |

Narrowed to `pub(crate)` after their last external reader moved: `velocity_signals` (the test now asserts the stored basis `recompute_trust` writes), `find_notice` (the app test's assertion was already made twice over the door).

## The measurement, after the scan learned to follow calls (2026-11-08)

The scan now builds a call graph inside `crates/db` and propagates reachability from the names an app source calls. That took
the number from 292 to **233**, and none of the 59 were work: every one was an internal helper reached through another
function. The row stays `planned` until 233 is zero — the requirement's own words are "no function exists only to satisfy its
own test".

**183 functions across 70 modules** (233 before `work_characters`, 216 before `preservation`, 199 before `subscriptions`). The largest:

| module | orphans |
|---|---|
| `work_characters.rs` | 17 |
| `preservation.rs` | 14 |
| `subscriptions.rs` | 10 |
| `external.rs` | 9 |
| `work_coordinates.rs` | 9 |
| `federation.rs` | 8 |
| `taste_leakage.rs` | 8 |
| `exports.rs` | 7 |
| `library.rs` | 7 |
| `imports.rs` | 6 |
| `story_identity.rs` | 6 |
| `canon_agnostic.rs` | 5 |
| `category_governance.rs` | 5 |
| `community.rs` | 5 |
| `exchange.rs` | 5 |
| `identity.rs` | 5 |
| `rating_integrity.rs` | 5 |
| `taste_health.rs` | 5 |
| `taste_import.rs` | 5 |
| `translation.rs` | 5 |

### Order of work, and why

1. **Modules whose feature has a door already** — the write half usually follows the read half. `work_characters` was 17 and
   is done: `PUT/DELETE /works/{id}/characters/{node}`, `/roles/{role}`, `/attributes/{attribute}`,
   `PUT /works/{id}/relationships/{node}` (the pairing in one write, ends and dynamics together), `/dynamics/{dynamic}`,
   plus `GET /roles`, `GET /dynamics` and `GET /characters/{node}/relationships`. Owner-scoped, and a stranger gets `404`.
2. **Modules with no route at all.** These are features that were built to the store and never surfaced. Each one is a
   decision, not a chore: build the door (and the ledger row it belongs to) or delete the store code and its tests. Deleting is
   allowed and is sometimes right — a function kept because it has a test is the defect the row was opened on — but the
   decision has to be recorded with the module, not taken in passing.
3. **Infrastructure that is genuinely not a reader surface** (`Database::connect`-style constructors and the `BlobStore`
   primitives) — the honest fix is a route that uses them or a narrower visibility, never an allowlist entry.

Every line in the baseline is a promise to come back to it. The list can only shrink.


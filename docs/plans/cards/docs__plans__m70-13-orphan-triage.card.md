# docs/plans/m70-13-orphan-triage.md — plan card

Source: `docs/plans/m70-13-orphan-triage.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

`crates/db/src` and requires the name to appear somewhere in a **non-test, non-db** source — i.e. some route must name it.
(M16-06/07/11/12/14, M29, M69-13), and every one was measured on 2026-11-08 by grepping call sites in `crates/` and
`crates/db/src`. Two more (`accepted_holder`, `fork_manifest`) had no caller anywhere and were deleted in `85fc1f3b`'s
| `accepted_holder_anywhere` | `evacuation.rs` | `settle` (the cross-notice fork check) |
| `find_notice` | `evacuation.rs` | `announce`, `close` |
| `find_assignment` | `evacuation.rs` | `settle`, `receipt` |
| `arm_totals` | `experiment_decisions.rs` | `evaluate_surface` |
| `clear_override` | `experiment_decisions.rs` | `evaluate_surface`, `override_for` |
| `install_override` | `experiment_decisions.rs` | `evaluate_surface`, `override_for` |
| `velocity_signals` | `governance.rs` | `recompute_trust` |
| `assert_tag` | `tag_assertions.rs` | `confirm`, `retract`, `assertions_for_work` are all routed; **nothing lets a reader assert a tag in the first place**. The proposal path is the feature's front door. |
| `seller_net` | `payments.rs` | the operator's payout view: payments and refunds are recorded, and no surface says what a seller is owed. |

## Copy-pasteable code / commands

No fenced command block was recorded in the source document.

## Verification command and expected output

`crates/db/src` and requires the name to appear somewhere in a **non-test, non-db** source — i.e. some route must name it.
Twelve functions fail that test today. None is from M69-21/M69-22; all are from features delivered earlier
The test treats "internal" and "dead" as the same thing, and they are not. Classifying them is the work, because the fix
Fix: `pub(crate)`, and move whatever database test names them onto the public door that already exists (`receipt`,
| `assert_tag` | `tag_assertions.rs` | `confirm`, `retract`, `assertions_for_work` are all routed; **nothing lets a reader assert a tag in the first place**. The proposal path is the feature's front door. |
Fix: a route each, behind the module that owns them, and a test asserting the door's audience. This is the class that
A function is wired when a **reader-facing or operator-facing door calls it**, with a test that requests the door over HTTP
from an app test is not wired (that is what the scan already excludes). That is the same standard
Three are held back, and the reason is worth stating because it is the ratchet working: an **integration test in `crates/db/tests`
`velocity_signals` (trust_velocity) are each read by a test and by nothing else. Narrowing them to `pub(crate)` would only move the
problem into a compile error, and the honest fix is the one this file already prescribed: **give the behaviour a door, then assert
trust event's `velocity_basis`, so its two test assertions can be made about that stored basis, which is the reader-visible fact.
## Second pass: 30 -> 17
Narrowed to `pub(crate)` after their last external reader moved: `velocity_signals` (the test now asserts the stored basis `recompute_trust` writes), `find_notice` (the app test's assertion was already made twice over the door).
own test".
   allowed and is sometimes right — a function kept because it has a test is the defect the row was opened on — but the

## RED/GREEN evidence

`orphan_queries::no_new_database_function_is_reachable_only_from_a_test` is red. It scans every `pub fn` in

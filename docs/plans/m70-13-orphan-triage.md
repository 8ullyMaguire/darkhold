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

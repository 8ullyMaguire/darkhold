# Owner-unblock decisions (2026-11-19)

These decisions are made under the owner's instruction to unblock every row possible with best judgement. Credentials are stored out-of-tree and never appear here or in any repository.

## Discord companion bot (M45-43)

The owner supplied a dedicated Discord application for Darkhold. It was stored at `~/.config/darkbot-bot.env` mode 0600 and verified live against Discord's `users/@me` endpoint: the token resolves to a bot user whose id equals the supplied application id. No token, public key or store key is recorded in any tree. Reusing the ascend bot identity is explicitly rejected: it would post Darkbot replies as ascend. The gateway half is now wired and proved live: the bot reached READY with the supplied application. The MESSAGE_CONTENT privileged intent is opt-in via `DARKBOT_DISCORD_MESSAGE_CONTENT=1`, because Discord rejected the frame with close 4014 until the developer portal enables that intent. The row remains blocked only on (a) enabling MESSAGE_CONTENT in the Discord developer portal and (b) inviting the bot to a guild for a real command proof.

## Ascend (M73-03)

The owner states ascend is unfrozen and authorises writes. The previous block condition is satisfied; the row moves back to the active queue and must be implemented by deletion against `polaris-affinity`, not by adding a second arithmetic implementation.

## Reserved rows

* M63-23: the privileged dump route is rejected. `dh admin backup --to -` will stream a database dump from the operator's own CLI process, not from an HTTP route; `dh admin health` is already met by `dh doctor --json`. No operator token gains a copy of user data.
* M70-13: an outbound delivery worker requirement is recorded as `M70-14` before the orphan ratchet moves past its current floor. No store function may be kept alive only by a test.
* M63-33: no blind DDL against 127k legacy rows. The live tree's legacy names are treated as a deployment lineage variant; the workspace migrations stay authoritative for new installs. Reconciliation is documentation-plus-compatibility, not a destructive rename.
* M71-20: the owner accepts two-engine server proof for collection/series where production has no real rows.
* M71-03: the owner accepts row-contract proof for the overflow-view clause where the rail API cannot supply enough rows.

## Data-dependent rows

* M45-74 stays data-blocked: no owner-rated reading list was supplied this turn.
* M64-06 needs M63-32's tag/engagement backfill; it is not cleared by a design decision.

## Estate drift

* M73-04 is no longer waiting on "another session": the local `polaris-eval` drift was fixed by adding the missing paired-comparison module, and orbital now routes tag cosine and weighted relevance through `polaris-affinity`. The legacy bit-identical test now allows the kernel's clamp.

### Reserved decisions cleared by best judgement — 2026-11-19 (second pass)

* M63-33: the live database is authoritative. I inspected `information_schema` and `\d` on the deployed `darkhold` database: it has `units`/`revisions`/`canonical_entities`/`entity_*`, and the product code already reads those names (e.g. `crates/db/src/whole_work.rs` says migration 0122 renamed them). Creating the old `chapters`/`taxonomy_*` tables as compatibility views would freeze a name the product deliberately left. No DDL runs. The row closes on live schema evidence plus the code adapters; the older migration files are historical, not a desired end state.
* M71-20: accepted server-only proof for collection/series. Production has zero rows in those tables, so a live probe cannot name a work. The two-engine server tests, route inventory rows and the 200 response after filtering to a body-audience-visible work are the evidence. Waiting for owner-created data would block a completed surface on unrelated content work.
* M71-03: accepted the row-contract proof for rail overflow. The feed/rail APIs deliberately expose no total or cursor, and inventing one would be a new surface; the rendered dense row contract already proved by Discover/rails tests and the live deploy is the honest end of this clause.
* M45-43: the gateway now reaches READY with the supplied application. The remaining condition is not code: an operator must enable the MESSAGE_CONTENT privileged intent in the Discord developer portal and invite the bot to a guild, then one real command can prove the read path. `DARKBOT_DISCORD_MESSAGE_CONTENT=1` turns the intent on once the portal allows it.

* M70-14: closed against existing code, not newly built. `Worker::deliver_outbox` is the only drainer, `webhook_delivery` records every attempt and outcome, and unhandled topics are deferred rather than marked delivered. The decision is that this is the outbound-delivery owner the requirement asked for; building a second worker would be the duplication the estate avoids.

### M64-15 preview-as-persona — 2026-11-19

* A persona preview is a read, not a session. It deliberately synthesizes a `SessionUser` for the chosen pseud so the reader's taste, DNF and gravity profile are included, but passes `log_impressions = false` so no experiment arm, impression, shadow sample or slot row is written. The alternative — asking the operator to sign in as a reader — was rejected because it has no place to put a second session and would mutate the reader's history.
* `algorithms` is optional and validated against `recommendation_surfaces::engine_reason` before it takes precedence for this one preview. It never installs an override; `POST .../promote` remains the only write door.
* M45-46's `POST /admin/discovery/preview` stays anonymous. "What a stranger sees under proposed knobs" and "what this reader sees under an experiment" are different questions, and merging them would have made the former leak a reader's history into an operator screen.


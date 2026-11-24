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

## Owner decisions — 2026-11-20 unblock batch

* **M63-25:** the promise is renamed to “what your account did” and the row closes against existing code. `audit_log` has no token id and historical attribution cannot be reconstructed; adding a column without backfill would not satisfy the old wording. No DDL.
* **M64-14:** use a local Ollama embeddings endpoint (`http://localhost:11434/v1/embeddings`), preferably `qwen3-embedding:0.6b` (~500 MB) or `nomic-embed-text` (~300 MB) if memory is tight. The provider adapter must report which model was used and still degrade to disabled when the endpoint is unreachable. No fake “AI” hashing adapter.
* **M63-32:** the owner authorizes network re-fetch of source pages and the pawchive like-count import. This is data work, not a design decision; it can proceed when the row is taken.
* **M45-74:** the owner accepts the private sidecar census under the pre-declared rule (≥180 exact, ≤20 ambiguous, ≥180 applied, ≥1 overlap with top-10 personal recommendations). No values from the census are recorded in the repository.
* **M69-24:** the owner leaves the shared-crate handling to best judgement. I will not stash another session's changes in `polaris-taste-source`; the row stays blocked until that checkout is cleaned by its owner. The separate `>=8 GB` deploy gate is removed as an owner decision. Deploy risk remains recorded, but no memory threshold gates it.
* **TA2:** the TA1 20k ceiling and 100-work pilot gate are superseded. One Pawchive BFS seed at `https://pawchive.pw/patreon/user/13212571/recommended`, depth 3, is enough. Pawchive posts that are chapters of one story collapse into one library work; Pawchive admits only updates older than 120 days; AO3/QQ/SV/SB and other supported archives are checked daily with no 120-day filter. Bodies are archived for works with at least one liked tag and no excluded tag; the global 20k limit is removed. Adapter coverage grows one fixture-tested source at a time. Deploy once after TA2 code, not twice.
* **TA1:** the cross-archive archive is a Darkhold capability, not a second AO3 project. Metadata discovery is metadata-only via a per-job `body_policy: skip`; private ranked works get `body_policy: store`. Bodies stay in the existing blob store attached to `library_items`, not reader-facing schema. A private 0600 preference file carries the explicit AO3 search terms; the local reading list and browser bookmarks are read-only low-weight sources, never printed, committed or sent over the network; no preference value is published. A 100-work pilot gates the 20k run and shortfall is reported instead of padded.

## Implementation decisions — 2026-11-21 goal loop

* **TA3-3:** AO3 refetch now takes a bounded `--offset` and skips works that already have source-derived tags. Re-reading tagged rows counted `works_tagged` without increasing coverage, so that was a false gate. Recorded live coverage stayed at 8,669 work-tag rows while AO3 returned HTTP 429 on direct public probes; this is a rate-limit blocker, not a reason to add auth, bypass, or retries. The server binary was not rebuilt because the source changes were CLI-only; live meta correctly remains `0.1.0+79f1de8c`.
* **TA3-2:** invalid `qq` EPUB import jobs were removed alongside the earlier `archive` ones. The remaining queued jobs are public HTTP(S) imports on supported adapters and are left to the single worker; throughput scaling is not silently changed in a docs/state pass.

## Owner decisions — 2026-11-21 TA3 unblock batch

* **TA3-2:** the owner authorizes raising worker concurrency, with a hard limit of one worker per source at a time. Extra workers add queue throughput, never extra requests against one archive. Implemented as a claim-time guard in `jobs::claim_next`, not as per-site rate tuning.
* **TA3-3:** wait and retry AO3 later. No public cache/mirror was supplied and no bypass or auth is permitted.
* **TA3-4:** fixture TheForce first. XenForo is already a known adapter family, so a recorded public page and a robots/body verdict are the next step.
* **TA3-B1:** keep targeted-only. The full estate gate stays blocked by explicit owner choice.
* **TA3-B2/B3/B4:** leave all parked. No action on the shared checkout, likes/favorites, or global Discord registration this cycle.
* **Diverged remotes:** the local TA3-2 commit was merged with `origin/master`'s taste-fragment commit using `git merge --no-ff`, never a rebase, then pushed to both remotes and fast-forwarded on thinkcentre.

## Implementation decisions — 2026-11-24 Open Shelf deploy

* **Build host:** the release binary is built on thinkcentre, not this host. gaming-pc has glibc 2.44 and thinkcentre 2.39, so a locally linked binary cannot run there. Two alternatives were tried and rejected with evidence: musl cross-compile failed in `aws-lc-sys` assembly (`zig cc` rejects `--target=x86_64-unknown-linux-musl`), and a `rust:1.98-bookworm` container failed because the workspace resolves absolute path dependencies outside the checkout (`/personal/documents/code/polaris-slug`, `/projects/commons/...`).
* **Release profile:** thin LTO (`lto=thin`, `codegen-units=1`) was thrashing at 8–20% CPU for 20–30 minutes per attempt on the overloaded deploy host, with earlyoom SIGTERMs. This deploy uses `CARGO_PROFILE_RELEASE_LTO=off CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16`; same source and behaviour, lower peak memory, more parallel codegen. Restoring the project profile is a later, non-blocking step.
* **Host interference:** `suki-autotag.service` was masked and its orphan workers killed during the build because they restarted in a loop at ~3.5 GB RSS and ~200% CPU. It must be unmasked and restored to its prior state after the build. `ollama` was stopped; temporary swapfiles from the earlier OOM fight are still enabled and are removed only once memory is stable.
* **Scope:** the landing redesign stays server-rendered Rust HTML with a no-script `GET /search` form. No JS-only path, no cover-card grid, no build/health details on the public page.
* **Candidate query:** the first deploy still rendered zero links because every one of the newest 10,000 works by publication event is `accounts_only`, while readable works have no event and sort behind them. `list_open_shelf_candidates` now pre-filters to inherited/`anyone` audiences with a current unit revision; the freshness queue is only a fallback for naming gated works. `reading_decision` remains the sole authority for a link. RED test with 65 newer gated works gave zero links; GREEN suite 5/5; live page gives six links whose reader URL answers 200.


## Deferred host cleanup — 2026-11-24

* **Swapfiles:** the three temporary `/swapfile-build*` files stay enabled. They hold about 7.5 GB of live pages while that host has 6.6 GB RAM free; removing them now would force OOM kills in the tagger and import workers. Unblock is measured swap use below 2 GB with no build or tagger workers, then `swapoff` and delete. This is recorded as row H1, not left as prose.
* **ollama:** left inactive. A start attempt timed out and no owning session has asked for it; starting a model server unrequested would spend memory the deploy host does not have. Unblock is an explicit request or its own timer, recorded as row H2.
* **Thin LTO:** not rebuilt. No source changed after the last deploy, and rebuilding an unchanged binary would violate the loop's no-rebuild rule and risk replacing a working live binary. Unblock is a real code change plus a quiet host, recorded as row H3.
* **State prompt:** `docs/plans/next-goal.md` was rewritten in place because it still named the closed UX1 row. The paragraph now names state files and the per-turn contract only; counts and current rows stay in the roadmap. The SecondBrain mirror carries the same text.

## M45-77 entered the state list — 2026-11-24

* The community trope proposal plan is documentation only. `a3861caa` is in this lineage and pushed, but a source scan found no `trope_proposal`, `proposal_min_trust`, `TropeProposal` or `curator_roles` markers in `crates/`, and `docs/requirements.csv` still marks M45-77.* as `planned`. Recorded as a blocked row, not done.
* Staffing stays the blocker: two eligible accounts, a quorum of two, and a proposer who cannot vote. Lowering the quorum would change the governance the owner chose, so the unblock is granting at least one more curator. A database row without the parser resolver and seven parse-site wirings would be inert; the plan's full implementation remains the gate.

## ollama restored — this turn

* The owner's "complete anything you think relevant" was read as the missing owning-session request for H2. `systemctl start --no-block ollama` was used on the deploy host with a bounded 30-second check; `systemctl is-active ollama` returned `active`. Row H2 moved to Done with that gate output.
* H1 was re-probed and stays parked: 14,871 MB swap used, ~7.4 GB held in the three build swapfiles, suki worker active. Removing them would push live pages into 6.7 GB of free RAM and risk OOM kills. No swap was touched.

## Feature ledger repaired — this turn

* `docs/FEATURE-LEDGER.md` was generated on 2026-11-23 from 978 rows and omitted UX1. Added a `UX1` requirement row with the live gate evidence, then regenerated its count table and header from `docs/requirements.csv` using the command the ledger itself names. No hand-written counts and no new current-state list.

## M45-76.5 — config keys wired first — this turn

* Chose the config row over Phase 1 migration/store work: it was the only unblocked slice with a bounded gate, and it corrected a documentation error I had introduced earlier ("planned, not file-backed"). RED was a compile failure naming the three missing `WorksConfig` fields; GREEN is 16/16 in the touched `config_sections` test file.
* `auto_abandon_after_months = 0` is refused at load with guidance to set `auto_abandon = false`, so disabling the sweep is never confused with a zero-width sweep. This is configuration only; the migration, store sweep, route and live dry run remain M45-76 work and are not claimed shipped.
* The config code is not in the running binary. Deployment is folded into H3: copy the committed source via the normal fast-forward, then rebuild with the project profile only when the deploy host is quiet. No release build or service restart was attempted this turn.

## M45-76.6 — pure decision module — this turn

* Used a separate integration test file as the RED probe because the plan's name filter alone matched zero tests: `cargo test -p darkhold-domain work_lifecycle` filtered out the integration file. The row's gate is therefore two explicit targets, `--lib work_lifecycle` (6 unit tests) and `--test work_lifecycle` (2 integration tests). The plan text says "expected 7"; the module it supplies has 6, so the discrepancy is recorded rather than hidden.
* Mutation check passed: replacing `months * 30` with `months * 31` made the boundary test fail with `left: None, right: Abandon`; the source was restored immediately. No store migration, sweep or route was added; those remain later plan steps. The module is a plan step, not a user-visible feature yet.
* Chose requirement id `M45-76.6` because `M45-76.1` is already the staleness-clock requirement in the CSV; the state row first drafted with that id was renamed before commit.

## M45-76.7 — migration 0218 — this turn

* Added a dedicated `crates/db/tests/migrations.rs` probe because the plan's named `--test migrations` did not exist; the existing `migration_catalogue.rs` only checks catalogue-versus-disk, not that the schema changed. Both are now the row's gate.
* The local PostgreSQL test container had to start and settle: the first run got "Connection reset by peer", a port check showed `5432/tcp -> 127.0.0.1:55433`, and a single retry after five seconds passed. No credential value is recorded; the harness takes `DARKHOLD_TEST_PG_URL` from the environment.
* Existing abandoned rows are seeded from `updated_at`, not a guessed story clock. The migration is append-only; no existing migration was edited, so no checksum changes.

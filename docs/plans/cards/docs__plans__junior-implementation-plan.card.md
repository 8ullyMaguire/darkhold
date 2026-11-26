# docs/plans/junior-implementation-plan.md — plan card

Source: `docs/plans/junior-implementation-plan.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

depth varies — `docs/requirements.csv` is the row-by-row authority, and
`docs/sessions/2026-09-14.md` is the hand-off that explains how to read the
the next work (§15 below). If `docs/verification.md`, `docs/requirements.csv`
> `docs/plans/remaining-work.md`; `docs/handoff.md` is the current-state
> summary; `docs/requirements.csv` is the authority.
- Migration head is `0009_library.sql`, present under both `migrations/sqlite/`
  and `migrations/postgres/` (a test keeps the two sets identical).
- Route modules: `crates/app/src/routes/{auth,pseuds,works,reading,settings,imports,exports,library,jobs,collaborators,health,meta}.rs`,
  wired in `build_router` (`crates/app/src/server.rs`).
- Frontend routes live in `frontend/src/routes/`, registered in
  `frontend/src/lib/router.ts`; linked-but-unbuilt pages render a `Planned`
- `docs/verification.md` holds the evidence for every claim above; `just check`

## Copy-pasteable code / commands

 1. ledger rows       add M<n>-NN rows to docs/requirements.csv (status planned)
 2. migration         BOTH dialects, identical sets (the drift test enforces it)
 3. domain types      crates/domain/src/<module>.rs — policy functions, no I/O
 4. repositories      crates/db/src/<module>.rs — both dialects written out
 5. routes            crates/app/src/routes/<module>.rs, wired in build_router
 6. pages             frontend/src/routes/<Name>.svelte + api.ts + router.ts
 7. hand journey      drive it in a browser against `just serve-dev`
 8. pin it            acceptance tests in crates/app/tests/milestone_<n>.rs
                      + component tests in *.test.ts beside each page
 9. record            verification.md rows with real evidence; flip CSV rows;
                      update docs/plans/README.md's table if scope moved
10. tag               v0.<nn>-<name> per docs/tutorial/README.md

classifications(
  id TEXT PRIMARY KEY,          -- uuid (TEXT + ?::uuid binding, ADR 0004)
  subject_type TEXT NOT NULL,   -- 'comment' | 'review'
  subject_id TEXT NOT NULL,
  class TEXT NOT NULL,          -- 'positive' | 'constructive' | 'other'
  confidence_bp INTEGER NOT NULL,  -- basis points 0..10000 (binds as i64)
  signals TEXT NOT NULL,        -- JSON document, machine-inspectable
  classified_at TEXT NOT NULL   -- RFC 3339
)
index (subject_type, subject_id)

deliveries(
  id TEXT PRIMARY KEY,
  classification_id TEXT NOT NULL,
  author_account TEXT NOT NULL,   -- the author whose preferences apply
  outcome TEXT NOT NULL,          -- 'delivered' | 'held' | 'withdrawn'
  created_at TEXT NOT NULL,
  resolved_at TEXT
)
index (author_account, outcome)

preference_changes(
  id TEXT PRIMARY KEY,
  account TEXT NOT NULL,
  changed_at TEXT NOT NULL,
  document TEXT NOT NULL          -- the preference snapshot, for audits
)

POST /api/v1/works/:id/comments   (existing route gains classification)
POST /api/v1/works/:id/reviews    (existing route gains classification)
GET  /api/v1/me/feedback-preferences
PUT  /api/v1/me/feedback-preferences
GET  /api/v1/me/feedback/inbox    (author-side received feedback)
POST /api/v1/feedback/:deliveryId/withdraw

## Verification command and expected output

re-verify before acting, and update this plan file in the same commit as any
  and `migrations/postgres/` (a test keeps the two sets identical).
  is the gate CI runs.
| Source revision cache unpopulated (`M6-12`) | **M10** — its search reading pass is the natural populator; if not reached, re-register before tagging M10 |
 2. migration         BOTH dialects, identical sets (the drift test enforces it)
                      + component tests in *.test.ts beside each page
  `migrations/postgres/`, kept identical by a test. Add the next number; today
  shipped once. `FieldBinding.test.ts` pins it.
- Run the frontend through `bash frontend/scripts/fe.sh build|test|check`;
- Rust: `cargo test --workspace`. Acceptance tests live in
- Frontend tests: `vitest`, in `*.test.ts` beside what they test.
- A test asserts the **property**, not the implementation: "a stale save writes
- When you find a bug during anything else, fix it and write the test that
  `evidence` column naming a command or test (never a bare file path); a tag
reviews (M16), translation reviews (M17). Spec §12 is the gate they all pass
- `pending_deliveries_for_author(account)` — what the outbox pass renders
  command or a migration data pass). Either is acceptable; an undocumented
   stored** (verify in the DB, not just the UI).
One test per spec §12 acceptance line:
- the sender-visible payload contains no reason field (assert on the JSON)

## RED/GREEN evidence

> snapshot of 2026-09-14 and are now badly stale — the build has passed
when `just check` is green, the CSV rows are flipped **with evidence**, and the
- Every milestone ends with: `just check` green; CSV rows flipped with an
  (`quoted → reserved → submitted → completed|failed`) with the capture rule
  status TEXT NOT NULL,           -- ok|retrying|failed
  failed delivery retries with backoff and then parks as `failed`
                                   -- approved|published|failed|cancelled
- a failed machine job releases the hold and preserves completed units
  state TEXT NOT NULL,             -- pending|processing|done|failed
- `just check` green on the reference box, twice in a row (flakiness check)
  a documented limitation with the number that failed
- `all_existing_tests_pass` — full suite green with default config

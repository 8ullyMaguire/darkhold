# docs/plans/m54-bot-core.md — plan card

Source: `docs/plans/m54-bot-core.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

spec §23.2 (amended, ADR 0024), `docs/goal.md` order-of-work step 2, and the
two rows M54-01/M54-02 in `docs/requirements.csv`.
`docs/plans/bots-extension.md` says this is "proposed, not adopted" and to
ADR 0024, which `docs/spec.md` §23.2 carries verbatim. This plan covers only
`crates/app/src/routes/external.rs:163` takes `MaybeSession(_user)` and passes
`issue_token` (`crates/db/src/external.rs:14`) inserts `(id, account_id, name,
same class as the `rec.mode` defect named in `docs/goal.md`: the machinery is
`migrations/postgres/0020_external.sql:4` declares `token_id TEXT NOT NULL`
`crates/app/src/auth.rs:311` builds the scope list with `.filter_map(|s|
stub `docs/goal.md` forbids: "every branch returns a real value or refuses".
appears in exactly one file in the whole tree: `crates/app/src/routes/media.rs`.**
`RequireToken` both exist in `crates/app/src/auth.rs`, and §23.1's scope check

## Copy-pasteable code / commands

-- link_challenges (spec §23.2).
--
-- The challenge is issued by the BOT and redeemed by the BROWSER. It carries no
-- secret of the reader's: it is a public nonce whose single use is the security
-- property, and the reader's identity arrives with the confirmation, not here.
--
-- expires_at is short (default 600s, configured) precisely so a challenge
-- captured from a chat log cannot be replayed tomorrow. used_at is set exactly
-- once: the redemption is a single UPDATE ... WHERE used_at IS NULL, so two
-- browsers racing on the same challenge produce one token, not two.
--
-- `state` is a value, not an inference from `used_at`/`expires_at`, for the same
-- reason migration 0080 gave `claims` one: an expiry that nobody ever evaluated
-- is the rec.mode defect from docs/goal.md, and a column that can say
-- 'expired' while the predicate that would make it expired is never run is a
-- state no query can rely on.
CREATE TABLE link_challenges (
    code         TEXT PRIMARY KEY,     -- public nonce, high entropy
    state        TEXT NOT NULL,        -- pending|used|expired
    bot_id       TEXT NOT NULL,        -- the bot that asked for it
    requested_scopes TEXT NOT NULL,    -- what the bot asked for, to display
    pseud_id     TEXT,                 -- chosen at confirmation, NULL until then
    token_id     TEXT,                 -- the token minted, NULL until then
    created_at   TEXT NOT NULL,
    expires_at   TEXT NOT NULL,
    used_at      TEXT
);
CREATE INDEX link_challenges_pending ON link_challenges (state, expires_at);

-- api_tokens gains only what is genuinely absent.
--
-- acting_pseud_id is §23.1's "explicit acting pseud": a token belongs to an
-- ACCOUNT but acts as a PSEUD, and without this column every token call acts as
-- the account's default pseud, silently conflating two identities.
--
-- kind is read by issue_token's `_kind` parameter and then thrown away today,
-- so every token in the database is indistinguishable from a personal one
-- however it was issued.
--
-- expires_at and last_used_at are NOT added here. They already exist (0001,
-- both dialects) and are already read by list_tokens; they are simply never
-- written. Re-adding them here would have been the same defect a second time.
ALTER TABLE api_tokens ADD COLUMN acting_pseud_id TEXT;
ALTER TABLE api_tokens ADD COLUMN kind TEXT NOT NULL DEFAULT 'personal';

cargo test -p darkhold-app --test m54_bot_actions   # exit 0, both backends

a_token_with_no_acting_pseud_is_refused_rather_than_defaulted   left: 401  right: 200
a_job_list_is_reachable_by_a_content_write_token               (items, not jobs)
a_job_list_never_shows_another_accounts_jobs                    (same)

## Verification command and expected output

   the workspace. The gate is a `cargo tree` assertion, not a code review.
   long-lived enough to be a bearer secret. The gate is a test that sends a
## The defects found while planning, which M54-02's exit condition depends on
reading the code, not by a test failing, which is the point: nothing here is
covered by a test today.
not a nicety: M54's exit condition is "revoking a token takes effect on the
- **D1–D4 before the bot.** The bot's exit condition is that revocation works.
  means the feature is demonstrably broken and the tests would pass anyway.
  and Matrix to pass *the same* core test suite. Writing an adapter first means
  re-test; discovering it first means one HTTP client contract. This is the
expected output.
The gate is `the_two_dialects_declare_the_same_columns_and_indexes` in
`crates/db/src/migrate.rs:601`, run by `cargo test -p darkhold-db`. It
two files. That test is stricter than this plan first assumed, and two of its
of *why* the parity test is strict — the first attempt there declared a
`_fixed` table name the PostgreSQL twin never mentioned, and the test correctly
Two things the parity test cannot see, verified against `declared_schema` in
  that test — the gate is the PG runtime pass instead, since a column declared
TEXT-typed column references a uuid id. The gate is `RequireActor`'s lookup on
the PG pass, per the INT4-into-i64 blind spot in `docs/goal.md`.

## RED/GREEN evidence

present and the wiring is absent, and a green suite would not see it.
failed on it.
The first run failed **five of them**, and not for the reason the plan
a token test will otherwise write `request_with` and get a green run that proves
This is recorded because the narrowing looked obviously correct, passed the test
That test predates this work and was green; it was poisoning the ones after it

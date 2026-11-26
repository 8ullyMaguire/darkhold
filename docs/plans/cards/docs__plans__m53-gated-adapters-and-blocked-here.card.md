# docs/plans/m53-gated-adapters-and-blocked-here.md — plan card

Source: `docs/plans/m53-gated-adapters-and-blocked-here.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

`implemented-fully-tested`: twelve adapters in `crates/scrapers/src/sites/`, fixtures
under `crates/scrapers/tests/fixtures/<source>/`. Both rows carry `--` for evidence,
**Spec basis.** `docs/spec.md` §11.6 (credential vault), §11.8 (runtime source
health), and the porting rules at `docs/spec.md:1993-1997`:
| `SourceAdapter` trait, `SourceKey`, `Wall` | `crates/scrapers/src/lib.rs:733+` |
| `SourceCapabilities` incl. `authentication: AuthKind` | `crates/scrapers/src/lib.rs:152` |
| `AuthKind { None, Token, Password, SessionCookie }` | `crates/scrapers/src/lib.rs:119` |
| `Registry`, `default_registry`, `can_handle` | `crates/scrapers/src/registry.rs` |
| `sources.health` column + `set_source_health` | `crates/db/src/imports.rs:386` |
| `recompute_source_health` (rolling window, §11.8) | `crates/db/src/imports.rs:437` |
| `settle_source_health` called from the worker | `crates/app/src/imports.rs:219` |
`grep -rln 'verification_status'` over `crates/` both return nothing):

## Copy-pasteable code / commands

-- Per-source credential vault (spec §11.6). Pseud-scoped, encrypted at rest,
-- never returned in plaintext by any endpoint.
CREATE TABLE source_credentials (
    id                TEXT PRIMARY KEY,
    pseud_id          TEXT NOT NULL REFERENCES pseudonyms(id) ON DELETE CASCADE,
    source_key        TEXT NOT NULL,
    kind              TEXT NOT NULL,          -- token | password | session_cookie
    origin_host       TEXT NOT NULL,          -- origin-bound credential use
    ciphertext        BLOB NOT NULL,          -- encrypted at rest
    created_at        TEXT NOT NULL,
    expires_at        TEXT NOT NULL,          -- default 30 days or the source's earlier expiry
    revoked_at        TEXT,                   -- immediate revocation
    last_used_at      TEXT,
    consent_at        TEXT NOT NULL,          -- explicit consent, required for password/cookie
    version           INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX source_credentials_pseud ON source_credentials (pseud_id);
-- No automatic copying across pseuds: one row belongs to one pseud, and the
-- absence of any (pseud, source) uniqueness would invite a "copy to all" write.
CREATE UNIQUE INDEX source_credentials_pseud_source ON source_credentials (pseud_id, source_key);
CREATE INDEX source_credentials_expiry ON source_credentials (expires_at);

GET    /api/v1/source-credentials        -> metadata only, never the secret
POST   /api/v1/source-credentials        -> create; explicit consent required for password/cookie
DELETE /api/v1/source-credentials/:id    -> revoke; does NOT delete imported copies
POST   /api/v1/source-credentials/:id/test -> test the credential, audit without secrets

-- Where this host could not reach a source at all (spec §11.8 + porting rules).
-- NULL means the default: verified or never claimed.
ALTER TABLE sources ADD COLUMN verification_status TEXT;   -- 'blocked-here' | NULL
ALTER TABLE sources ADD COLUMN verification_note TEXT;     -- why, in operator words

## Verification command and expected output

>   (§11.6) and the age gate, and record that in their fixture suite.
| age-gate machinery | exists (M-stage prior); `AuthKind::Password` docs already demand §11.6 consent |
## M53-03 — gated adapters port only behind the credential vault and age gate
constraint. Test that a second insert fails.
### Step 2 — the gate itself, in the scrapers crate
refuses to register one without the gate.
  credential vault *and the age gate*", so a build with the age gate off must not
POST   /api/v1/source-credentials/:id/test -> test the credential, audit without secrets
revoked_at, last_used_at` and **never** `ciphertext`. Test that assertion directly
job payload for a test.
proof of the second sentence — test that N consecutive auth failures produce N
### Step 6 — fixtures record the gate (the row's own test)
age-gated, and was not live-verified. A fixture test asserts the three.
**Do not guess which those are — record it.** The gate in Step 3 makes a wrong
The test must be behavioural, not a restatement: build a registry with one verified
and one blocked adapter, assert `supported == 1 && blocked_here == 1`, and assert
that removing the gate (counting everything) fails that assertion.
- Test: set a source to `blocked-here`, run the recompute, assert `health` is still
  `unknown`. Asserting only that `verification_status` was written would pass
the cookie's host must match the source's `origin_host` — test that a cookie for a

## RED/GREEN evidence

created|used|tested|failed|revoked|renewed|expired`. §11.6 says "audit events

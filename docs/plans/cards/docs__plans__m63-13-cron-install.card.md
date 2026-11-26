# docs/plans/m63-13-cron-install.md — plan card

Source: `docs/plans/m63-13-cron-install.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

> `[[site.jobs]]` in estate (exact declarations in `docs/handoff.md`). `dh cron install` remains for
| `cron` subcommand in `crates/cli/src/main.rs` | **absent** — `Command` has `Rec`, `Q`, `Supply`, … no `Cron` |
| config directory convention in the CLI (`~/.config/darkhold`, `DARKHOLD_CONFIG`) | **absent** — `grep -rn "\.config/darkhold\|config_dir\|DARKHOLD_CONFIG" crates/cli/src` is empty |
| `api_tokens` table | present since `0001_identity.sql:161`, with `scopes TEXT NOT NULL DEFAULT ''`, `expires_at`, `revoked_at` |
| `Scope` enum | `crates/domain/src/api_scopes.rs:5` — `content.read`, `content.write`, `library.read`, `comments.write`, `translation.read`, `translation.write`, `admin.read`, `admin.write` |
| session-vs-token extractors | `RequireSession` (cookie only), `RequireActor` / `RequireActorScoped` (either; a session passes with an empty scope set) — `auth.rs:375-520` |
| routes already accepting either | `routes/exports.rs` (`RequireActorScoped`, three handlers), `routes/external.rs` (`RequireActor`) |
Why this is precedent rather than invention: `exports.rs` and `external.rs` already accept a session or a
1. `crates/domain/src/api_scopes.rs` — no change (`LibraryRead` exists).
2. `crates/app/src/routes/library.rs` — every `/saved-views` handler that takes `RequireSession` takes
3. Test in `crates/app/tests/m63_13_cron.rs`:
New module `crates/cli/src/cron.rs`, new `Command::Cron(CronArgs)` with subcommand `Install`:

## Copy-pasteable code / commands

#[tokio::test]
async fn a_token_with_library_read_opens_the_saved_views_and_one_without_is_refused_by_name() {
    // … register, mint a token with "library.read", GET /saved-views → 200
    // … mint a second token with "content.read" only → 403, body.error.required_scope == "library.read"
    // … and a cookie session → 200, because a session's authority was settled at login
}

dh cron install --saved weekly --every 6h --jitter 10m [--dry-run] [--print]

weekly.timer                 the timer unit
weekly.service               the service unit
weekly.env                   DARKHOLD_TOKEN=…  (0600, refused if it would be readable by others)

## Verification command and expected output

The blast radius is one account's own views: the store is owner-scoped (`M63-06`'s cross-account test is
3. Test in `crates/app/tests/m63_13_cron.rs`:
#[tokio::test]
Mutation: drop the `actor.require(...)` call → the second token gets `200` and the test fails.
   reading the output of the probe rather than its exit code; it now names `POST /api/v1/me/tokens`
cargo test -p darkhold-app --test m63_13_cron
cargo test -p darkhold-cli --bin dh cron::
cargo test -p darkhold-cli
bash scripts/gate.sh all
Mutation proofs: remove `actor.require` (token without the scope gets 200 → test fails); make the unit
test fails); make the env writer `0644` (the permission test fails).
systemd-analyze verify --user ~/.config/darkhold/cron/weekly.timer   → accepts the unit (or names the line it dislikes)
dh cron install --saved weekly --every 6 --jitter 10m              → exit 2, names the accepted shape
`systemd-analyze verify` is the step that separates "the file looks like a unit" from "systemd accepts it".

## RED/GREEN evidence

No RED/GREEN evidence was recorded in the source document.

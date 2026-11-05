# M63-13 — `dh cron install`: a timer a user owns, without a unit file by hand

Status: planned, blocked on nothing. Row `M63-13` (power-user). Idea: sweep P13 — "the instance
already runs timers for fic-catalogue-ingest; this hands the same mechanism to a user".

## What exists, measured

| thing | state |
|---|---|
| `cron` subcommand in `crates/cli/src/main.rs` | **absent** — `Command` has `Rec`, `Q`, `Supply`, … no `Cron` |
| config directory convention in the CLI (`~/.config/darkhold`, `DARKHOLD_CONFIG`) | **absent** — `grep -rn "\.config/darkhold\|config_dir\|DARKHOLD_CONFIG" crates/cli/src` is empty |
| `api_tokens` table | present since `0001_identity.sql:161`, with `scopes TEXT NOT NULL DEFAULT ''`, `expires_at`, `revoked_at` |
| `Scope` enum | `crates/domain/src/api_scopes.rs:5` — `content.read`, `content.write`, `library.read`, `comments.write`, `translation.read`, `translation.write`, `admin.read`, `admin.write` |
| session-vs-token extractors | `RequireSession` (cookie only), `RequireActor` / `RequireActorScoped` (either; a session passes with an empty scope set) — `auth.rs:375-520` |
| routes already accepting either | `routes/exports.rs` (`RequireActorScoped`, three handlers), `routes/external.rs` (`RequireActor`) |
| `/saved-views` | `RequireSession` — cookie only, measured live in `M63-06` (a valid bearer token with a matching hash and an active account gets `401`) |
| `M63-19` (scoped tokens per command family) | `implemented-fully-tested` — the 403 body carries `required_scope`, the CLI names the missing scope |

So the mechanism for an unattended credential is **already shipped and already used on two surfaces**.
The only reason `/saved-views` refuses a token is that `M63-06` chose `RequireSession` for it, and that
choice is what this row has to revisit — a timer cannot hold a session cookie for a month.

## The decision, and what was rejected

**Decision: `/saved-views` moves from `RequireSession` to `RequireActorScoped` with
`Scope::LibraryRead`.** A session still passes (an empty scope set means "settled at login"); a token
must carry `library.read`, and a refusal names that scope through `M63-19`'s mechanism, so the CLI can
print "mint a token with library.read" instead of "403".

Why this is precedent rather than invention: `exports.rs` and `external.rs` already accept a session or a
scoped token, and `library.read` is exactly "read this account's library", which is what a saved query
does. Rejected alternatives, with the reason:

- **Bake the session cookie into the timer's environment file.** The cookie expires (the session TTL is
  measured in days), so the timer works until it silently stops — a failure mode that reports nothing,
  which is the worst kind for a scheduled job. Rejected.
- **A new `scope` value just for saved queries** (`query.read`). It would be the only scope that means
  "the same thing as `library.read`, but narrower", and the enum's comment says a scope names a command
  family, not a route. Rejected as a second word for one authority.
- **Keep `/saved-views` cookie-only and have the timer fail loudly at 06:00.** Honest, but the row asks
  for a timer that runs unattended; refusing to make one work is not shipping the row.
- **`systemd` system unit.** Needs root; the row says "user timer", and `M63-13`'s own text says "a user
  without a unit file".

The blast radius is one account's own views: the store is owner-scoped (`M63-06`'s cross-account test is
a `404`), so a token with `library.read` reaches exactly what that account's session reaches. That is why
this is a widening with precedent rather than a new trust boundary — and it is recorded in `README.md`,
which currently says `/saved-views` is cookie-only, because that sentence becomes false.

## Steps

### Step 1 — the route, tests first

1. `crates/domain/src/api_scopes.rs` — no change (`LibraryRead` exists).
2. `crates/app/src/routes/library.rs` — every `/saved-views` handler that takes `RequireSession` takes
   `RequireActorScoped`, calls `actor.require(Scope::LibraryRead)?`, and uses `actor.account_id` in place
   of `session.account_id`.
3. Test in `crates/app/tests/m63_13_cron.rs`:

```rust
#[tokio::test]
async fn a_token_with_library_read_opens_the_saved_views_and_one_without_is_refused_by_name() {
    // … register, mint a token with "library.read", GET /saved-views → 200
    // … mint a second token with "content.read" only → 403, body.error.required_scope == "library.read"
    // … and a cookie session → 200, because a session's authority was settled at login
}
```

Mutation: drop the `actor.require(...)` call → the second token gets `200` and the test fails.

### Step 2 — `dh cron install` writes a user timer

New module `crates/cli/src/cron.rs`, new `Command::Cron(CronArgs)` with subcommand `Install`:

```
dh cron install --saved weekly --every 6h --jitter 10m [--dry-run] [--print]
```

Files written, all under `~/.config/darkhold/cron/` (the first use of that convention, so it is stated
here rather than guessed later):

```
weekly.timer                 the timer unit
weekly.service               the service unit
weekly.env                   DARKHOLD_TOKEN=…  (0600, refused if it would be readable by others)
```

`weekly.timer`:

```ini
[Unit]
Description=darkhold saved query "weekly"

[Timer]
OnBootSec=2min
OnUnitActiveSec=6h
RandomizedDelaySec=10m
Persistent=true

[Install]
WantedBy=timers.target
```

`weekly.service`:

```ini
[Unit]
Description=darkhold saved query "weekly"
Wants=network-online.target
After=network-online.target

[Service]
Type=oneshot
EnvironmentFile=%h/.config/darkhold/cron/weekly.env
Environment=DARKHOLD_BASE_URL=http://127.0.0.1:8081
ExecStart=%h/.local/bin/dh rec --saved weekly --digest-diff
```

Refusals, each with its own code:

- `--every` must parse to a duration the timer grammar accepts (`6h`, `90m`, `1d`); anything else is a
  usage error naming the accepted shape — `systemd` would otherwise reject the unit at `daemon-reload`
  time, where the error is a journal line nobody reads.
- `--jitter` likewise, and `--jitter` without `--every` is refused (a jitter on no interval means
  nothing).
- The name is sanitized into a unit name (`[a-z0-9_-]`, others become `-`), and two different saved
  queries that sanitize to one name are a refusal, not an overwrite.
- The token is read from `DARKHOLD_TOKEN` and never printed; `--print` prints the units with the
  `EnvironmentFile=` line only, never its contents.
- An existing file is not overwritten without `--force` (a timer somebody edited by hand is data).

`--dry-run` prints the three paths and the two units and writes nothing.

### Step 3 — running it

`dh cron install` writes the files and then *prints* the two commands it will not run for you
(`systemctl --user daemon-reload`, `systemctl --user enable --now weekly.timer`), because a CLI that
shells out to `systemctl` is a CLI that fails differently on every distro. `dh cron list` (same module)
reads the directory and prints `name · every · next run` from the timer files it can parse — the
diagnostic that makes `--print` unnecessary in the common case.

### Step 2b — deviations the implementation forced

1. **Three verbs widened, not all six.** The plan said every `/saved-views` handler takes
   `RequireActorScoped`. Built narrower, on purpose: `library.read` covers list, get and
   record-a-run, which is what a timer needs, and the verbs that create, rename and delete stay
   `RequireSession`. A token minted to read a view every six hours should not be able to delete it,
   and there is no `library.write` scope to express "may write, may not destroy". The server answers
   `401` there, not `403`: it is the wrong kind of credential, not one with too few scopes.
2. **Recording a run needs `library.read`, not a write scope.** A run row is bookkeeping for a read
   — it records what the page the account already saw contained — so requiring a write scope would
   have made every scheduled `--digest-diff` need a privilege it has no other use for.
3. **`--print` does not exist; `--dry-run` prints.** Two flags for one behaviour is a flag too many.
4. **`--jitter` without `--every` cannot be tested because clap already requires `--every`.** The
   refusal written for it in Step 2 was dead code and is not shipped.
5. **The client had to learn the token too.** `dh rec` refused before the request (`set
   DARKHOLD_COOKIE … DARKHOLD_TOKEN will not open it`), so the probe answered with the right words
   for the wrong reason. `Credential` now resolves a cookie or a token, and `--save` keeps the
   session-only arm — otherwise the CLI would send a token the server still answers `401` to.
6. **The no-token refusal first named `dh token create --scope …`, which does not exist.** Caught by
   reading the output of the probe rather than its exit code; it now names `POST /api/v1/me/tokens`
   and the scope, which is the only way to mint one today.

### Step 4 — verification

```bash
cargo test -p darkhold-app --test m63_13_cron
cargo test -p darkhold-cli --bin dh cron::
cargo test -p darkhold-cli
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings
bash scripts/gate.sh all
```

Mutation proofs: remove `actor.require` (token without the scope gets 200 → test fails); make the unit
writer emit `OnUnitActiveSec` from the raw `--every` string (a unit with `6 h` and a space → the golden
test fails); make the env writer `0644` (the permission test fails).

Live probe, which is the only thing that proves the timer grammar is real:

```
dh cron install --saved weekly --every 6h --jitter 10m --dry-run   → three paths, two units, nothing written
dh cron install --saved weekly --every 6h --jitter 10m             → files, 0600 on the env, prints the two systemctl lines
systemd-analyze verify --user ~/.config/darkhold/cron/weekly.timer   → accepts the unit (or names the line it dislikes)
dh cron install --saved weekly --every 6 --jitter 10m              → exit 2, names the accepted shape
DARKHOLD_TOKEN=<scoped token> dh rec --saved weekly --digest-diff    → 200 with the token instead of 401
dh rec --saved weekly --digest-diff                                 → still 200 with the cookie
```

`systemd-analyze verify` is the step that separates "the file looks like a unit" from "systemd accepts it".

### Step 5 — the record

Ledger row; `backlog.py`; `ideas_catalogue.py`; `README.md` — the cookie-only sentence for
`/saved-views` becomes "a session, or a token carrying `library.read`", with the reason; `docs/plans/m63-06-saved-queries.md`
gets a one-line pointer that its cookie-only note was superseded here, so the two documents do not
contradict each other; STANDING-PROMPT state re-derived.

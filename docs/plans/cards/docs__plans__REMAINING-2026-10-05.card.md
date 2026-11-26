# docs/plans/REMAINING-2026-10-05.md — plan card

Source: `docs/plans/REMAINING-2026-10-05.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

| `docs/requirements.csv` | the canonical inventory — 703 rows: 672 implemented, 26 `planned`, 4 `unsupported`, 1 `evaluated-and-rejected` |
| `docs/plans/100-ideas-remaining.md` | the ideas queue — Tier 1 remainder, the 26 planned rows, the ~72 unassessed |
| `docs/plans/TRACKER-2026-10-05.md` | what the Rust gates measured, per commit |
     --port 8174 --config "$(pwd)/e2e/scratch-config.toml" serve --with-worker &
route file changed underneath: `arena.rs`, `vanguard.rs`, `media_health.rs`,
`community.rs` and `author_media.rs` are **byte-identical** between `1f5ce1a` and HEAD
fixed-height guarantee in `App.svelte:590-617` still holds.
**Why this was never caught:** `frontend/e2e/shell.spec.ts:39` ("every destination is
- `App.svelte:104-111`: the group label is `Create`, **not** `Publish` — renaming it
- `.desktop` must not go back to `flex-wrap: wrap` (`App.svelte:590`) and the scrollbar
  must stay visible if the row ever scrolls again (`App.svelte:631`).
  (`App.svelte:763-782`, drawer has `theme-select-mobile` and its own sign-out).

## Copy-pasteable code / commands

# release binary + a scratch SQLite instance with the embedded frontend
cd ~/code-local/rust/darkhold/frontend
BIN=$HOME/.cargo-target/darkhold/release/darkhold
SCRATCH=$(pwd)/test-results/probe
$BIN --storage-root "$SCRATCH/storage" \
     --database-url "sqlite://$SCRATCH/darkhold.db?mode=rwc" \
     --port 8174 --config "$(pwd)/e2e/scratch-config.toml" serve --with-worker &
# then, twice — once anonymous, once after POST /api/v1/auth/register —
# Playwright visited 35 routes and recorded every response >= 400, every console
# error, and whether an <h3>That did not work</h3> was on screen.

ERROR request{path=/api/v1/arena/next}: darkhold_app::http: request failed
      error=Internal(not enough works for arena round)  → status=500

for f in crates/app/src/routes/*.rs; do
  n=$(basename "$f" .rs)
  for fn in $(grep -oP 'pub fn \K\w+' "$f" | sort -u); do
    case "$fn" in
      *router*)
        if [ "$(grep -c "routes::$n::$fn" crates/app/src/server.rs)" -eq 0 ]; then
          echo "NOT-IN-server.rs: $n::$fn"
        fi ;;
    esac
  done
done
# → NOT-IN-server.rs: author_media::router     # the only miss in the tree

## Verification command and expected output

Two complaints opened this pass — *the header is crowded* and *some pages say "That did
SCRATCH=$(pwd)/test-results/probe
**Provenance caveat, stated rather than hidden.** The release binary under test is
## 1. The header — crowded, and the guard test structurally cannot see it
the row that fits and never the row that does not. The test is correct about the state it
  sets a `signedOut` flag, with a comment that states the rule this pass rediscovered:
**no Playwright spec visits `/arena` either.** The endpoint has no HTTP-level test at
Fix: restore the `.merge(...)`, then close the gate in the other direction — every
| gate | result |
| Playwright E2E | **not re-run today**; last recorded figure is 126/126 from the earlier pass |
| Rust workspace, PostgreSQL | `651cc9b`: **4126 passed / 3 failed**, and those 3 are the harness races fixed *by* `651cc9b` — **still open in `TRACKER-2026-10-05.md`: "in flight — the one gate that closes the session"** |
it: `scripts/pg-test-db.sh`, then export the URL it prints (4 GB shm is fixed at container
| 1 | **Restore `author_media::router()` (§3.4) + the reverse-direction route gate** | five endpoints answer a session; CI fails if any `routes/*.rs` router is unmerged |
| 6 | **Header re-layout + signed-in guard test (§1)** | `shell.spec.ts` fit assertion passes signed **in** at 1024 and 1440; header still one row, still 65px |
| 7 | **PG workspace run at HEAD (§5)** | the tracker's last open gate closes with a number |
file. 6 is the complaint that opened the pass and is deliberately last among the UI items
because its guard test has to be written before the layout is touched — a fix with no
test is the thing that let `1439bac` ship.

## RED/GREEN evidence

Result: **35 routes × 2 states = 70 page loads**. 15 of them showed the red error panel.
component for a failed *action*; it is the wrong thing to open a page with. Both halves
### 2a. Anonymous: nine pages open with a red error instead of an invitation
red error panel rather than 404. The drawer already refuses to link the operator queue
ERROR request{path=/api/v1/arena/next}: darkhold_app::http: request failed
because the four share one `error` slot the whole page renders the red panel — status,
  registration) is unchecked, and that is exactly the direction that failed.
| the browser console echoing `Failed to load resource: 401` | the same 401, logged by Chromium, not by the application |
| Rust workspace, SQLite | last measured `651cc9b`: **4160 passed / 0 failed** |
| Rust workspace, PostgreSQL | `651cc9b`: **4126 passed / 3 failed**, and those 3 are the harness races fixed *by* `651cc9b` — **still open in `TRACKER-2026-10-05.md`: "in flight — the one gate that closes the session"** |

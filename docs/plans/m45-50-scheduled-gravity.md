# M45-50 — scheduled gravity: a theme that applies for a season and then stops

Status: implementing. Row `M45-50` (admin). "Gaps review F6."

## What "theme" means here, measured first

`crates/db/src/instance_theme.rs` — a **taste** vector, not colours: `InstanceTheme { instance_id,
theme_vector: {tag: weight}, public, computed_at, updated_at }`, computed from private bookmarks and
served by `federation::get_instance_theme` (admin pseud only). So "scheduled gravity" is a taste vector
that applies during a window — an event or a season — and then stops applying, without editing the
instance's own theme and remembering to edit it back.

## The shape

One table, one pure rule, one admin surface, one reader of it:

```
theme_schedules(id, instance_id, label, theme_vector, starts_at, ends_at, created_at, revoked_at)
```

- **`active_at(db, instance_id, at)`** — the row covering `at` (`starts_at <= at < ends_at`,
  `revoked_at IS NULL`), and when several cover it, the one that **started last** wins. That tie-break
  is the decision: a Christmas event that overlaps a generic "winter" window is the more specific
  intent, and "last written wins" would make the answer depend on the order an operator typed.
- **Ends are exclusive.** `ends_at` is the instant the gravity stops; a window `..2026-12-26T00:00:00Z`
  does not apply at midnight on the 26th. Half-open intervals are what make two adjacent windows
  (the 25th, then the 26th) not both apply for an instant.
- **A window is refused before it is stored** when `ends_at <= starts_at`, or either timestamp is not
  RFC 3339 with an offset. A schedule that ends before it starts is a typo that would otherwise be
  discovered by it never applying.
- **Revoking is `revoked_at`, not deletion.** An event that was called off is a fact worth keeping, and
  the same reason `M63-06` keeps runs.

`get_instance_theme` answers with the active schedule when one covers now — `theme_vector` replaced,
plus `schedule: {id, label, starts_at, ends_at}` so a reader of the response can tell a seasonal vector
from the instance's own — and with the stored theme when none does.

## Why not reuse the config

`[modules]`, `instance_theme` and the config file are the alternatives, and each loses for a stated
reason:

- **A config key** (`theme.event_vector`, `theme.event_ends_at`) needs a restart to begin *and* a
  restart to end; an event that requires the operator to be awake at midnight to switch off is a
  scheduled thing only in name.
- **Overwriting `instance_theme.theme_vector`** loses the instance's own theme, so ending the event
  means reconstructing it — from memory, or by recomputing from bookmarks that have moved on.
- **A jobs-table row that mutates the theme** has no window at all: it fires once and the state it
  writes is indistinguishable from the operator's own.

## Steps

1. Migration `0150_theme_schedules.sql` (sqlite + postgres): the table and one index on
   `(instance_id, starts_at, ends_at)`.
2. `crates/domain/src/theme_window.rs`: `Window::{starts_at, ends_at}`, `parse_window(&str, &str) ->
   Result<Window, String>` (RFC 3339, offset required, ends > starts), `covers(&self, at)`. Tests pin
   the exclusivity, the refusal, and that `2026-12-26T00:00:00Z` does not cover a window ending then.
3. `crates/db/src/theme_schedule.rs`: `create`, `list_for_instance`, `active_at`, `revoke`. Both
   engines.
4. `crates/app/src/routes/federation.rs`: `GET/POST/DELETE /api/v1/admin/theme/schedule`, admin pseud
   only (the neighbouring theme routes' rule), and `get_instance_theme` serves the active schedule.
5. Tests: domain window rule; db create/active/revoke/overlap; app route refused for a non-admin and
   for a backwards window; the theme response carrying the schedule.

## Verification

```bash
cargo test -p darkhold-domain --lib theme_window
cargo test -p darkhold-db --test theme_schedule
cargo test -p darkhold-app --test m45_50_theme_schedule
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings
bash scripts/gate.sh all
```

Mutation proofs: make `covers` inclusive at `ends_at` → the adjacent-window test fails; make
`active_at` prefer the *earliest* start → the overlap test fails; drop the `revoked_at IS NULL`
predicate → the revoke test fails.

## The defect this row found

The first version of the route test asserted `403` for a registered reader and got `401
AUTH_REQUIRED`. The session middleware in `server.rs` is layered on the `/api/v1` router, and the
federation mount is merged at the **root** (so ActivityPub URLs are `/federation/inbox`, not
`/api/v1/federation/inbox`) — outside that layer. So `RequirePseud` and `RequireSession` saw an
anonymous request inside every federation route: the instance theme, its schedule, peers, recompute
all answered `401` to a reader who was logged in. Nothing in the tree tested them, which is why it
survived.

The fix is the precedent the reader pages already set: the federation mount loads its own session
(`routes::federation::routes().layer(from_fn_with_state(state, auth::load_session))`). Anonymous
requests stay anonymous, because `load_session` attaches a user when there is one and leaves the
request alone when there is not. Red before, green after, on the same test.

## Not done, deliberately

- No "next event starts in…" surface. The row asks for windows, and a countdown is a UI decision with
  a timezone in it.
- No per-fandom scoping of the seasonal vector beyond what a theme vector already is
  (`{tag: weight}`) — narrowing to fandoms is `M69`'s fingerprint shape, not this row's.

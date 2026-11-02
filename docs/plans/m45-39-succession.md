# M45-39: succession — what happens to the estate when the instance dies

**Ledger row:** `M45-39`. **Spec:** §11.11a (written with this plan, 2026-11-02).
**Status:** phase 1 implemented 2026-11-02 as `M45-39a` (gates below); phases 2 and 3 open, named at
the bottom of this file and still `planned` under `M45-39`.

## What the row asks for

`docs/requirements.csv`, M45-39, preservation: *"Instance-death protection: mutual backup,
dead-man's-switch export, redirect registry"*. It was queued as needing **external dependencies**
(ActivityPub), which is true of only one of its three parts — and that is the reason it sat at the
front of the queue for a month while smaller things shipped past it.

Split by what each half actually needs:

| half | needs | phase |
|---|---|---|
| redirect registry | a table and two routes | **1 — this** |
| dead-man's switch export (the *document* readers read) | a table, a clock, one operator route | **1 — this** |
| the export itself (a complete, resumable archive of an account) | an archive writer, checksums, resume | 2 |
| mutual backup between two instances | a peer protocol and a trust relationship | 3 |
| ActivityPub `Move` | actor-keyed signing; `federation.rs` has no such write | 3 |

Phase 1 is the part that makes the other two *possible to trust*: without a published destination
and a redirect registry, an export nobody can find is a file on a dying disk.

## The design decisions, and the one that is a refusal

**The succession document is public and unauthenticated.** A document behind a login cannot be read
by the person holding a dead link. It is cacheable (`Cache-Control: max-age=300`) because a document
that helps the wrong moment must not be rate-limited by that moment.

**Staleness is computed, not stored.** `review_days` (30–365, default 90) is a bound parameter and
`stale` is derived at read time. A stored `is_stale` flag becomes a lie when the clock moves, and in
a dead instance nothing will ever move it — which is the exact case the flag exists for.

**A stale plan is served, marked, not hidden.** `stale: true` plus `confirmed_at` is more useful than
a 404, because it distinguishes "this instance had a plan and stopped confirming it" from "this
instance never had one".

**`export_url` is mandatory.** A destination with no export is a forwarding address where the
destination holds the only copy. The validator refuses a plan without one; it is a field error, not a
default.

**Redirects are exact on host and path prefix, never wildcarded across hosts.** A wildcard is a
redirect any operator can aim at any instance, which turns a registry into a phishing primitive.
`from_host` is normalised (lowercase, no scheme, no port) and `from_path` is a prefix with a leading
slash.

**Both writes are operator-only and answer 404 to anyone else** — the rule that repeats across this
tree: an endpoint whose existence is a disclosure must not answer 403.

## Schema

```sql
CREATE TABLE succession_plan (
    id           TEXT NOT NULL PRIMARY KEY,   -- the singleton 'instance'
    destination  TEXT NOT NULL,               -- https URL readers should go to
    export_url   TEXT NOT NULL,               -- where a full export can be fetched
    contact      TEXT NOT NULL,               -- mailto: or https URL
    review_days  INTEGER NOT NULL DEFAULT 90,
    confirmed_at TEXT NOT NULL,               -- when the operator last confirmed
    updated_at   TEXT NOT NULL
);

CREATE TABLE succession_redirect (
    id         TEXT NOT NULL PRIMARY KEY,
    from_host  TEXT NOT NULL,                 -- normalised, lowercase, no scheme or port
    from_path  TEXT NOT NULL,                 -- prefix, always leading '/'
    to_url     TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE UNIQUE INDEX succession_redirect_pair ON succession_redirect (from_host, from_path);
```

One row for the plan (`id = 'instance'`): an instance has one succession, and a second row is a
second opinion nobody can act on.

## Routes

| method | path | audience | why |
|---|---|---|---|
| `GET` | `/api/v1/succession` | public | the document a stranger reads |
| `GET` | `/api/v1/succession/redirects?host=&path=` | public | "where did this go?" |
| `PUT` | `/api/v1/succession` | operator | set the plan; stamps `confirmed_at` |
| `POST` | `/api/v1/succession/confirm` | operator | re-confirm without changing anything |
| `DELETE` | `/api/v1/succession` | operator | withdraw the plan |
| `PUT` | `/api/v1/succession/redirects` | operator | register one redirect |

`confirm` exists separately from `put` because the common case is "nothing changed, I am still here" —
forcing an operator to resend the whole document to prove liveness would make the cheapest action the
most error-prone one.

## What is deliberately not in phase 1

- **The export itself.** `export_url` is a URL the operator publishes, not a route this instance
  serves. Building an account archive (works, pseuds, bookmarks, feedback, media manifest, checksums,
  resumable download) is phase 2 and is its own milestone; publishing a URL to a page that 404s would
  be worse than publishing none, so the validator accepts any https URL and the operator is
  responsible for it being real. Recorded here so nobody reads the column name as a promise.
- **Mutual backup.** Needs a peer protocol: which instance, what is mirrored, how a conflict is
  resolved, how trust is established, and what happens when the peer is the one that dies. Two
  instances holding each other's archive is a federation project, not a table.
- **ActivityPub `Move`.** `routes/federation.rs` reads actors; it has no actor-keyed signed write, so
  an announcement cannot be authenticated yet. Phase 3.

## What the mutations showed, recorded rather than tidied

Two of the three mutations came back the way a plan hopes. The third did not, and it is the useful
one:

- **Shortest-prefix instead of longest** → the registry test goes red. Prefix choice is the feature.
- **Width the review window by 100,000 days** → the staleness test goes red. The window is enforcement.
- **Skip the empty-`export_url` branch** → every test stays green, because `!is_https("")` refuses an
  empty string on its own. That branch buys a better sentence, not a guarantee, and the guarantee is
  the https check. Kept for the message; recorded so the next reader does not mistake it for the
  enforcement point.

## Gates

```bash
cargo test -p darkhold-domain --lib succession        # validation and staleness
cargo test -p darkhold-db  --test succession          # store, both engines
cargo test -p darkhold-app --test m45_39_succession   # the public document and the 404s, both engines
cargo test -p darkhold-app --test route_inventory     # the routes are tabled
cargo test -p darkhold-db  --test migration_catalogue # 0145 is paired
```

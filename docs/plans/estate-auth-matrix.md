# The estate's auth matrix

*M72-08. Written 2026-11-08 from the code, then checked against the running services — every row below cites the
file that implements it, and the two "answers" columns were measured with `curl` on the host, not read off a spec.*

The estate has four products and **three account stories plus two deliberate refusals**. That is not a bug being
tolerated: a recommender with no writing has no reason to hold a password, and an exporter that returns the file
you asked for has nothing to keep. What *was* a bug is that the difference was undocumented, so a shared client
could not tell "no accounts here" from "you are not signed in", and `/api/v1/auth/me` on a product with no accounts
answered `200 text/html` — the page fallback.

## The matrix

| product | accounts | who issues identity | session | machine access | the door, anonymously | implemented in |
|---|---|---|---|---|---|---|
| **darkhold** | yes | **polaris-id** (OIDC) *and* local email+password | HttpOnly cookie + CSRF token | bearer token | `401` envelope `AUTH_REQUIRED` | `crates/app/src/routes/{auth,oidc}.rs`, `crates/app/src/auth.rs` |
| **ascend** | yes | its own table | HttpOnly cookie holding a bearer token (SHA-256 stored) + CSRF | the same cookie | `200` `{user: null, auth: {password: true, …}, anonymous: {…}}` | `backend/crates/ascend-api/src/{session,cookies}.rs`, `routes/auth.rs` |
| **kindred** | **no, by design** | — | none; reading history stays in the browser (`web/src/lib/session.js`) | none needed — every route is a GET | `404` envelope `NOT_FOUND` | `src/api.rs` (`not_found_any`), `web/src/lib/session.js` |
| **ficnexus** | **no, by design** | — | none — stateless exporter | none | `404` envelope `NOT_FOUND` | `src/server.rs`, `docs/EXPORTER-SCOPE.md` |

Two rows are refusals, and both are the product working as intended:

* **kindred holds no accounts because it has nothing to write.** It ranks a read-only snapshot; the only state a
  reader produces is "what have I looked at", and that stays in their browser. Adding accounts to get "sync across
  devices" would be adding a database of reading habits for a feature nobody asked for — and the product would then
  need a policy, an erasure path and a password reset flow. `ERASURE.md` in the estate helps precisely because two
  products have nothing to erase.
* **ficnexus holds no accounts because it is an exporter.** Give it a URL, get a file; its scope document says the
  rest of the old site is darkhold's job. An account there would be a second place to sign in for no second
  capability.

## The two decisions this row forced

### 1. ascend keeps its own session — the migration is refused, with the reason

`polaris-id` is the estate's identity provider and darkhold is its client. Moving ascend onto it was the obvious
tidy answer, and it is **refused for now**, deliberately:

* ascend's session is not just "who is this". It carries **anonymous build ownership** — a visitor who never signed
  up owns builds, and `SessionUser` holds both identities so those builds can be migrated later
  (`session.rs`, first doc paragraph). Swapping the session issuer mid-flight locks every one of those people out of
  work they already own unless the migration path is written first.
* its CSRF middleware is bound to the cookie format (`session.rs::csrf`, `cookies.rs`), and its admin/trust gates
  read session-derived roles. That is three subsystems to move, not one field.
* the OIDC client would have to be tested against a *second* product's redirect URIs and consent copy — darkhold's
  is already registered, and a botched cutover is a sign-in loop for the site's own users.

What would make it right: a milestone with (a) a polaris-id client registration for ascend, (b) a one-shot migration
that maps existing sessions to polaris-id subjects by verified email, (c) anonymous build ownership claimed by
signed-in users before the old cookies stop being accepted, (d) the CSRF layer moved to the shared form. Recorded in
the ledger as the row's *remaining* half rather than quietly called done.

### 2. "Who am I" has one shape per product *class*, not one shape overall

darkhold refuses an anonymous caller with `401 AUTH_REQUIRED`; ascend answers `200` with `user: null` and the auth
capabilities. Both are defensible, and the difference is information, not style:

* ascend's sign-in page must know **which providers this instance has enabled** before it can draw a form. Sending
  that in a `401` body, or making the page fetch a second endpoint, is worse than answering the question it asked.
* darkhold's reader is anonymous by default and reads the policy block from `/api/v1/meta`; a `401` there is the
  honest answer to "give me my account".

The rule the family keeps: **a refusal is the estate envelope** (M72-04), and a product may answer
`200 {user: null}` where its UI needs capability discovery — with the difference named here. What a client must be
able to rely on is the pair: *no accounts* ⇒ `404 NOT_FOUND`, *accounts but anonymous* ⇒ either `401 AUTH_REQUIRED`
or `200` with a null user, never HTML.

## What changed in code for this row

* **kindred** — its fallback rendered a page for everything, so `/api/v1/auth/me` answered `200 text/html` on a
  product with no accounts: indistinguishable from being signed in, to anything that is not a browser. The fallback
  is now path-aware (`not_found_any`): a page under `/`, the envelope under `/api/`. `AppError::RouteNotFound` exists
  so the answer comes from the same place every other refusal does. Three mutants red (paths never detected, prefix
  instead of segment, a resource code on a route refusal).
* **ascend** — `/api/...` had no fallback, so an unknown address answered a bare `404` with no body. It now answers
  the envelope, like every other refusal.
* **darkhold** — already conformed; its `/api/v1/auth/me` answers `401 AUTH_REQUIRED` with a `request_id`.

## How this is checked

```sh
# every product's account door, anonymously, on the host
curl -s -o /dev/null -w '%{http_code} %{content_type}\n' http://127.0.0.1:8081/api/v1/auth/me   # 401 application/json
curl -s                              http://127.0.0.1:8024/api/auth/me                          # 200 {"user": null, …}
curl -s -o /dev/null -w '%{http_code} %{content_type}\n' http://127.0.0.1:8030/api/v1/auth/me   # 404 application/json
```

The failure this row exists to prevent is the third one answering `200 text/html` — measured on 2026-11-08 before
the fallback change, and it is why the rule is written as "never HTML" rather than "always one status".

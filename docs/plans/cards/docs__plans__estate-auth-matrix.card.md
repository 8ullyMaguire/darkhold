# docs/plans/estate-auth-matrix.md — plan card

Source: `docs/plans/estate-auth-matrix.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

| **darkhold** | yes | **polaris-id** (OIDC) *and* local email+password | HttpOnly cookie + CSRF token | bearer token | `401` envelope `AUTH_REQUIRED` | `crates/app/src/routes/{auth,oidc}.rs`, `crates/app/src/auth.rs` |
| **ascend** | yes | its own table | HttpOnly cookie holding a bearer token (SHA-256 stored) + CSRF | the same cookie | `200` `{user: null, auth: {password: true, …}, anonymous: {…}}` | `backend/crates/ascend-api/src/{session,cookies}.rs`, `routes/auth.rs` |
| **kindred** | **no, by design** | — | none; reading history stays in the browser (`web/src/lib/session.js`) | none needed — every route is a GET | `404` envelope `NOT_FOUND` | `src/api.rs` (`not_found_any`), `web/src/lib/session.js` |
| **ficnexus** | **no, by design** | — | none — stateless exporter | none | `404` envelope `NOT_FOUND` | `src/server.rs`, `docs/EXPORTER-SCOPE.md` |
  (`session.rs`, first doc paragraph). Swapping the session issuer mid-flight locks every one of those people out of
* its CSRF middleware is bound to the cookie format (`session.rs::csrf`, `cookies.rs`), and its admin/trust gates

## Copy-pasteable code / commands

# every product's account door, anonymously, on the host
curl -s -o /dev/null -w '%{http_code} %{content_type}\n' http://127.0.0.1:8081/api/v1/auth/me   # 401 application/json
curl -s                              http://127.0.0.1:8024/api/auth/me                          # 200 {"user": null, …}
curl -s -o /dev/null -w '%{http_code} %{content_type}\n' http://127.0.0.1:8030/api/v1/auth/me   # 404 application/json

## Verification command and expected output

No verification command or expected output was recorded in the source document.

## RED/GREEN evidence

  so the answer comes from the same place every other refusal does. Three mutants red (paths never detected, prefix

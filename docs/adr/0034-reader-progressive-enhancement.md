# 0034 — The reader gains an external progressive-enhancement script

**Date:** 2026-11-26
**Status:** Accepted
**Context:** ADR 0006 ("one binary, two renderers"), ADR 0033 ("the Archive is the chrome; the Reading Room is the reader"), `crates/app/src/routes/reader_page.rs`, `crates/app/src/server.rs`, and the 2026-11-26 UI critical pass.

## Problem

ADR 0006 promises server-rendered documents with no `<script>` element. That guarantee made sense when the reader's only dynamic need was "save my place", delivered by a real form. The 2026-11-26 audit found two things it cannot deliver:

* a reader with a saved position sees the position label but is not returned to it automatically;
* author-supplied HTML carries multiple `<h1>`/`<h2>` headings, so the document has three titles before prose.

Adding an inline script is blocked by the site's `script-src 'self'` CSP, and an inline script would also turn the no-script guarantee into a lie. The user chose automatic restore while requiring the no-JS document to remain fully readable.

## Decision

The reader keeps all reading content, chapter links, search, position form and gate forms in the server-rendered HTML. It links one external, same-origin script (`/reader-position.js`, copied from `frontend/static/` into the embedded dist bundle) from the end of the body.

The script does only two things:

1. when the server has emitted `data-restore-permille` on `.reader-surface__main`, scroll once to that permille of the reading body after layout;
2. update a lightweight progress rail from scroll position, without writing to the server.

Demoting author headings is **not** the script's job. It is a pure server-side transform of the already-sanitized body HTML before it is written into the document, so it works with JavaScript disabled too.

The no-JS promise is amended: **no inline script, and nothing needed to read or navigate is behind the external script**. The test that used to assert "no `<script>`" now asserts that no inline script exists and that the external script is an enhancement.

## Alternatives rejected

**Inline `<script>` with a CSP hash/nonce.** Would require per-page nonce plumbing or a static hash that changes with every edit, and would still put executable code in a document whose selling point is that it needs none.

**localStorage-only restore with no server position.** Would ignore the stored position the product already has and would not follow the reader across devices.

**Client-side rendering of the reader.** Rejected by ADR 0006 and spec §8.5; with JavaScript off it produces an empty page.

**Leave the imported headings as-is.** A single reader page then contains three `h1` elements and repeats its title before the prose, which fails both hierarchy and reading comfort.

## Consequences

* `frontend/static/reader-position.js` becomes part of the embedded asset set and must be served under the existing `script-src 'self'` policy.
* `reader_page.rs` may link an external script but must never emit an inline one.
* The reader's `--reader-*` tokens remain independent of site palette tokens (ADR 0033).
* The enhancement is small enough to fail silently: if it is blocked or missing, the document is unchanged except for the absence of automatic restore and the live progress fill.

## Verification

```bash
grep -n 'reader-position.js' crates/app/src/routes/reader_page.rs
grep -n 'fn normalize_imported_body' crates/app/src/routes/reader_page.rs
grep -n 'script-src' crates/app/src/server.rs
```

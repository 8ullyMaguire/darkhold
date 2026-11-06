# 0006 — One binary, two renderers: Rust owns the document pages

Status: accepted, 2026-11-06. Decides M71-01; unblocks M71-03…M71-16.

## Problem

The site has two kinds of page and one renderer. The reader (`/works/{id}/units/{unit}`) is rendered by the server
in `crates/app/src/routes/reader_page.rs`, deliberately, with the spec clauses quoted in its header: §8.5 requires a
document, §21.1 requires it to work without JavaScript, and the spoiler reveal, position, notes and DNF are all
`<form>` posts. Everything else — landing, work page, browse, author, fandom, tag, library, admin — is a Svelte 5
application built by Vite into static assets and embedded in the same binary.

The consequences are real but narrower than the external proposal claims: those ~90 surfaces answer a crawler, an
OPDS consumer, a link preview or a reader with JavaScript off with an empty shell. The prose is safe because the
reader already escaped. Everything that decides *whether you will read* — the work page, the fandom page, the
search results — does not.

The proposal's answer is "server-render every page" without naming a mechanism. That is the decision this ADR
makes, because the mechanism determines the cost and the deploy shape.

## Decision

**Keep one binary. Extend the Rust renderer to the pages whose job is to be a document, and keep the SPA for the
surfaces whose job is to be an application.**

Document-class pages, in this order:

1. the work page `/works/{id}` — the page that gates every read, and the one a link preview shows;
2. the fandom page `/browse/fandoms/{slug}` and the tag page `/browse/tags/{slug}` — the catalogue's landing pages;
3. search results `/browse?q=…` — the answer to "the catalogue is 141k rows";
4. the landing page `/`, which must give a stranger something readable with no runtime;
5. the author page `/browse/people/{handle}` and collection/series pages.

Application-class, staying in Svelte: the editor, library management, settings, forum composition, admin console,
governance, import/export flows, everything that is a form over a session.

A page is document-class if a reader can arrive at it from a link, a feed, a search engine or a shared URL and its
value is the information on it. A page is application-class if its value is interaction with state.

The rule that keeps this from drifting: **a document page has a server test that fetches it, parses the HTML and
asserts its content with no script execution**, and the page is served with the same stylesheet the SPA uses
(`assets::app_stylesheet`), so there is one design system, not two.

## Alternatives and why they lost

- **SvelteKit with `adapter-node` (real SSR).** It is the shortest path to "every page renders server-side" and it
  costs a second runtime: a Node process beside the Rust binary, a second set of secrets and config, a second thing
  to keep alive under systemd on a single-operator host, and the deploy contract ("scp one binary, restart one
  unit") stops being true. It also puts the HTML path in a language the rest of the server cannot test.
- **`adapter-static` plus prerendering.** Cannot prerender content that changes per request. It would produce an
  empty shell with extra steps for the pages that matter and a stale file for the ones that do not.
- **Rewrite the whole frontend in Rust templates.** 91 route components, most of them interactive state machines
  (editor, admin, forum). The templates would be worse than the Svelte components at the thing they do, and the
  work would be measured in months for a benefit that exists on perhaps eight pages.
- **Accept the shell.** This is what the site does now, and it contradicts §8.5, §8.10 (anonymous is a
  first-class audience) and §7O (an e-reader is a first-class client). It is also the failure the proposal
  names most accurately.
- **A separate static site generator for "public" pages.** Two build pipelines, two deploy artefacts, and the
  catalogue changes too often for a build-time snapshot to be honest.

## Consequences

- Two template paths coexist until a page migrates. The mitigation is that migration is per page and each page's
  Svelte route is deleted in the same commit, so the count only goes down.
- Layout primitives (header, skip links, footer, flash) must exist in both the Rust renderer and the SPA until the
  last document page moves. They read the same tokens and the same stylesheet; the Rust side reuses `crate::html`.
- The SPA keeps the session-cookie path, so a signed-in reader's view of a document page (bookmark state, reading
  status, author tools) needs either a second request or a small script that enhances the document. The chosen
  shape: render the anonymous-eligible document, then let the existing SPA boot on the same URL only if the reader
  is signed in and the page declares an enhancement root. A signed-out reader never downloads the bundle.
- Crawlers, previews, feeds and OPDS get real documents without new endpoints.

## Evidence for the decision, not for the ambition

- `crates/app/src/routes/reader_page.rs` renders a complete document, 1,431 lines, with the stylesheet linked from
  the asset table; `crates/app/src/html.rs` is a 241-line escaping builder with `open`/`close`/`text`/`raw`.
- The SPA is `vite` + Svelte 5 with a hand-rolled router (`frontend/src/lib/router.ts`) — there is no SvelteKit and
  no adapter to switch, so "SSR" would mean a new runtime, not a config flag.
- `frontend/src/routes` holds 91 components; `frontend/src/lib/components` holds 80, of which the interactive ones
  (editor, admin, forum) are the majority of the surface area.

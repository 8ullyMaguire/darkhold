# UI critical pass — implementation plan (2026-11-26)

Work from `/home/alvaro/code/rust/darkhold` unless a step says otherwise. Do not start before the spec and ADR are committed. Every step ends with its exact verification command. Only the last step runs the repository gate.

## Step 0 — docs before code

Files (already written):
- `docs/spec-amendments/ui-critical-pass-2026-11-26.md`
- `docs/adr/0034-reader-progressive-enhancement.md`

```bash
cd /home/alvaro/code/rust/darkhold
git add docs/spec-amendments/ui-critical-pass-2026-11-26.md docs/adr/0034-reader-progressive-enhancement.md
git commit -m "docs: specify the UI critical pass and reader enhancement"
git status --short
```
Expected: empty status.

## Step 1 — fix the palette cascade and give the injected bar inherited tokens

File: `frontend/src/styles/tokens.css`.

Replace the four palette imports with this order, so the plain `[data-palette="archive"]` rule comes after reading-room's `:root, [data-palette="reading-room"]` rule of equal specificity:

```css
@import '@polaris/ui/palettes/reading-room.css';
@import '@polaris/ui/palettes/archive.css';
@import '@polaris/ui/palettes/after-hours.css';
@import '@polaris/ui/palettes/clear-day.css';
```

Add to the existing `:root` block (after `--font-interface`):

```css
  /* The injected estate chrome lives in a shadow root; inherited custom properties are the only styling contract it has. */
  --polaris-bar-bg: var(--surface-0);
  --polaris-bar-fg: var(--text-primary);
  --polaris-bar-dim: var(--text-secondary);
  --polaris-bar-line: var(--border-subtle, rgba(128,128,128,.3));
```

Verify:

```bash
cd /home/alvaro/code/rust/darkhold/frontend
node node_modules/vite/bin/vite.js build >/tmp/darkhold-ui-build-1.log 2>&1 && grep -n 'data-palette=archive' dist/assets/*.css | head -3 && echo BUILD1_OK
```
Expected: `BUILD1_OK`, archive rule appears after reading-room root rule.

## Step 2 — make the injected Polaris bar auth-aware and site-tuned

File: `/home/alvaro/code/estate/ui/src/chrome/topbar.js`.

In the shadow CSS replace the hard-coded bar vars and icon size:

```js
      --bar-bg: var(--polaris-bar-bg, #06070d);
      --bar-fg: var(--polaris-bar-fg, #b9bdc9);
      --bar-dim: var(--polaris-bar-dim, #7f8698);
      --bar-line: var(--polaris-bar-line, #ffffff14);
```

```js
    .icon-link {
      width: 44px; height: 44px; display: inline-flex; align-items: center; justify-content: center;
```

Replace the Search/Sign-in emission block with body-state-aware logic:

```js
      const signedIn = document.body?.dataset?.darkholdSignedIn === 'true';
      const pageHasSearch = document.body?.dataset?.darkholdHasSearch === 'true';
      if (routes.search && !pageHasSearch) nav_.append(el('a', { class: 'action', href: routes.search }, 'Search'));
      const authHref = signedIn ? (routes.account || '/account') : routes.auth;
      if (authHref) nav_.append(el('a', { class: 'action', href: authHref }, signedIn ? 'Account' : 'Sign in'));
```

Replace the fixed `legible(accent || '#b3a6ff', [6, 7, 13])` call with a bar-background-aware version that mixes toward white on dark backgrounds and toward black on light ones:

```js
      const barBg = parse(getComputedStyle(document.documentElement).getPropertyValue('--polaris-bar-bg').trim()) || [6, 7, 13];
      const text = legible(accent || '#b3a6ff', barBg);
```

After `root.append(nav_)`, add a re-render trigger for SPAs that set body state after the topbar first paints:

```js
      if (!this._stateObserver && typeof MutationObserver !== 'undefined') {
        this._stateObserver = new MutationObserver(() => {
          if (this.shadowRoot) this.shadowRoot.replaceChildren();
          this.render(caps, nav);
        });
        this._stateObserver.observe(document.body, { attributes: true, attributeFilter: ['data-darkhold-signed-in', 'data-darkhold-has-search'] });
      }
```

Guard against duplicate observers by disconnecting before re-render if one exists. Keep the "no caps/nav => empty element" contract intact.

Verify:

```bash
node --check /home/alvaro/code/estate/ui/src/chrome/topbar.js && echo TOPBAR_PARSE_OK
cd /home/alvaro/code/estate && git status --short
```

Deploy on thinkcentre only after all other code is ready:

```bash
ssh thinkcentre 'cd /personal/documents/code/estate && sudo bash scripts/install-chrome.sh darkhold.polarisocial.xyz'
curl -s https://darkhold.polarisocial.xyz/estate/topbar.js | grep -q 'data-darkhold-signed-in' && echo TOPBAR_LIVE_OK
```

## Step 3 — set body state from both renderers

File: `crates/app/src/routes/document.rs`. Change the body open to:

```rust
    page.open(
        "body",
        &[
            ("class", "doc"),
            ("data-darkhold-has-search", "false"),
        ],
    );
```

File: `crates/app/src/routes/landing_page.rs`. Change the body open to:

```rust
    page.open(
        "body",
        &[
            ("class", "landing"),
            ("data-darkhold-has-search", "true"),
        ],
    );
```

File: `crates/app/src/routes/reader_page.rs`. Change the body open to include both flags:

```rust
    page.open(
        "body",
        &[
            ("class", "reader-surface"),
            ("data-darkhold-has-search", "true"),
            ("data-darkhold-signed-in", if signed_in { "true" } else { "false" }),
        ],
    );
```

File: `frontend/src/App.svelte`. In the existing reactive/effect block that owns theme state, add:

```ts
  $effect(() => {
    document.body.dataset.darkholdHasSearch = 'true';
    document.body.dataset.darkholdSignedIn = signedIn ? 'true' : 'false';
  });
```

Use the component's existing session-derived boolean; do not invent a new request.

Verify:

```bash
cd /home/alvaro/code/rust/darkhold
cargo check -p darkhold-app >/tmp/darkhold-check-shell.log 2>&1 && echo SHELL_RUST_OK
cd frontend && npx vitest run src/App.test.ts 2>&1 | tail -5
```
Expected: `SHELL_RUST_OK` and passing App tests.

## Step 4 — repair the landing page

File: `crates/app/src/routes/landing_page.rs`.

In the inline style, append these declarations to `.landing .shelf-list li`:

```css
             .landing .shelf-list .meta{grid-column:2;display:flex;flex-wrap:wrap;gap:0 var(--space-3)}
             .landing .shelf-list .meta span{white-space:nowrap}
             .landing .shelf-head{display:flex;align-items:baseline;justify-content:space-between;gap:var(--space-3)}
             .landing .shelf-all{font-size:var(--text-sm);color:var(--text-link,inherit);min-height:44px;display:inline-flex;align-items:center}
```

Change the meta emission from one joined string to wrapped spans:

```rust
            page.open("div", &[("class", "meta")]);
            for part in &meta {
                page.open("span", []).text(part).close("span");
            }
            page.close("div");
```

Replace the bare `h2` with a heading row:

```rust
        page.open("div", &[("class", "shelf-head")]);
        page.open("h2", []).text("Open now").close("h2");
        page.open("a", &[("class", "shelf-all"), ("href", "/search")])
            .text("Browse all")
            .close("a");
        page.close("div");
```

Add author diversity in `starters()`: keep a `HashSet<String>` of author handles; skip a candidate whose non-empty author was already used until the linked list has six entries, then relax the filter. Do not reduce `STARTERS` or `STARTER_POOL`.

Verify (targeted test file only):

```bash
cd /home/alvaro/code/rust/darkhold
cargo test -p darkhold-app --test landing_page 2>&1 | tail -8
```
Expected: all tests pass, including a new assertion that `.meta` contains spans and `Browse all` links to `/search`.

## Step 5 — reader: one heading, normalised author HTML, automatic restore

File: `frontend/static/reader-position.js` (new). Complete content:

```js
// Progressive enhancement only. The reader is fully readable without this file.
(() => {
  const main = document.querySelector('.reader-surface__main');
  const body = document.querySelector('.reader-surface__body');
  if (!main || !body) return;

  const rail = document.querySelector('.reader-surface__rail');
  const paint = () => {
    const scrollable = Math.max(1, document.documentElement.scrollHeight - innerHeight);
    const value = Math.min(100, Math.max(0, Math.round((scrollY / scrollable) * 100)));
    main.style.setProperty('--reader-progress', String(value));
    if (rail) rail.setAttribute('aria-valuenow', String(value));
  };

  const saved = Number(main.dataset.restorePermille || 0);
  if (saved > 1 && saved <= 1000 && !location.hash) {
    requestAnimationFrame(() => requestAnimationFrame(() => {
      const top = body.getBoundingClientRect().top + scrollY;
      const distance = Math.max(0, body.scrollHeight - innerHeight);
      scrollTo(0, top + (distance * saved) / 1000);
      paint();
    }));
  }
  addEventListener('scroll', paint, { passive: true });
  addEventListener('resize', paint, { passive: true });
  paint();
})();
```

File: `crates/app/src/routes/reader_page.rs`.

Add a pure transform near `render_page`:

```rust
/// Author HTML arrives with `<h1>`/`<h2>` headings that compete with the page's own title.
/// Demote those exact tags to styled paragraphs. The editor schema emits no attributes on headings,
/// so the closed input makes string replacement sufficient and avoids a second HTML parser.
fn normalize_imported_body(html: &str) -> String {
    html.replace("<h1>", "<p class=\"reader-imported-heading\">")
        .replace("</h1>", "</p>")
        .replace("<h2>", "<p class=\"reader-imported-heading\">")
        .replace("</h2>", "</p>")
        .replace("<p>Published:", "<p class=\"reader-imported-meta\">Published:")
        .replace("<p>Updated:", "<p class=\"reader-imported-meta\">Updated:")
        .replace("<p>Words:", "<p class=\"reader-imported-meta\">Words:")
        .replace("<p>Chapters:", "<p class=\"reader-imported-meta\">Chapters:")
}
```

Use it where the sanitized body is emitted; do not change `sanitized_html` in the database.

Add the restore attribute to the `main` open:

```rust
    let restore = (!stale_position)
        .then(|| position.map(|p| p.fraction))
        .flatten()
        .filter(|fraction| *fraction > 1);
    page.open(
        "main",
        &[
            ("class", "reader-surface__main"),
            ("data-restore-permille", restore.map(|f| f.to_string()).as_deref().unwrap_or("")),
        ],
    );
```

Add one progress rail inside `.reader-surface__controls` after the position label:

```rust
    page.open(
        "div",
        &[
            ("class", "reader-surface__rail"),
            ("role", "progressbar"),
            ("aria-label", "Reading progress"),
            ("aria-valuemin", "0"),
            ("aria-valuemax", "100"),
            ("aria-valuenow", "0"),
        ],
    );
    page.close("div");
```

Link the external script immediately before closing `body`:

```rust
    page.open(
        "script",
        &[("src", "/reader-position.js"), ("defer", "defer")],
    );
    page.close("script");
```

File: `frontend/src/styles/reader.css`. Add:

```css
.reader-imported-heading {
  margin: 1.25em 0 0.4em;
  font-family: var(--font-interface);
  font-size: 0.82em;
  font-weight: 600;
  letter-spacing: 0.04em;
  text-transform: uppercase;
  color: var(--reader-muted);
}
.reader-imported-meta {
  font-family: var(--font-mono);
  font-size: 0.85em;
  color: var(--reader-muted);
}
.reader-surface__rail { flex: 1 1 6rem; height: 4px; border-radius: 999px; background: color-mix(in srgb, var(--reader-text) 14%, transparent); }
.reader-surface__rail::after { content: ''; display: block; width: calc(var(--reader-progress, 0) * 1%); height: 100%; border-radius: inherit; background: var(--reader-link); }
```

Update `crates/app/tests/reader_surface.rs` (or the nearest reader document test) to assert: no inline `<script>`, one external `src="/reader-position.js"`, one `h1`, zero `<h2>` inside `.reader-surface__body`, and a `data-restore-permille` attribute when a position is passed.

Verify:

```bash
cd /home/alvaro/code/rust/darkhold
cargo test -p darkhold-app --test reader_surface 2>&1 | tail -8
```
Expected: all pass.

## Step 6 — standalone tap targets and heading order

File: `frontend/src/app.css`. Append:

```css
/* Standalone controls, not prose links: WCAG 2.5.8. Radio/checkbox indicators are exempt. */
button, summary, input[type='submit'], input[type='button'],
a.button, a.btn, a.action, a.pill, [role='button'] {
  min-height: 44px;
  min-width: 44px;
}
```

Heading order: for each of `/library/history`, `/account`, `/notifications`, `/import`, `/write`, `/jobs`, `/arena`, `/draw`, `/concierge`, find the first `h3` in the live DOM after sign-in and add a descriptive `h2` before it (or promote that `h3` when it is the section title). Do not add a visually hidden heading; it must be visible.

Verify with a signed-in Playwright query using the existing sweep harness:

```bash
PLAYWRIGHT_BROWSERS_PATH=/home/alvaro/.hermes/profiles/coding/cache/ms-playwright \
  /home/alvaro/.hermes/profiles/coding/cache/scratch/audit-venv/bin/python \
  /home/alvaro/.hermes/profiles/coding/cache/scratch/darkhold_ui_sweep.py >/tmp/darkhold-sweep-after.log 2>&1
python3 - <<'PY'
import json
d=json.load(open('/home/alvaro/.hermes/profiles/coding/cache/scratch/darkhold-ui-sweep-dark.json'))
print('heading_skips',sum(1 for r in d['results'] if r.get('structure',{}).get('headingSkips')))
print('small_targets',sum(len(r.get('smallTargets',[])) for r in d['results']))
PY
```
Expected: `heading_skips 0`, `small_targets 0`.

## Step 7 — empty, loading and identity states

- `frontend/src/routes/Roadmap.svelte`: in the empty state add a visible `<a class="button" href="/community">Suggest a feature</a>`.
- `frontend/src/routes/Library.svelte`: in the signed-in empty state add `<a class="button" href="/import">Import a work</a>` and keep the existing explanatory copy.
- `frontend/src/routes/Notifications.svelte`: while loading show three skeleton rows with text `Loading notifications…`; on failure show `Notifications could not be loaded. Try again.` plus a retry button.
- `frontend/src/routes/Jobs.svelte`: same skeleton/failure pattern for the job list.
- `frontend/src/App.svelte`: footer `Radio` gets a one-line description matching `Kindred`; change `Writing as@` to `Writing as @`; add a visible ellipsis when the handle is clipped.
- `frontend/src/App.svelte`: label the coloured "More" dot with `title`/`aria-label`; label the typography envelope icon with `aria-label="Reading appearance"`.

Verify:

```bash
cd /home/alvaro/code/rust/darkhold/frontend
npx vitest run src/App.test.ts src/routes/Library.test.ts src/routes/Roadmap.test.ts 2>&1 | tail -8
npx svelte-check --tsconfig ./tsconfig.json 2>&1 | tail -4
```
Expected: tests pass, `svelte-check` reports 0 errors.

## Step 8 — build, deploy, live proof

Build and deploy Rust + frontend on thinkcentre only:

```bash
ssh thinkcentre '~/.local/bin/estate-build-mode.sh start'
ssh thinkcentre 'set -e; cd /personal/documents/code/rust/darkhold; \
  CARGO_PROFILE_RELEASE_LTO=off CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 \
  CARGO_TARGET_DIR="$HOME/.cargo-target/darkhold" cargo build --release --locked; \
  cp ~/.cargo-target/darkhold/release/darkhold ~/.cargo-target/darkhold/release/darkhold.new; \
  sudo systemctl stop darkhold.service; \
  install -m 0755 ~/.cargo-target/darkhold/release/darkhold.new ~/.cargo-target/darkhold/release/darkhold; \
  ~/.cargo-target/darkhold/release/darkhold migrate; \
  sudo systemctl start darkhold.service; \
  ~/.local/bin/estate-build-mode.sh end; \
  ~/.local/bin/estate-build-mode.sh status'
```
Expected: `OFF`, health 200, meta stamp changes.

Live DOM proof: re-run the sweep from Step 6 and inspect at least `dark-out-home-desktop.png`, `dark-out-home-phone.png`, `dark-in-library-desktop.png`, `dark-out-reader-desktop.png` with vision. Confirm:
- archive body bg dark;
- no duplicated Sign in / Search in the injected bar;
- landing metadata on one line;
- exactly one `h1` on the reader and no inline script;
- `/reader-position.js` returns 200.

## Step 9 — record, gate, commit

Update `docs/requirements.csv`, `docs/FEATURE-LEDGER.md`, `docs/plans/roadmap-remaining.md`, `docs/handoff.md` and a vault note under `/home/alvaro/secondbrain/10-Projects/darkhold/` with the same row ids `UC-1`..`UC-10` and the final live measurements.

Run the repository gate once:

```bash
cd /home/alvaro/code/rust/darkhold
./scripts/gate.sh 2>&1 | tail -12
```
Expected: GATE PASS with counts matching the ledger.

Commit each repo separately with conventional messages, then push to the configured remotes:
- darkhold: `feat(ui): finish the critical UX pass`
- estate: `fix(chrome): make the injected bar auth-aware and theme-tuned`
- vault: `docs(darkhold): record the UI critical pass`

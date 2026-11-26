# UI refactor from the skeleton — execution-ready plan cards

**Source:** `docs/spec-amendments/ui-refactor-from-skeleton-2026-11-26.md`
**Decision:** `docs/adr/0035-port-skeleton-patterns-not-the-stack.md`
**Comparison evidence:** `/home/alvaro/darkhold-build-comparison-2026-11-26.md`

These cards are written for an implementer with zero context. Paths and symbols were checked
against this checkout while drafting; the code blocks below were **not compiled in this docs-only
pass**, and the verification commands were **not executed here**. Run each command at its step and
paste the real output into the card before marking it done. Do not invent a green result.

## Ordering constraints

UIR-0 before everything (two measured hard failures outrank polish). UIR-1 before UIR-2..4 because
wayfinding is the largest comprehension win and the server-rendered documents need a shared
partial before per-row work. UIR-5 is cheap and can follow UIR-0. Do not start XW-1 (ranking
feedback) from this plan — it stays gated on the experiment loop.

---

## UIR-0 — Repair the two measured hard failures

### UIR-0a — reader controls recede to 0.14 at scroll position zero

**File:** `frontend/src/styles/reader.css` (lines 439–449, `@keyframes reader-controls-recede`).

Measured at 390×844 on live build `0.1.0+24e5b2cd`: `document.querySelector('.reader-surface__controls')`
computes `opacity: 0.14` with `scrollY = 0`, even though the keyframe starts at 1 and the rule is
scoped to `animation-range: 0 220px`. Hover/focus restores it, so a pointer user can recover; a
touch reader sees four nearly invisible controls. This is a recede effect firing at the wrong end,
not a missing stylesheet.

Change only the floor and keep the hover/focus escape:

```css
@keyframes reader-controls-recede {
  from { opacity: 1; transform: none; }
  to { opacity: 0.62; transform: translateY(-0.2rem); }
}
```

Do **not** change `--reader-*` tokens (ADR 0033) and do not add an inline script (ADR 0034).

**Verify (browser, 390×844):** navigate to a reader URL, set `scrollY = 0`, read
`getComputedStyle(document.querySelector('.reader-surface__controls')).opacity`. Expected: `>= 0.6`.
Then scroll to 400 and re-read: expected still `>= 0.6`. Focus the strip: expected `1`.

**Verify (build):**

```sh
cd /home/alvaro/code/rust/darkhold/frontend
node node_modules/vite/bin/vite.js build
```

Expected: `✓ built in …` and no TypeScript error. This command is the documented frontend build;
do not use `pnpm build`.

### UIR-0b — same work, two authors and two word counts

**Files:** `crates/app/src/routes/landing_page.rs` (Starter projection, lines 194–221 and 415–420),
`crates/app/src/routes/work_page.rs` (public contributor/word projection, lines 145–212),
`crates/app/tests/landing_page.rs`.

Measured: `/` prints `by devwriter · 5,367 words`; the reader prints `By: MarcusBlack` and
`Words: 6220` for the same work id. Do not silently rewrite imported metadata. Add a test that
derives both strings from one work id and fails while they disagree, then make the shelf projection
use the same public-contributor and readable-unit word sum as `work_page.rs` (functions
`darkhold_db::collaboration::public_contributors` and the `readable.iter().map(|unit| unit.word_count).sum()`
pattern already exist at `work_page.rs:145-146`). If the two projections are legitimately different
(owner vs contributor), print the role word on both surfaces instead of deleting one.

**Verify:**

```sh
cd /home/alvaro/code/rust/darkhold
cargo test -p darkhold-app --test landing_page
```

Expected: the new disagreement test passes; quote its first line in the card. Mutation: change
the shelf fixture author by one character, re-run, require `FAILED`, restore.

**Status:** code/tests done; live DOM opacity proof is the UIR-6 checkpoint.

Execution evidence (2026-11-26):

```
# First RED exposed a fixture-order bug, not the product bug:
$ cargo test -p darkhold-app --test landing_page
error returned from database: (code: 787) FOREIGN KEY constraint failed
test result: FAILED. 2 passed; 3 failed; 0 ignored; 0 measured; 0 filtered out
RED_EXIT=101

# Fixture order fixed (work before work_contributors), real RED:
$ cargo test -p darkhold-app --test landing_page
assertion `left: "/by hollowgast/"`, right: `"/by Public Writer/"`: the shelf and the work page must credit the same public contributor
test result: FAILED. 4 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out
RED2_EXIT=101

# GREEN after projecting public contributors on the shelf:
$ cargo test -p darkhold-app --test landing_page
test a_signed_in_reader_is_sent_to_discovery ... ok
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 7.45s
GREEN2_EXIT=0

# Mutation: display name `Public Writer` -> `Public Writeq` in the fixture:
$ cargo test -p darkhold-app --test landing_page
test the_landing_page_is_a_document_with_no_script ... FAILED
test result: FAILED. 4 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 16.46s
MUT_EXIT=101
# Restored to `Public Writer`; prior GREEN applies to the restored file byte-for-byte.

# Reader-control floor:
$ grep -n 'to { opacity: 0.62' frontend/src/styles/reader.css
441:  to { opacity: 0.62; transform: translateY(-0.2rem); }
```

The first assertion used the pseud handle; GREEN uses the work-page display-name rule, so the shelf and work page print the same string for the same work. No metadata was rewritten. XW-1 and the injected Polaris bar were not touched.

---

## UIR-1 — One Darkhold nav on every surface

**Files:** `crates/app/src/routes/landing_page.rs` (`render`, lines 254–332),
`crates/app/src/routes/search_page.rs`, `crates/app/src/routes/work_page.rs`,
`crates/app/src/App.svelte` (`NAV_GROUPS`, `primaryNav`, header at line 395),
`estate/ui/src/chrome/topbar.js` (read-only constraint).

Public server-rendered pages inject `<polaris-chrome>` from `/estate/topbar.js`; its shadow root
contains the 12-site Polaris switcher, not Darkhold destinations. Do not delete it and do not put
site links inside it. Emit a small server-side `<nav aria-label="Darkhold">` **before** `<main>` in
each server-rendered document, using the same destination vocabulary as `App.svelte`'s
`primaryNav` (Discover / Search / Library / Shelf / More). Mark the current destination with
`aria-current="page"`. Use the site's inherited CSS custom properties; no inline script, no client
mount. Keep the existing `data-darkhold-has-search` and `data-darkhold-signed-in` body flags so the
injected bar keeps its auth-aware contract.

**Verify (repo):**

```sh
cd /home/alvaro/code/rust/darkhold
cargo test -p darkhold-app --test route_inventory
```

Expected: no new route; 6 passed (as recorded in `docs/plans/roadmap-remaining.md` for this target).

**Verify (public host, after deploy):**

```sh
curl -sS https://darkhold.polarisocial.xyz/ | grep -c 'polaris-chrome'
```

Expected: `1` (the estate bar is still present). Then in the browser at 1440 and 390, assert one
`nav[aria-label="Darkhold"]` with an `aria-current="page"` entry on `/`, `/search` and a work page,
and exactly one visible account/sign-in control. At 390 assert app nav `scrollWidth <= clientWidth`
and that adjacent links are not closer than 4px without a visible separator.

---

## UIR-2 — Availability and evidence on every work row

**Files:** `crates/app/src/routes/landing_page.rs`, `crates/app/src/routes/search_page.rs`,
`frontend/src/lib/components/WorkRow.svelte`, `frontend/src/lib/components/WorkCard.svelte`,
`frontend/src/lib/components/WorkCard.test.ts`, `frontend/src/lib/components/WorkRow.test.ts`.

The landing page already knows whether a title can be opened: `Starter.href: Option<String>` is
`None` when no readable unit exists. Turn that fact into words. Add this pure helper beside
`grouped()` in `landing_page.rs`:

```rust
/// A row says whether the text is held here. `None` is never rendered as a blank.
fn availability(href: &Option<String>) -> &'static str {
    match href {
        Some(_) => "Readable here",
        None => "Reference only · text not held",
    }
}
```

Add one more span inside the existing `.meta` div after the `meta-state` span:

```rust
page.open("span", &[("class", "meta-availability")])
    .text(availability(&starter.href))
    .close("span");
```

Add `.landing .shelf-list .meta .meta-availability::before{content:'·';margin-right:var(--space-3);color:var(--text-secondary)}`
to the inline `<style>` next to the existing `.meta-state::before` rule. Do not colour-only signal
it; the words are the signal.

For `search_page.rs`, reuse the source projection already added by M45-75 (`source_key`,
`source_updated_at`) rather than inventing a new column: print `Source: <key>` when present and
`source unmeasured` when absent, and keep `Status:` as a separate word. For the two Svelte row
components, add the same two strings as an `availability` prop with a default of `unmeasured`;
a missing prop must render the word, never an empty span.

**Verify:**

```sh
cd /home/alvaro/code/rust/darkhold
cargo test -p darkhold-app --test landing_page
cargo test -p darkhold-app --test search_page
cd frontend && npm test -- src/lib/components/WorkCard.test.ts src/lib/components/WorkRow.test.ts
```

Expected: all three commands exit 0; each card names the first line of real output. Mutation: set
one fixture's `href` to `None`, require the availability test to fail, restore.

---

## UIR-3 — Work dossier anatomy

**Files:** `crates/app/src/routes/work_page.rs` (facts/status block, lines 145–212),
`frontend/src/lib/components/WorkCard.svelte`, `frontend/src/lib/components/WhyRecommended.svelte`
(existing reason rendering), `docs/spec.md` §15.6 (work-page contract).

Add four labelled blocks to the server-rendered work page, in this order, using the existing
`Html` builder and the facts already computed in `work_page.rs`:

1. `Work facts` — fixed rows: source, last source update, words, parts, completion, rating,
   warnings, relationships, characters, availability, evidence. Unknown prints `unmeasured`.
2. `Choose this if` / `Avoid this if` — three bullets each, derived from existing facts
   (completion, warnings, length). Do not invent taste claims.
3. `Biggest unknown` — one sentence naming the missing fact; if nothing is missing, say so.
4. `Export work` — link to the existing export route; do not build a new export format.

Keep `WhyRecommended.svelte` as the reason surface; do not duplicate reason text in the dossier.

**Verify:**

```sh
cd /home/alvaro/code/rust/darkhold
cargo test -p darkhold-app --test work_page
cargo test -p darkhold-app --test route_inventory
```

Expected: exit 0; quote the first line. Then fetch one live work URL and assert the page text
contains all four block labels and the string `unmeasured` where a fact is absent.

---

## UIR-4 — Constrained search and named recovery

**Files:** `crates/app/src/routes/search_page.rs` (form and result loop, lines 80–144),
`crates/app/tests/search_page.rs`, `crates/app/tests/m71_15_search_state.rs`,
`docs/spec-amendments/faceted-search-brief-2026-11-26.md`.

Keep the single input that works with no script. Add a visible `<details>` named `Constraints`
with the existing closed vocabulary: fandom, relationship, completion, length, warning, source,
language, availability, reading status, and sort. Every control is a named GET field so the URL
holds state; no JavaScript is required. Sort values stay exactly `for-you|new|updated|top|trending|best-match|az|random`;
an unknown value must be named in a 422-style sentence, not silently replaced. Zero results must
print `No works match these constraints. Remove the last constraint.` with the narrowest active
constraint named in the sentence.

Do not add a new search backend: `search_works_ast_scoped` at `search_page.rs:100` is the existing
query and its `SearchFilters::default()` argument is where the parsed constraints are threaded.
Check the signature before editing; do not invent a filter field.

**Verify:**

```sh
cd /home/alvaro/code/rust/darkhold
cargo test -p darkhold-app --test search_page
cargo test -p darkhold-app --test m71_15_search_state
```

Expected: exit 0. Live: `/search?q=mystery` returns 200 with a count and at most 50 rows;
`/search?q=mystery&sort=nope` names `nope`; `/search?q=<impossible>` shows the named recovery.

---

## UIR-5 — Honest state copy

**Files:** the route file that owns each surface (`landing_page.rs`, `search_page.rs`,
`work_page.rs`, `reader_page.rs`, `arena.rs`, `library.rs` equivalents), plus
`frontend/src/lib/components/EmptyState.svelte` and `SignInGate.svelte`.

Use the exact sentences in the amendment's **Copy to carry over** section. Rules: no `locked`,
no `coming soon`, no empty box where data is absent, and no `0` standing in for an unmeasured
value. A module that is off is absent, not disabled-but-visible. A sign-in gate says why it needs
an account and offers one next action.

**Verify (grep-style, per surface):**

```sh
cd /home/alvaro/code/rust/darkhold
grep -R -n 'coming soon\|locked' crates/app/src/routes frontend/src/lib/components | head -20
```

Expected: no product-facing hits (test fixtures and historical docs may match; name any hit and
say why it is not user-facing).

---

## UIR-6 — Live proof before closing

Do not close any card on a unit test alone. After deploy, run the same DOM probe used for the
comparison (saved at `~/.hermes/profiles/coding/cache/scratch/dh-compare/probe.js.txt`; copy it into
`scripts/site-audit/probe.js` if it becomes a repo gate) at 1440×1000 and 390×844 on `/`,
`/search?q=mystery`, one work page, `/arena`, `/library`. Record for each: document overflow,
non-inline controls under 44px, contrast failures with composite backgrounds, `h1` count, skip
link, availability strings, and Darkhold nav active state. Validate the probe with the 1.10:1
canary before trusting a zero. Then state which claims were measured and which were only looked at.

**Expected:** 0 page overflow; 0 standalone controls under 44px; 0 contrast failures; availability
words present on every work row; one active site nav. Any deviation is a new row, not a footnote.

## Definition of done

- UIR-0a/0b fixed with mutation evidence, not just a green test.
- UIR-1..5 each have a real command output pasted into its card.
- UIR-6 has a dated JSON+PNG set and the probe canary is proven.
- `docs/DECISIONS.md` or the ADR index points at ADR 0035; no doc still says "rewrite the frontend".
- No code was added to XW-1 from this plan; the experiment gate is unchanged.
- Commit is doc-only until an owner explicitly starts execution; builds/deploys are reported separately.

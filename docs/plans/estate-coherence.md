# Estate coherence — making four products look like one maker's family

Written 2026-11-08, after reading `ascend`, `ficnexus`, `kindred` and this tree side by side. It exists because
"darkhold is finished" is a narrower claim than "the estate makes sense": four products now share a host, a
domain, a topbar and a maker, and they do not share a design system, an error envelope, a health contract or a
vocabulary. This file is the survey (measured, with file paths), the decisions, and the rows that follow — the
work items are `M72-*` in `docs/requirements.csv`.

## 1 — What the four products actually are

| | darkhold | ascend | kindred | ficnexus |
|---|---|---|---|---|
| job | archive + reader + recommendations | draw a character, share the legend | a reading companion over an AO3 snapshot | export a work as EPUB/HTML/… |
| UI stack | SvelteKit `adapter-static` SPA | SvelteKit `adapter-node` | Svelte 5 + Vite, no kit | **none** — API only |
| Svelte files | 103 | 40 | 14 | 0 |
| design system | `@polaris/ui` 0.1.2 (its only consumer) | Tailwind 4 `@theme`, dark-only | hand-written CSS reproducing AO3's grammar | — |
| API root | `/api/v1` | `/api` | `/api/v1` | `/api` |
| error body | `{"error":{code,message,request_id}}` | `{"error":{code,message,details}}` | `{"error":{code,message}}` (typed `code()`) | `{"err":-1,"msg":"…"}` |
| meta | `/api/v1/meta`: `name`, `version`, `build`, `api_version`, `environment`, `base_url`, `policy` | `/api/meta` only — `/api/v1/meta` is 404 | none: `/api/v1/meta` is 404; `/api/v1/health` carries the index and budget numbers | `/api/meta` |
| health | `/healthz`, `/health/live` | `/healthz` at the root → `{"status":"ok"}` | `/api/v1/health` (index + budgets) | `/api/health` |
| accounts | OIDC (polaris-id) + session cookie + bearer | own session cookies + CSRF | **deliberately none** | none |
| unit / ports | `darkhold.service` 8081 → 8008 | `ascend-api.service` 8024 → 8023 | `kindred-rs.service` 8030 → 8009 | — |
| work URL | `/works/{id}` | `/fics/…` | `/works/{id}` | — |

Two of these are deliberate and should stay:

* **ficnexus has no frontend.** It was reduced on purpose (`docs/EXPORTER-SCOPE.md`: "there are no accounts, no
  bookmarks, no reader… those live in darkhold"). Its coherence obligation is the *contract*, not the pixels.
* **kindred reproduces AO3's visual grammar** on purpose (`web/src/styles/kindred.css` opens by citing
  `otwarchive/public/stylesheets/site/2.0/`). A reader who lives on AO3 should recognise the page. Homogenising
  kindred into the Archive skin would delete a product decision, not fix a defect.

So "same family" cannot mean one skin. It has to mean one **grammar**: the same components with the same
behaviour, the same tokens with different palettes, the same JSON contracts, the same words for the same nouns.

## 2 — What already exists, and is not being used

The estate is further along than the four repos suggest:

* `estate/ui` — `@polaris/ui` 0.1.2: `Button`, `Card`, `Field`, `Tag`, `Empty`, `Onboarding`, a `tokens.css`,
  palettes, `a11y.mjs` (contrast), `docs/census.md`.
* `estate/ui/docs/census.md` — a measured census of **24 primitives** across eight frontends, with a chosen
  source for each and an adoption column. It names six primitives that exist nowhere: checkbox/radio, avatar,
  breadcrumb, tooltip, `kbd`, and the card frame's action row.
* `estate/ui/src/chrome/topbar.js` — a framework-free web component **appended to every site by nginx**:
  shadow root, degrades to nothing if `/estate/nav.json` or `/api/v1/capabilities` is missing.
* `estate.toml` + `generated/nav.json` — every site's hostname, ports, unit, tier (`sfw`/`nsfw`), group
  (`content`/`social`), status and tagline. darkhold, kindred, ascend are all in it.

And yet: **darkhold is the only consumer of `@polaris/ui`**, and it consumes it through a path dependency —
`"@polaris/ui": "file:../../../estate/ui/dist/polaris-ui-0.1.2.tgz"` — which resolves only on this machine,
with the estate checkout in that exact relative position. A clean clone of darkhold cannot install. The same
tarball is the only distribution channel, so ascend and kindred cannot adopt a version even if they wanted to.

That is the first row (M72-01): nothing else in this plan is adoptable until there is a registry to adopt *from*.

## 3 — Four layers, in dependency order

**Layer 1 — distribution.** `@polaris/ui` publishes to the estate's Forgejo npm registry (`estate.toml`
`[registry] npm`), semver, one version pinned by every consumer. Adoption is then a `package.json` line rather
than a filesystem coincidence.

*Done 2026-11-08 (M72-01):* `@polaris/ui@0.1.2` and `@polaris/inbox-client@0.1.1` are published; this tree
depends on `^0.1.2` / `^0.1.1` through `frontend/.npmrc`'s `@polaris:registry`, and a clone with no `estate/`
sibling installs and builds. Two things the exercise taught: the token is per-machine (`~/.npmrc`, or
`NPM_CONFIG_USERCONFIG`) and never in a tracked file; and `frontend/pnpm-workspace.yaml` still held pnpm's
unanswered `allowBuilds` placeholder, so every install exited non-zero and a clean clone installed without ever
linking esbuild. ascend and kindred pin when they adopt (M72-06) — a dependency nobody imports is not a consumer.

**Layer 2 — tokens, palettes as data.** *(Done 2026-11-08, M72-02: `@polaris/ui@0.2.0` ships six palettes —
reading-room, after-hours, clear-day, ascend, ao3 — resolved to literal values at build time and contrast-checked per
palette (0 failures over 6 × 84 tokens). Light mode is a written ramp, not `100 - L`. A palette is also allowed
`tokens` overrides, so an identity that already ships is recorded exactly as hex rather than re-invented in a hue;
darkhold's three presets are now those overrides, and darkhold's `tokens.css` keeps its `--color-*` names as aliases.
The gate found two real defects on the way: darkhold's control boundary was 1.8:1 (WCAG 1.4.11 wants 3:1) and its dark
preset had no dim ink at all. ascend's and kindred's palettes are measured but not yet adopted — that is M72-06.)*
**Layer 2 — tokens, palettes as data.** One token vocabulary (`--polaris-*`: surface, ink, ink-soft, line,
accent, ok/warn/err, radius, space, type scale), three palettes that are *values* of it: darkhold's
Archive/Reading Room, ascend's dark, kindred's AO3. The contrast report (`ui/a11y.mjs`) runs over every palette
in CI, so a palette that fails 4.5:1 on body text cannot ship. This is what lets kindred stay AO3-shaped while
still being recognisably from the same hand: same rhythm, same focus ring, same spacing scale, different paper.

**Layer 3 — components, ported not invented.** *(The six that existed nowhere are built in `@polaris/ui@0.3.1` —
M72-03 — and adopted here in WorkCard, Account, Docs and VoteBudget; the census's remaining ports (textarea, select,
modal, tabs, table, menu, badge, skeleton, banner, toast) wait for their adoption wave, since a port nobody renders is
dead code.)*
 The census's 24 primitives are the list. Port the six that do
not exist, adopt the five that do, and **prove adoption by deletion** — the local copy is removed in the same
commit, or the product is running two design systems and the plan is not done.

**Layer 4 — contracts.** One error envelope, one meta/health shape, one noun vocabulary. This layer is
invisible to a human reading the page and yet it is what makes the products feel like one thing to anyone
scripting them (`dh`, the estate probes, the uptime board) — and it is what makes cross-product features
(a "more from this maker" rail, a shared work blurb, one search box) cheap instead of repeated.

## 4 — Decisions

**A family shares grammar, not skin.** Rejected: one palette for all four. It would delete kindred's reason to
exist (a reader's AO3 reflexes), make ascend's rarity colours arbitrary, and force darkhold's Archive direction
onto products with different jobs. What must match: component anatomy and behaviour, spacing scale, focus
treatment, motion budget, empty/error/loading language, URL shape for the same noun.

**Port, never invent a sixth.** The census already names a source for each primitive. A product that needs a
control adds it to `@polaris/ui` or files the row — it does not inline a fourth variant.

**The estate topbar is chrome, not navigation.** It already renders on every page. Making the four products
feel like siblings is not a matter of adding a second bar; it is a matter of the bar naming *relationships*
(group and tier already in `nav.json`) and of one component rendering "more from this maker" instead of four
hand-written footers.

**ficnexus gets contracts, not pixels.** No design system, no tokens: its row is `/api/v1`, the envelope, and
the meta shape — because `dh` and the download UI both call it.

**kindred's no-accounts stance is a decision, not a gap.** Its session lives in `localStorage` and never leaves
the machine (`web/src/lib/session.js`, "R3"). The auth row is therefore a *matrix* that records each product's
answer, not a mandate to bolt accounts onto a tool that deliberately has none.

## 5 — Sequencing (this is why M72 comes before the rest of the roadmap)

1. **M72-01** registry, and `@polaris/ui` 0.2.0 with the token vocabulary (M72-02 in the same release).
2. **M72-03** the six missing primitives, inside that release.
3. **M72-04/05** the JSON contracts (darkhold already matches the error envelope; kindred and ascend align;
   ficnexus keeps `err/msg` behind a documented alias for one release).
4. **M72-06** adoption in darkhold → kindred → ascend, each proved by deleting the local copy.
5. **M72-07** the family nav module, once the components it needs are shared.
6. **M72-08/10** the auth matrix and the noun/URL vocabulary — decisions plus their small code changes.
7. **M72-09** the design review that keeps it from drifting: it is last because it reviews what 1–8 produced,
   and it is a gate rather than a document.

## 6 — How each row is proved

Every row carries the same shape as the rest of this ledger: a command, a mutant where the row asserts a
behaviour, and deletion as the adoption proof.

* Token/palette rows: `node estate/ui/a11y.mjs` reports contrast for **every** palette; a mutant that sets body
  ink to 4.4:1 must fail.
* Component rows: the product's local file is **gone** (`git log --diff-filter=D`), and the page still renders —
  checked by a headless render, not by a unit test of the component in isolation.
* Contract rows: one test per service asserting the exact body shape, plus a live probe of all three on this
  host (`8081`, `8024`, `8030`) with `jq -e` on the same expression for each.
* Family-nav row: block `nav.json` and the module must render nothing (the topbar's own contract); serve a
  filtered nav and it must show exactly the siblings in the same group and tier.

## 7 — What this plan does not do

* It does not merge the products into one binary. They have different jobs and different deploy units; the
  shared thing is the kit, not the process.
* It does not unify the databases. `works` means the same noun in darkhold and kindred and they are still two
  tables in two clusters; a shared *vocabulary* is not a shared schema.
* It does not restyle kindred or ascends' colour to match darkhold's Archive direction.
* It does not touch `gravity`, `suki`, `whitebois`, `streakforge`, `orbital` or `commons/ui` beyond what
  `@polaris/ui` gains — the census names them as sources, and a source that is copied once is enough.

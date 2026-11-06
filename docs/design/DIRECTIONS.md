# Three visual directions

Decided in ADR 0007. Each direction is described by the same eight dimensions so they can be compared rather than
admired. The winner is *The Archive* for site chrome, with *The Reading Room* retained for the reader surface and one
signature accent from *The Forbidden Section*. The losing directions are kept because a direction that is not written
down gets re-proposed every six months.

All three are bound by the same laws: WCAG 2.2 AA in every preset (4.5:1 body and reading text, 3:1 boundaries and
large text, measured including on-accent text), no motion beyond 150 ms state transitions, no shadows, no imagery in
chrome, and the reader's type scale independent of the site's.

---

## A. The Reading Room (incumbent)

**Register.** A private library after hours: paper, lamplight, quiet. The product as a place to sit.

**Palette.** Warm paper base, ink text, one muted accent for actions. Three polish presets already shipped.

**Type.** Serif headings (Source Serif 4), humanist sans for interface (Source Sans 3), Literata for prose.

**Layout.** Generous. 8-step spacing, wide gutters, cards with breathing room, 1.5–1.8 line height everywhere.

**Component shapes.** Rounded cards, pill tags, comfortable buttons. Everything is a container.

**Strength.** The reader feels like reading. Nothing in it fights prose.

**Failure.** It is the same shape as every other polarisocial site and as a literary blog. At 141k rows a card grid
is not calm, it is slow: three rows per screen, no counts, no density. It expresses "books" but not "the dark side",
and it has no vocabulary for provenance, which is the one thing this catalogue must say on every row.

**Verdict.** Keep it for the reader, where it is correct. It cannot carry the catalogue.

---

## B. The Archive (chosen for site chrome)

**Register.** A restricted collection's finding aid: index cards, catalogue numbers, accession stamps, marginalia in
pencil. The product as a *collection* with rules, not a shelf of favourites.

**Palette.** Paper base with a cooler grey for metadata; one ink accent for action; provenance, rating and status get
*fixed* hues that never change with the preset, because they are information rather than decoration.

**Type.** Interface and headings in a tight humanist sans at small sizes; **metadata in monospace** (counts, word
counts, dates, ids, hashes) so numbers line up in columns; prose untouched by the direction (the reader keeps its own
scale).

**Layout.** Dense rows, not cards. A row is a line of a catalogue: provenance mark at the left edge, title, author,
fandom, counts, status, date. Column alignment across rows is the design. Facet groups are drawers with counts.
Whitespace is deliberate at section boundaries and tight inside a row.

**Component shapes.** Square-ish corners (2px), hairline dividers instead of containers, stamps instead of badges: a
provenance mark is a small letter-spaced word with a rule, not a coloured pill. Buttons are compact and rectangular;
primary actions are the only filled element on a page.

**Strength.** It answers the two facts the incumbent ignores — 141k rows, and most rows are not readable here. Density
becomes the aesthetic rather than a compromise. It is unmistakably not a dashboard, and it scales to the console
without changing register.

**Failure.** Can read as bureaucratic if the typography is timid; needs a strong title scale and a confident accent or
it becomes a spreadsheet. Also demands discipline: one stray 24px padding in a row breaks the column rhythm.

**Verdict.** Chosen. It is the only direction whose *structure* carries the product's central claim.

---

## C. The Forbidden Section (signature only)

**Register.** The shelf behind the rope: redaction bars, specimen labels, a single hot accent, high contrast.

**Palette.** Near-black ground, bone text, one saturated accent (oxide red or sodium amber) used at most twice per
screen.

**Type.** Condensed display caps for labels, everything else quiet; strong horizontal rules.

**Layout.** Asymmetric, wide margins, big empty fields punctuated by dense blocks of metadata.

**Component shapes.** Bars, rules, labels that look stamped. Rating and provenance marks as letterpress blocks.

**Strength.** The strongest identity of the three. Instantly recognisable, and it says "dark side" without a single
image.

**Failure.** As a whole-site language it is a grimdark game HUD: it taxes long reading, it ages badly, and it makes
ordinary tasks (settings, library) feel theatrical. It also fights the product's own claim that reading is calm.

**Verdict.** Rejected as a language, adopted as an accent: the provenance/rating/status marks and the console's
critical red may borrow its letterpress treatment, nothing else.

---

## What the winner requires, concretely

| Layer | The Archive |
|---|---|
| Tokens | add `--density-row`, `--provenance-*`, `--rating-*`, `--status-*`; keep `--reader-*` separate |
| Type | metadata scale at `--text-xs`/`--text-sm` with `--font-mono` for counts and dates |
| Rows | one row component, three provenance variants, fixed column grid, hover/focus reveals actions |
| Badges | letter-spaced word + rule; colour is reinforcement, never the only signal |
| Empty states | nothing renders (absent), or a named sentence; never a titled empty box |
| Console | same row language, monospace values, status word + dot, alert bar with a fix link |
| Reader | unchanged, except the bar and typography panel adopt the row's type scale for chrome |

Every one of those is a component or a token, not a page rewrite — which is why the direction can be adopted
surface by surface while the renderer migration (ADR 0006) proceeds independently.

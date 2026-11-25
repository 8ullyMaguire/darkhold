# Faceted-search brief triage — 2026-11-26

**Status:** accepted with triage · **Source:** owner-pasted outside brief
**Authority:** `docs/requirements.csv` (1,039 rows) and `docs/spec.md`; this note adds only what the census says is missing.

## Rule for outside critiques

An outside brief is a claim set, not a fact set. Every sentence describing the current product was checked against
the requirement ledger and the spec before it was allowed to shape work. The brief's structural advice is sound;
its implicit claim that Darkhold is a CRUD archive with no discovery vocabulary is wrong.

## Already covered — do not re-propose

| Brief item | Existing evidence |
| --- | --- |
| Boolean/operator search, exact phrase, OR, wildcard, filters in URL | `M10-02` (advanced boolean/facet query language), `M69-04` (grammar as stable reference), `M45-46`, `M59-04`, `M66-09` |
| Saved searches, named views, alerts/subscriptions | `M47-05`, `M21-D12`–`D15`, `M63-12` (`dh watch`), `M15-01`, `M15-08` |
| Facet counts under the current filter set | `M71-11` — one server endpoint, latency budget, UI renders counts per group |
| Completion, word count, rating, warnings, language, dates | `M64-10` makes length/status/rating/cadence first-class; `M10-02`, `M10-04`, `M45-30`, `M45-31` |
| Canonical tags, synonyms, wrangling | `M29-03` (propose/approve/reject/revert), `M64-06` (normalised taxonomy + synonym mapping), `M45-70` |
| Recommendations and reasons | `M11-01`, `M11-05`, `M29-01`, `M47-04`, `M52-09`, `M64-01`, ADR 0006/0033 |
| EPUB/PDF downloads, reading position, themes | `M7-01`, `M63-05`, `M57`, `M64-10`, `@polaris/ui` palettes in `frontend/src/styles/tokens.css` |
| Search within a work | `M10-03` |
| Tag pages and directory browsing | `M8-01`, `M12-04`, `M12-06`, `M71-02` |

## Missing — add to the roadmap

### FS-01 — crossover as a first-class include/exclude facet

`docs/spec.md:2780` and `:6045` name crossovers as a filter dimension; the requirement ledger has no row that
implements it. Add `crossover:include|exclude|only` to the query grammar, expose it as a facet with counts derived
from the current filter set, persist it in every shareable filter URL, and surface it in the advanced filter panel.
Canonical fandom tagging already exists; this is a query/UI addition, not a new taxonomy.

Acceptance: a dual-engine test creates a crossover and a single-fandom work; `crossover:exclude` returns only the
single-fandom work, `crossover:only` returns only the crossover, and a URL round trip keeps the clause. Mutation:
dropping the crossover predicate must fail the test.

### FS-02 — role metadata on tag joins

`docs/spec.md:2780` names character prominence and relationship roles, but no requirement stores them. A bare tag
list cannot answer “is this the main pairing?” Add optional `main_relationship`, `side_relationship`, `pov` and
`primary_character` roles to the work↔tag join, map them from existing importers where the source states them,
and leave the role empty when the source is silent. Never infer a main pairing from tag order or popularity.

Acceptance: a dual-engine test stores one main and one side relationship on the same work; a query for main
relationships returns one row and a query for side relationships returns the other; an untagged role stays null.
Mutation: collapsing both roles to a plain tag must fail the test.

### FS-03 — equal include/exclude treatment on tag chips

Exclusions exist in the query engine and content filters (`M47-04`), but no requirement makes include/exclude a
toggle on every facet chip. Add the toggle to every tag, fandom, relationship and character chip, keep the active
filter list readable, and let one click flip a clause from must to must-not without rebuilding the whole query.
Counts beside each chip remain those for the current filter set (`M71-11`).

Acceptance: DOM test flips one chip and asserts the query string changes from `tag:x` to `-tag:x` (or the grammar's
named equivalent) and back; the chip's count does not become the unfiltered global count.

## Parked — explicit, not forgotten

- **External OpenSearch/Redis/S3 split.** The brief recommends it as an architecture; this instance has one corpus,
  a working dual-engine query path, and no evidence of the scale that justifies an index cluster. Revisit with a
  measured p95 and facet cardinality, not as a precondition for the three rows above. `search_after` deep paging is
  part of that revisit.
- **Human tag wranglers.** `M29-03` already gives curators propose/approve/reject/revert. Staffing wranglers is an
  owner decision, not a schema gap.
- **Podfic/audio availability as a filter.** `M40-02` covers narration/permission mechanics; no live supply is
  measured. Add a facet when narrations exist.

## Refused — record so it is not re-proposed

- Popularity-only ranking and kudos-to-hits as the default sort: ADR 0006 and `M64-01` require named algorithms with
  empty-data refusal and reason transparency.
- Full-text chapter bodies indexed by default: privacy and preservation rules permit opt-in bodies only.
- “Supports every site” scraper claims: `M53-04` and the canonical scraper adoption record require named sources
  with measured health.

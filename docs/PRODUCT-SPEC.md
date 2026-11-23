# Darkhold product specification — the short canonical layer

Decision 2026-11-23 (delegated best judgement). Review C1 measured 98,832 words of specification against 866 source
files and found that the spec had become a second product. The repository already has a requirements CSV and 26 ADRs;
this document makes their roles explicit so a new change does not have to edit a 659 KB narrative.

## 1. Product

A self-hosted home for fanfiction: read without an account, write without a publishing queue, import the library you
already have, and take it offline. The north star is quality fanfiction the operator would actually enjoy reading —
more of it written, the right stories reaching the right reader, and trust machinery that keeps both working.

Ascend is the narrower sibling for progression fiction with a system. No shared database, no cross-service calls.

## 2. Source-of-truth map

| Document | Canonical for | Not canonical for |
|---|---|---|
| `docs/PRODUCT-SPEC.md` (this file) | product identity, goals, non-goals, source-of-truth rules | implementation status, architecture decisions |
| `docs/requirements.csv` (982 lines) | requirement ids, milestone, status, evidence | architectural reasoning, prose rationale |
| `docs/adr/*.md` (26 files) | decisions and rejected alternatives | feature inventory |
| `docs/goal.md` | standing build brief and per-change gates | product scope |
| `docs/spec.md`, `docs/spec-amendments/`, `docs/archive/` | historical narrative and design discussion | current status or build order |
| `docs/measurements/*.md` | measured facts with commands and timestamps | requirements |

When these disagree, requirements status wins for status, an ADR wins for a decision, and this document wins for scope.
A prose sentence in `docs/spec.md` cannot create a new requirement or overrule either.

## 3. Active scope

- Reader: authenticated and anonymous reading, offline reading, accessible rendering.
- Writer: draft-to-publish path with no editorial queue; author-controlled updates and withdrawals.
- Importer: one source at a time with provenance, revision cache and an operator-visible queue.
- Discovery: tags, search and taste surfaces where the manifest enables them; no claim that `taste`/`recs` are on.
- Trust: pseudonym isolation, consent, moderation and audit trails for the surfaces that exist.
- Federation: only the implemented ActivityPub surface; see `docs/federation-status.md` for measured state.

## 4. Non-goals for this split

- This is not a rewrite of `docs/spec.md`. The narrative stays as history; nothing is deleted.
- This is not a new requirements ledger. `docs/requirements.csv` already holds 982 lines; do not add a second one.
- This is not a claim that every requirement in the CSV is implemented. Status lives in the CSV, not here.
- This is not a new ADR. Decisions already made stay in `docs/adr/`; a new architecture decision gets a new numbered ADR.
- This is not a product pivot: Ascend, Lorebook and the extension remain separate products with their own docs.

## 5. Measured context for this decision

- Imported share of the live corpus: 57,225 of 58,042 works have import provenance (98.59%); 817 have none.
  Measured read-only on 2026-11-23, recorded in `docs/measurements/corpus-origin.md`.
- Dual-engine migration tax: 214 Postgres + 214 SQLite migration files, 185 divergent pairs, 21,252 lines.
  Recorded in `docs/measurements/migration-tax.md`. Postgres is primary; SQLite stays frozen at the current schema.
- These measurements are facts, not requirements. A requirement that depends on them must name its evidence command.

## 6. Change rule

1. A new product capability starts with a row in `docs/requirements.csv` and a pointer to its evidence command.
2. A new architectural choice gets a numbered ADR with the rejected alternative named.
3. This document changes only when the product identity or a non-goal changes. It should stay short.
4. `docs/spec.md` may be amended for explanation, but an amendment alone never changes status.
5. A measurement that changes a decision is written under `docs/measurements/` with the exact command and output.

## 7. Acceptance for this split

- `docs/PRODUCT-SPEC.md` exists and is linked from README.
- `docs/requirements.csv` and `docs/adr/` are named as canonical for status and decisions.
- No requirement row, ADR or source file was changed by this document.
- A future review asking "where is the spec?" is answered by §2 in under a page.

This document is a specification split, not a compliance claim, build, test or deploy.

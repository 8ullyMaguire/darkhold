# Triage of the 96 planned rows

Method: every one of the 96 was read as `(id, milestone, requirement)` and checked against the tree by name —
the function, table or document the requirement describes. Three outcomes are recorded below: rows that were already
done and never recorded, rows that are documentation decisions, and rows that are real systems. Nothing here is a
promise; each line names the evidence that put it in its bucket.

## Closed by triage (no new code)

| Row | Why it was already satisfied | Evidence |
|---|---|---|
| M45-58 — erasure reaches `supply_routing` | `erasure::UnlinkedColumn { table: "supply_routing", column: "account_id" }` rewrites the link to `erased:<row id>`; the row survives (§58.3) and the account leaves (§51). | `crates/db/tests/supply_routing_erasure.rs` 4/4 and `erasure_covers_fk_free_accounts` 3/3, both green 2026-11-06 |
| M71-02 — three directions, one chosen in an ADR | The deliverable is the writing plus the decision. | `docs/design/DIRECTIONS.md`; `docs/adr/0007-the-design-system-is-owned-the-identity-is-not.md` |

The lesson from M45-58 is worth keeping: a row is not "planned" because nobody did it, but because nobody wrote it
down. Before implementing a row, grep for the thing it names.

## Closed by implementation this batch

| Row | What was missing | Evidence |
|---|---|---|
| M71-16 — machine surfaces obey page eligibility | No sitemap existed at all, and the audience enum had no honest word for a surface that takes no session. | `4a7f8839` |

## Already substantially present, missing one named thing

These are the cheapest remaining wins: the hard part exists and the row asks for a specific finishing behaviour.

| Row | Present | Missing |
|---|---|---|
| M64-01 — surfaces choose 2–4 named algorithms | `db::rec_strategy::RecRegistry` with strategies, RRF blending and per-strategy traces (`RecRunReport`); `GET /browse/surfaces` lists eight surface keys. | Per-surface algorithm selection read from config, and a **named** refusal when a chosen algorithm's data is empty rather than an empty result. |
| M69-08 — one blend with decay and a floor | `domain::discovery::blend()` merges engine candidates deterministically and is used by discover, draw, concierge and rec_run. | Interaction decay and a score floor; the current sum has neither. |
| M71-06 — every recommendation carries a reason | `Candidate.reason` is on the domain type and `blend` preserves the first engine's reason. | The surfaces do not render it, and there is no closed vocabulary for reason sentences. |
| M69-09 — affinity as a stored 0..1 score | `taste_profiles` with signals, `operator/affinities`, affinity reordering in discovery. | Nightly recomputation and the 0..1 stored column, rather than a per-request read. |

## Documentation-decision rows (the deliverable is prose, or prose plus a config knob)

| Row | Shape |
|---|---|
| M45-54 — RPF instance policy knob; Spain honour/image rights | Config key + ADR; no runtime behaviour beyond the switch. |
| M69-26 — platform default / operator override / reader personalisation | The ladder exists in `settings`; the row asks for the bounds stated once, in prose. |
| M71-13 — coverage statements on partially covered dimensions | Needs the counting query first; the statement is then a template. |

## Real systems (not a turn each, and several are not a session each)

* **M16-06…14** — the extension sandbox: wasmi, fuel, memory ceilings, host-call limits, review workflow, gallery,
  marketplace, themes, operations. Six rows, one system.
* **M64-02…15** — the experiment platform and nine algorithms. Fourteen rows; M64-02 and M64-03 are the foundation and
  nothing else in the milestone can be honest without them.
* **M45-39…46** — instance death, curator views, attribution, claims, Discord, sister instances, taste governance,
  preview. Eight rows of federation.
* **M69-02…07, 10…24** — passports, mirroring, rescue rings, evacuation, Glicko-2, trust velocity, three-layer
  filtering, saved filters.
* **M63-xx** — the CLI surface: thirteen subcommands, most independent of each other.

## The order this implies

1. The four "missing one thing" rows — each is a contained change with an obvious test.
2. The documentation-decision rows — prose plus, at most, one config key.
3. M64-02/M64-03, because every other M64 row reads their tables.
4. M16-06…12 as one system, in one place, rather than row by row across milestones.
5. Everything else as capacity allows.

Enumerated in `docs/plans/roadmap-remaining.md`; this file is the reasoning, that one is the sequence.

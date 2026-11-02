# 61. Cross-language supply: fund a translation, and report the gap honestly

**Status:** reserved
**Milestone:** —
**Source:** §61 of `docs/spec.md`

Adopted 2026-11-02 (M45-30). The gaps review's C6 asks for three things in one sentence: *taste-score
foreign works (tags map across languages; stylometry partially transfers), then fund translation
bounties with author permission.* Two of those three are not what this section builds, and saying
which is the point of writing it down.

## 61.1 The report is a count, and the transfer half is a refusal

`GET /api/v1/translations/supply` answers with the languages this instance holds:

```json
{
  "by_language": { "en": 58042 },
  "total": 58042,
  "unrepresented": ["pt", "es", "fr", "ru", "zh", "ja", "ko", "id"],
  "transfer": { "available": false, "missing": ["stylometry", "tag backfill (M63-32)"] }
}
```

- Counting is by **primary subtag**, lowercased: `pt-BR`, `pt-br` and `pt` are one bucket. A report
  that splits Brazilian from European Portuguese before it holds either is a report about tags rather
  than about readers.
- A tag that is not a language (`""`, `"12"`, a sentence) is counted as `unknown` rather than dropped,
  so `total` always equals the number of works.
- `unrepresented` is computed, not hard-coded prose: it names the languages C6 cares about that the
  instance has **nothing** in.
- `transfer.missing` names the two measured gaps. `grep -rln stylometr crates/` finds a catalogue
  string and nothing else, and `work_tags` is not backfilled (M63-32). A cross-language taste score
  over that would be a number with no signal behind it, so the door refuses in words and returns
  `available: false` — never a plausible `0.5`.

## 61.2 Funding is gated by §40.2, before any money

`POST /api/v1/works/{id}/translation-bounty` with `{ "target_language", "amount" }` writes a row in
the existing `bounties` table with `job_kind = 'translation'` and terms naming the work, its own
language and the target. It does not invent a second funding path: `create_bounty` is §20.3's.

The gate is the work's `translation` statement, read through §40's model:

| statement | outcome |
|---|---|
| `yes` | funds, if the balance covers the amount |
| `ask` | **refused** — the author must answer first; funding now would spend their silence |
| `no` | refused, naming `no` |
| `unstated` | refused, naming `unstated` |

`unstated` refuses because C6 says *author permission required*, and §40's default is the restrictive
reading. Two more refusals: an empty or unusable language tag (`target_language`), and a
non-positive amount (`amount` — a bounty is a promise of credits).

**The order is the rule: permission before money.** A refusal that named a balance first would tell a
patron to top up for something the author has forbidden, so the statement is read first and the
balance second. The balance check exists because a bounty from an account that cannot pay is a promise
the archive would have to break; the refusal names both numbers.

## 61.3 §40.2's enforcement sentence is now true

§40.2 already said *the translation pipeline (M17) checks `translation` before a request is created*.
Measured on 2026-11-02, `POST /works/{id}/translations` created the job without ever reading the
statement: the sentence was a spec claim with nothing behind it. It now refuses on the field
`translation`, with a different sentence per statement, and `ask` refuses with the honest reason —
this build does not create a request it cannot get an answer to.

Permission statements and lineage edges were **already built** (`GET`/`PUT /works/{id}/permissions`,
`GET`/`POST /works/{id}/lineage`, `derivative_lineage.kind = 'translation'`), so this section adds no
table and no migration. Three milestones in a row have needed none; the schema is richer than the
review assumed.

## 61.4 What is deliberately not here

- **Taste-scoring foreign works.** Refused in the payload, §61.1.
- **Claiming and paying a bounty.** `create_bounty` writes `state = 'open'` with
  `escrow_transaction = 'pending'`; settlement is §20.3's flow, and this row funds rather than pays.
- **The `ask` workflow.** There is no permission-request queue in this tree, so `ask` refuses with
  that as the stated reason instead of silently creating an unanswerable request.
- **Language detection.** `works.language` is declared by the importer; a detector is a different
  feature with its own false-positive story.

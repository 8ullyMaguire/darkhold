# Darkhold — feature ledger

**Generated 2026-11-24 from `docs/requirements.csv`.** This is the short answer to "what is built": the handoff is a
35,617-word history file, not a ledger. `docs/requirements.csv` remains the row-by-row authority; this file only
aggregates it and names where to look next.

## Measured counts (2026-11-24, 1022 rows, 65 milestones)

| status | rows |
|---|---:|
| blocked | 7 |
| done | 3 |
| evaluated-and-rejected | 7 |
| implemented | 52 |
| implemented-fully-tested | 461 |
| implemented-locally-tested | 269 |
| implemented-source-probe | 1 |
| implemented-targeted-green | 21 |
| implemented-verified-e2e | 172 |
| mutation-proven | 5 |
| partial | 2 |
| planned | 18 |
| unsupported | 4 |

The CSV is the source: a hand-written number here would become a second lie to maintain. Regenerate with:

```
python3 -c "import csv,collections; r=list(csv.DictReader(open('docs/requirements.csv'))); print(len(r), sorted(collections.Counter(x['status'] for x in r).items()))"
```

## Where the other answers live

- **Open work and gates:** `docs/plans/roadmap-remaining.md`, under `Current state — open work`.
- **Standing goal:** `docs/plans/next-goal.md`.
- **Optional community products M12–M20:** `docs/plans/community-products-m12-m20.md`.
- **Evidence for a specific row:** the `evidence` and `verified_at` columns of `docs/requirements.csv`; then
  `docs/verification.md` for the command output.
- **History, decisions and incidents:** `docs/handoff.md`. Read it when you need to know *why*, never to count what
  exists.

## Rule

Do not add a new "current state" list to the handoff. Add a row to `docs/requirements.csv` (with evidence) and, if it
changes an aggregate above, regenerate this file. This is a ledger declaration, not a claim that every implemented
row has passing evidence today.

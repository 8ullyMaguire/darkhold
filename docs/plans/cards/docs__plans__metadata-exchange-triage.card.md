# docs/plans/metadata-exchange-triage.md — plan card

Source: `docs/plans/metadata-exchange-triage.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

in `docs/spec.md` (§0.3, §2.3.1, §11.17, §15.17, §16.16.1, §19.14) with
requirement rows in `docs/requirements.csv` and a build order in
`docs/plans/remaining-work.md` (M57). The rejected items below stay rejected,
- **B** — this repo's `docs/spec.md`
the document. Checked with `grep -nE '^#+ *N.M' docs/spec.md`:
all exist in `docs/spec.md` §4.5 and §4.3. The substrate is not invented — which
Both prerequisites are **already built**. `docs/requirements.csv` is ground
grep -c '^## 11.17' docs/spec.md          # expect 1
grep -c '^## 15.16' docs/spec.md          # expect 1
grep -c '^## 19.14' docs/spec.md          # expect 1
grep -cE '^## 11\.(1[7-9])' docs/spec.md  # expect 1
grep -cE '^## 15\.16' docs/spec.md        # expect 1

## Copy-pasteable code / commands

# new sections, one count each
grep -c '^## 11.17' docs/spec.md          # expect 1
grep -c '^## 15.16' docs/spec.md          # expect 1
grep -c '^## 19.14' docs/spec.md          # expect 1
# no collisions introduced
grep -cE '^## 11\.(1[7-9])' docs/spec.md  # expect 1
grep -cE '^## 15\.16' docs/spec.md        # expect 1
# forbidden vocabulary: "quorum" must not describe signal counting
grep -n 'auto_quorum' docs/spec.md        # expect nothing
# code fences balanced
awk '/^

## Verification command and expected output

| Auto-created entities usable immediately | B §15.16 (renumbered) | The "usable but unverified" split is right — it avoids a gate that stalls organic growth. |
edited — re-verify with the `grep` in §"Corrections required" before citing any

## RED/GREEN evidence

direct site lookup, on the grounds that it "has passed quorum review; the site's

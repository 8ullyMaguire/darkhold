# docs/plans/periodic-deidentified-dataset.md — plan card

Source: `docs/plans/periodic-deidentified-dataset.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

**Spec:** `docs/spec-amendments/periodic-deidentified-dataset.md` (§11.16, §11.17)
   `crates/app/src/limiter.rs` builds a rate-limit key in memory and discards it
### D1.1 The inventory gate — `scripts/check-snapshot-pii.py`
    <path>        check a migrations directory (default: migrations/postgres)
python3 scripts/check-snapshot-pii.py --self-test
python3 scripts/check-snapshot-pii.py
`scripts/build-snapshot-sql.py` reads the policy and emits one `.sql` file.
-- See docs/spec-amendments/periodic-deidentified-dataset.md §11.16.3
python3 scripts/build-snapshot-sql.py --out /tmp/snap.sql
psql "$DARKHOLD_TEST_PG_URL" -f /tmp/snap.sql   # must not error
`crates/app/tests/snapshot_anonymisation.rs` — the §11.16.7 test, and the one
          python3 scripts/check-snapshot-pii.py --self-test

## Copy-pasteable code / commands

   sudo docker exec lh-m32e-pg psql -U darkhold -d postgres -tAc \
     "SELECT name FROM pg_available_extensions WHERE name IN ('postgresql_anonymizer','pgcrypto')"
   # observed: pgcrypto
   

   sudo docker exec lh-m32e-pg psql -U darkhold -d postgres -tA -c "
     SELECT v,
       (substr(encode(digest(v || 'darkhold-snapshot-v1','sha256'),'hex'),1,8) || '-' ||
        substr(encode(digest(v || 'darkhold-snapshot-v1','sha256'),'hex'),9,4) || '-4' ||
        substr(encode(digest(v || 'darkhold-snapshot-v1','sha256'),'hex'),14,3) || '-a' ||
        substr(encode(digest(v || 'darkhold-snapshot-v1','sha256'),'hex'),18,3) || '-' ||
        substr(encode(digest(v || 'darkhold-snapshot-v1','sha256'),'hex'),21,12))::uuid
     FROM (VALUES ('11111111-1111-1111-1111-111111111111'::text),
                  ('22222222-2222-2222-2222-222222222222'::text),
                  ('11111111-1111-1111-1111-111111111111'::text)) s(v);"
   

#!/usr/bin/env python3
"""Fail when a column in a PII-bearing table is absent from the snapshot policy.

The policy is a table, not a list: a column that appears in a PII-bearing table
after the policy was written is, by construction, unclassified. Failing on it is
the only thing that keeps §11.16.2's "a new column requires deciding its row"
true rather than aspirational.

    --self-test   run the built-in cases (must pass)
    <path>        check a migrations directory (default: migrations/postgres)
"""

## Verification command and expected output

Every step names the exact command and the exact expected output.
them. **Re-verify before implementing** — the last one is version-dependent.
   requires to be **published** so a third party can verify the construction.
   Expected: three rows, **the first and third identical**, the second
## Phase D1 — The masking rules as code, with a test that proves them
### D1.1 The inventory gate — `scripts/check-snapshot-pii.py`
    --self-test   run the built-in cases (must pass)
python3 scripts/check-snapshot-pii.py --self-test
# expected: self-test: N cases passed
# expected: OK: every column in a PII-bearing table is classified (N tables, M columns)
**Required self-test cases** — one per rule, plus these two that are the point:
**Mutation to run before committing** (the repo's standing rule — a gate that
# add an unclassified column to accounts, confirm the gate goes red, revert
# expected: a non-zero count. Zero means the join does not survive, which is
# expected: t
#    BYTES, per the same reasoning as the canary test.
# expected: t | t
# expected: t
### D1.3 The end-to-end leak test
`crates/app/tests/snapshot_anonymisation.rs` — the §11.16.7 test, and the one

## RED/GREEN evidence

**This phase is the whole safety property. Do not go further until it is green.**
# expected: self-test: N cases passed
has never been seen red is a hypothesis):
# add an unclassified column to accounts, confirm the gate goes red, revert
that must be red before the feature exists.
**Verification:** red before the masking exists, green after. Record both.

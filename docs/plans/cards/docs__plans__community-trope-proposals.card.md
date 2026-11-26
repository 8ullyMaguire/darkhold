# docs/plans/community-trope-proposals.md — plan card

Source: `docs/plans/community-trope-proposals.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

Every store statement is written twice with `db.sql(sqlite, postgres)`, or in an
- `docs/spec-amendments/community-trope-proposals.md`
- `crates/domain/src/tropes.rs`
- `crates/domain/src/query.rs`
- `crates/db/src/category_governance.rs` (pattern)
- `crates/app/src/config.rs`
- `crates/app/tests/config_sections.rs`
- `crates/app/src/routes/inventory.rs`
- `crates/app/src/server.rs`
- `docs/requirements.csv`
test -f docs/spec-amendments/community-trope-proposals.md && echo spec-ok
grep -n 'MAX_MEMBERS' crates/domain/src/tropes.rs | head -3

## Copy-pasteable code / commands

cd ~/code-local/rust/darkhold
git status -sb
test -f docs/spec-amendments/community-trope-proposals.md && echo spec-ok
grep -n 'MAX_MEMBERS' crates/domain/src/tropes.rs | head -3
python3 scripts/check-requirement-ids.py | tail -3

import csv
path = 'docs/requirements.csv'
with open(path, newline='') as f:
    rows = list(csv.DictReader(f))
new = [
 ('M45-77','discovery','Curators and TL>=N accounts can propose a trope group of 1..10 real tags; two independent approvals make it active','M45-77','planned','docs/spec-amendments/community-trope-proposals.md','owner decisions 2026-11-24','',''),
 ('M45-77.1','governance','One account gets one vote; the proposer may not vote on their own proposal','M45-77','planned','trope_votes UNIQUE (proposal_id, account_id) + route guard','','' ,''),
 ('M45-77.2','search','Active community groups expand through the same parser and term budget as the five seed groups','M45-77','planned','cargo test -p darkhold-domain parse_query_with_tropes','','',''),
 ('M45-77.3','config','[tropes] min trust, quorum and voting days are read from darkhold.toml by the real loader','M45-77','planned','cargo test --test config_sections tropes','','',''),
 ('M45-77.4','store','Proposal, vote and active state are applied in one transaction on SQLite and PostgreSQL','M45-77','planned','DARKHOLD_TEST_PG_URL cargo test -p darkhold-db --test trope_governance','','',''),
 ('M45-77.5','routes','Proposal/vote/settle routes are tabled with the right audience; public reads never expose an account id','M45-77','planned','cargo test --test route_inventory','','',''),
]
fieldnames = list(rows[0].keys())
assert len(fieldnames) == 9, fieldnames
with open(path, 'a', newline='') as f:
    w = csv.DictWriter(f, fieldnames=fieldnames)
    for values in new:
        w.writerow(dict(zip(fieldnames, values)))

python3 scripts/check-requirement-ids.py | tail -3
python3 scripts/ledger.py show M45-77 | head -20
git diff --check

## Verification command and expected output

rewrites the placeholders for PostgreSQL. Never hand-write `$1`. A new test is
Verify:
test -f docs/spec-amendments/community-trope-proposals.md && echo spec-ok
Expected: clean `## master...origin/master`, `spec-ok`, `MAX_MEMBERS = 10`
 ('M45-77.2','search','Active community groups expand through the same parser and term budget as the five seed groups','M45-77','planned','cargo test -p darkhold-domain parse_query_with_tropes','','',''),
 ('M45-77.3','config','[tropes] min trust, quorum and voting days are read from darkhold.toml by the real loader','M45-77','planned','cargo test --test config_sections tropes','','',''),
 ('M45-77.4','store','Proposal, vote and active state are applied in one transaction on SQLite and PostgreSQL','M45-77','planned','DARKHOLD_TEST_PG_URL cargo test -p darkhold-db --test trope_governance','','',''),
 ('M45-77.5','routes','Proposal/vote/settle routes are tabled with the right audience; public reads never expose an account id','M45-77','planned','cargo test --test route_inventory','','',''),
assert len(fieldnames) == 9, fieldnames
Verify:
Expected: `995 unique ids / 0 duplicates`, all six rows printed.
Verify:
# expected: 2
# expected: 0219_community_tropes.sql
                    "trope:{value}: not a trope group — expected one of {}",
#[test]
        other => panic!("expected OR, got {other:?}"),
#[test]
    assert!(err.contains("system-grind"), "{err}");
    assert!(err.contains("system-progression"), "{err}");

## RED/GREEN evidence

                    CHECK (status IN ('proposed','passed','active','failed','expired','retired')),
  `status='proposed' AND closes_at < now` as `expired`; re-apply any `passed`
async fn two_no_votes_fail_without_applying() { /* status='failed', active_groups empty */ }
8. Two distinct yes votes -> 200 with `status="passed"`; `GET /tropes` lists the
10. Two no votes -> `status="failed"`, no active group.
Record each red test name in the commit body. A mutation that does not redden

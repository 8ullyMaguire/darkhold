# Roadmap — community trope proposals with quorum (M45-77)

Spec: `docs/spec-amendments/community-trope-proposals.md`
Plan: `docs/plans/community-trope-proposals.md`
Ledger: `M45-77` through `M45-77.5`
Owner decisions: 2026-11-24

## Phase 0 — record the decision (done)

- [x] Ask who counts as curator, the TL bar, quorum size and electorate.
- [x] Record: `accounts.is_curator` OR active curator role; TL3; two independent
      approvals, proposer excluded; same pool votes.
- [x] Measure the live reviewer pool and write it into the spec rather than
      discovering it during rollout.

Live shape that gates the whole feature:

    trust_levels: TL1 40, TL2 8, TL4 1, TL6 1  -> only 2 accounts >= TL3
    accounts.is_curator = true                  -> 0 rows
    curator_roles                               -> 0 rows
    canonical_entities kind='tag'               -> 67,876 rows

With two eligible accounts and no curators, a proposal by either account has
only one independent approver left. No group can reach quorum on this instance
until at least one more curator is granted or a third TL3+ account exists.
Phase 6 is blocked on that staffing action, not on code.

## Phase 1 — ledger and migration

- [ ] Add M45-77 rows to `docs/requirements.csv`.
- [ ] Add `0219_community_tropes.sql`: `trope_proposals`, `trope_votes`,
      partial live-slug index, one-vote-per-account constraint.
- [ ] Verify: check-requirement-ids shows no duplicates; migration count 2.

Exit: both engines can create the tables.

## Phase 2 — domain rule and parser

- [ ] `darkhold_domain::trope_governance`: action enum, slug grammar, reused
      `proposal_decided`, derived expiry.
- [ ] `parse_query_with_tropes`: seed groups first, store groups second, one
      parser, one 24-term budget, unknown slug lists every active slug.
- [ ] Verify: `cargo test -p darkhold-domain tropes` and the unknown-slug test.

Exit: a community group expands to OR-of-tags without any text substitution.

## Phase 3 — config with a real reader

- [ ] `[tropes]` section: `proposal_min_trust=3`, `quorum=2`, `voting_days=7`.
- [ ] Load through `FileConfig`; refuse `quorum < 2` at load.
- [ ] Extend `crates/app/tests/config_sections.rs` with the full, partial and
      typo cases.
- [ ] Verify: `cargo test --test config_sections tropes`.

Exit: the keys in `docs/config-reference.md` are true of the running binary.
Do not repeat the `taxonomy.trope_coverage` defect (declared field, no file
reader), and do not claim the planned `[works]` M45-76 keys work before that
plan wires them.

## Phase 4 — store and authority

- [ ] `darkhold_db::trope_governance`: insert/get/list, vote with
      `ON CONFLICT ... DO NOTHING`, recount-and-decide, atomic apply, active
      groups, expiry sweep.
- [ ] `darkhold_app::trope_authority::for_account`: curator OR TL bar,
      re-checked at vote time, coarse role for public answers.
- [ ] Verify: five repository tests on SQLite and PostgreSQL, five authority
      cases.

Exit: two distinct yes votes pass and apply in one transaction; a second vote
from the same account changes nothing.

## Phase 5 — routes and search wiring

- [ ] Proposal create/list/get, vote, operator settle; five route-inventory
      entries with the right audience.
- [ ] `GET /tropes` merges seed and active community groups with `source`.
- [ ] Wire the resolver into all production parse sites: works, forum, users,
      media x2, exports, bulk export.
- [ ] Verify: route inventory passes; `cargo check --workspace --all-targets`.

Exit: one slug means the same thing in every search box.

## Phase 6 — acceptance, mutation, rollout

- [ ] 15 route tests on both engines.
- [ ] Mutation list from plan step 11, with each red test recorded.
- [ ] Grant at least one additional curator so two independent approvals are
      possible. This is an operator action.
- [ ] Live probe: propose, vote from two accounts, compare `trope:<slug>` with
      the written-out OR of tags; retire and confirm the 422 lists active slugs.
- [ ] Update `docs/known-gaps.md` from "static only" to "quorum-governed" with
      the live counts and the date.

Definition of done: every M45-77 row has an evidence command; both engines are
green; a live proposal has passed through two independent human accounts; no
account id appears in a public trope route answer.

## Dependencies

- **M45-75:** search predicates and popularity sort are a separate plan. This
  feature only changes the meaning of `trope:`. Do not bundle them.
- **M45-76:** lifecycle marks are separate. No shared table.
- **M69-18:** trope clusters are operator coverage checklists, not search
  vocabulary. No shared route.
- **ADR 0029:** no provenance accepted from a request body.
- **§19.3:** single-admin mode does not reduce quorum.

## Risks

1. **Reviewer starvation (live today).** Two eligible accounts, no curators,
   proposer excluded. Mitigation: grant curators before rollout; do not lower
   quorum.
2. **Meaning drift.** A saved search naming a retired group starts failing.
   Mitigation: 422 lists active slugs; the saved text is never silently
   rewritten.
3. **Two parsers.** A forgotten call site gives one surface the old meaning.
   Mitigation: grep all `parse_query(` call sites in plan step 8 and add a
   per-surface test.
4. **Stale update.** Two proposals editing the same group out of order.
   Mitigation: `target_version` checked in the apply transaction.
5. **Config that looks live but is not.** Mitigation: file-backed config test
   through `GlobalArgs`, plus a typo refusal case.

## Related ideas, deliberately not in this roadmap

- Proposed groups with a cooling period before voting, reusing the retention
  governance pattern, if the instance later sees rushed vocabulary changes.
- Per-fandom groups, once `fandom:` and `trope:` can be combined in one query
  budget without hitting the 24-term ceiling.
- A "trope health" report showing groups with zero hits, to retire dead
  vocabulary; advisory only, never an auto-retire.
- Letting a quorum-approved group suggest tags for a work, as a prompt to the
  author or curator, never as an accepted provenance signal (ADR 0029).

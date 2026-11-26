# docs/ROADMAP-community-trope-proposals.md — plan card

Source: `docs/ROADMAP-community-trope-proposals.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

Spec: `docs/spec-amendments/community-trope-proposals.md`
Plan: `docs/plans/community-trope-proposals.md`
- [ ] Add M45-77 rows to `docs/requirements.csv`.
- [ ] Add `0219_community_tropes.sql`: `trope_proposals`, `trope_votes`,
- [ ] Extend `crates/app/tests/config_sections.rs` with the full, partial and
Exit: the keys in `docs/config-reference.md` are true of the running binary.
- [ ] Update `docs/known-gaps.md` from "static only" to "quorum-governed" with

## Copy-pasteable code / commands

No fenced command block was recorded in the source document.

## Verification command and expected output

- [ ] Verify: check-requirement-ids shows no duplicates; migration count 2.
Exit: both engines can create the tables.
- [ ] Verify: `cargo test -p darkhold-domain tropes` and the unknown-slug test.
Exit: a community group expands to OR-of-tags without any text substitution.
- [ ] Verify: `cargo test --test config_sections tropes`.
Exit: the keys in `docs/config-reference.md` are true of the running binary.
- [ ] Verify: five repository tests on SQLite and PostgreSQL, five authority
Exit: two distinct yes votes pass and apply in one transaction; a second vote
- [ ] Verify: route inventory passes; `cargo check --workspace --all-targets`.
Exit: one slug means the same thing in every search box.
- [ ] Mutation list from plan step 11, with each red test recorded.
   per-surface test.
5. **Config that looks live but is not.** Mitigation: file-backed config test

## RED/GREEN evidence

- [ ] Mutation list from plan step 11, with each red test recorded.
green; a live proposal has passed through two independent human accounts; no

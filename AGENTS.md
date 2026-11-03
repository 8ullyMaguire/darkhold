# AGENTS.md

<!-- estate:process — managed by scripts/install-agents-md.py; edit estate.toml instead -->
You are working in **darkhold**, one product of the Polaris estate (22 hostnames, one operator).

Read first, in this order:
1. `/personal/documents/code/estate/docs/PROCESS.md` — the standing process for every repo.
2. This repo's documents:
   - spec: `/personal/documents/code/rust/darkhold/docs/spec.md`
   - decisions: `/personal/documents/code/rust/darkhold/docs/adr`
   - requirements: `/personal/documents/code/rust/darkhold/docs/requirements.csv`
   - changelog: `/personal/documents/code/rust/darkhold/docs/changelog.md`
3. The wave document under `~/secondbrain/10-Projects/` if this work belongs to a wave.

This repo's declared style: `adr`.
Verification: run `just check` and paste the real output; then the estate gates
(`bash ~/estate-gates/run-gates.sh`, must end `VERDICT: all gates clear`).
Refused estate-wide: merging databases, one shared palette, one imposed framework, a load-bearing
topbar, a second comment tree or notification table or search index, a federation bridge without a
written policy (see PROCESS.md §7).
<!-- /estate:process -->
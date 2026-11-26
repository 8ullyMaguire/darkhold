# docs/plans/deploy-lineage-decision.md — plan card

Source: `docs/plans/deploy-lineage-decision.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

| deploy tree `/personal/documents/code/rust/darkhold` | HEAD `c177bc1b`, with uncommitted edits (`README.md`, `auth.rs`, `governance.rs`, `inventory.rs`, …) |
M63-02's code is in `crates/domain`, `crates/db` and `crates/app` on **this** line. Deploying it
modified (`auth.rs`, `governance.rs`, `inventory.rs`, `README.md`).
`install-binary.sh` already keeps the replaced binary.
local SQLite dev database and migrated nothing. The command discovers `darkhold.toml` **from the

## Copy-pasteable code / commands

cd ~/code/rust/darkhold
git branch -f wip/deploy-checkout-2026-11-01 HEAD      # preserve, do not lose their state
git add -A && git commit -m "wip: the deploy checkout's uncommitted catalogue work, preserved"
git remote add local ~/code-local/rust/darkhold
git fetch local master && git reset --hard local/master # -> 9b1bb539, then b5f5b9cf

## Verification command and expected output

No verification command or expected output was recorded in the source document.

## RED/GREEN evidence

No RED/GREEN evidence was recorded in the source document.

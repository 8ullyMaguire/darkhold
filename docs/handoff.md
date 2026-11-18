## 2026-11-18 — M63-29 `dh tui` shipped

`dh tui` is now a command in the same binary, compiled only with `--features tui`. Without the feature it exits 2 with a sentence naming the missing feature; with it, an alternate-screen prompt dispatches each line back to `dh --json`, so the query grammar, saved views and exit codes are not restated. `tui` inside the prompt is refused by name; `q` restores the terminal. Default-feature refusal 1/1; `tui`-feature integration 2/2 and module 2/2; the feature-gate mutant (exit 0 instead of 2) reddened the refusal test and was restored from `/tmp/dh-tui.bak`. Full gate: `/tmp/gate-all-m63-29-18edf198-jobsenv.log` ended `GATE_ALL_EXIT=0`, all eight stages ok, with `DARKHOLD_JOBS_TEST_DSN` set for the concurrently merged row 409 A3. No deploy: feature-gated CLI only. M73-03 and M73-04 remain Blocked with their clearing conditions.

## 2026-11-18 — concurrent jobs row integrated into the CLI layering gate

A concurrent session pushed `a8a6b8b5` (row 409 A3) during the M63-29 full gate, adding `darkhold-db` and a path dependency on `polaris-jobs` to the CLI. That made `layering` red and the path unavailable in this checkout. The shared crate exists on the thinkcentre mount, so `/home/alvaro/code-local/estate/crates` was symlinked to `/home/alvaro/mnt/thinkcentre/personal/documents/code/estate/crates` (environment fix, not a repo change). `layering.rs` now permits `darkhold-db` only when every import is confined to `src/jobs.rs`; the new guard reads the manifest and the source files. Targeted suite: 3/3. No other session work was reverted.


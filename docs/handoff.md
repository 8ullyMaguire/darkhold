## 2026-11-18 — SSHFS mirror cleared

`/home/alvaro/code/rust/darkhold` fast-forwarded to `66aad53d` and is clean. The earlier `folio_wait_bit_common` hang did not recur after `origin/master` was fetched with `GIT_CONFIG_GLOBAL=/dev/null`; only the `merge --ff-only` needed a longer timeout (90s). No remaining mirror block.

## 2026-11-18 — M63-30 Calibre interop shipped

`dh lib --calibre <path>` reads a Calibre `metadata.db` read-only and emits the seed grammar `darkhold seed read-list` already parses. Rated books only, stable author order, 0–10 ratings left unchanged so the server remains the only owner of star conversion. The default prints a preview; `--out <path>` writes the same text locally. No crate/RPC link, no HTTP route, no schema change, no deploy.

Proof: full gate `/tmp/gate-all-m63-30-063badea.log` `GATE_ALL_EXIT=0`, all eight stages ok at `063badea`; integration 1/1 with a source-DB byte-hash check; unit 1/1; separator mutant red and restored from `/tmp/dh-calibre.bak`; clippy clean.

## 2026-11-18 — M63-26 `dh stats` shipped

`dh stats --by week|year [--year YYYY] [--share]` reads two private surfaces the site already has: `GET /api/v1/me/streak` for current/longest streak and freezes, and `GET /api/v1/me/passport` for the origin-signed contribution snapshot. The passport payload is base64url-decoded locally; the CLI does not restate a signature check that belongs to the receiving instance. `--share` drops handle, origin and exact issue time and keeps totals only. No new route, schema, migration or deploy.

The power-user sweep's "operation log (imports, tags applied, reports filed)" wording has no schema behind it in either engine; works/revisions/comments are the counts that exist. Recorded in the roadmap so a future operation-log surface can replace the adapter instead of being duplicated.

Proof: full gate `/tmp/gate-all-m63-26-ba364ea5.log` `GATE_ALL_EXIT=0`, all eight stages ok at `ba364ea5`; integration 3/3, unit 3/3, identity-leak mutant red and restored from `/tmp/dh-stats.bak`, clippy clean.

## 2026-11-18 — M63-29 `dh tui` shipped

`dh tui` is now a command in the same binary, compiled only with `--features tui`. Without the feature it exits 2 with a sentence naming the missing feature; with it, an alternate-screen prompt dispatches each line back to `dh --json`, so the query grammar, saved views and exit codes are not restated. `tui` inside the prompt is refused by name; `q` restores the terminal. Default-feature refusal 1/1; `tui`-feature integration 2/2 and module 2/2; the feature-gate mutant (exit 0 instead of 2) reddened the refusal test and was restored from `/tmp/dh-tui.bak`. Full gate: `/tmp/gate-all-m63-29-18edf198-jobsenv.log` ended `GATE_ALL_EXIT=0`, all eight stages ok, with `DARKHOLD_JOBS_TEST_DSN` set for the concurrently merged row 409 A3. No deploy: feature-gated CLI only. M73-03 and M73-04 remain Blocked with their clearing conditions.

## 2026-11-18 — concurrent jobs row integrated into the CLI layering gate

A concurrent session pushed `a8a6b8b5` (row 409 A3) during the M63-29 full gate, adding `darkhold-db` and a path dependency on `polaris-jobs` to the CLI. That made `layering` red and the path unavailable in this checkout. The shared crate exists on the thinkcentre mount, so `/home/alvaro/code-local/estate/crates` was symlinked to `/home/alvaro/mnt/thinkcentre/personal/documents/code/estate/crates` (environment fix, not a repo change). `layering.rs` now permits `darkhold-db` only when every import is confined to `src/jobs.rs`; the new guard reads the manifest and the source files. Targeted suite: 3/3. No other session work was reverted.


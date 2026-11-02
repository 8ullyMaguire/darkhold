# 38. Instance Configuration — The Self-Hosted Contract

**Status:** reserved
**Milestone:** —
**Source:** §38 of `docs/spec.md`

> **2026-09-20 addition.** Darkhold is self-hosted. The operator — not the
> codebase — decides how their instance behaves. This section codifies that
> principle: every tunable value is admin-configurable via `darkhold.toml`,
> with sensible defaults that let a fresh instance run without touching a
> single config line.

## 38.1 The principle

1. **No hardcoded magic numbers.** If a value affects behavior (timeouts,
   limits, thresholds, retention windows, budgets, taxonomies), it lives in
   `Config`, not a `const` in application code.
2. **Defaults are safe, not restrictive.** A fresh `darkhold.toml` with
   every section omitted must produce a working instance. Defaults favor
   development-friendliness; production hardening is opt-in.
3. **Every config key has a documented default.** The operator can see what
   the default is without reading source code.
4. **Validation at startup, not at use.** A malformed config key refuses to
   start with a clear error message. No silent fallbacks to arbitrary values.
5. **Environment overrides file.** `DARKHOLD_*` environment variables take
   precedence over the file, so containerized deployments can inject secrets
   and tune values without rewriting the file.

## 38.2 Currently configurable (verified against `Config`)

| Section | Key | Default | Purpose |
|---------|-----|---------|---------|
| `[site]` | `name` | `"Darkhold"` | Instance display name |
| `[site]` | `base_url` | auto from bind+port | Canonical URL |
| `[site]` | `contact_email` | `None` | Admin contact |
| `[site]` | `topics` | `[]` | Public discovery topics |
| `[server]` | `bind` | `"127.0.0.1"` | Listen address |
| `[server]` | `port` | `8080` | Listen port |
| `[server]` | `max_body_bytes` | `2097152` (2 MiB) | Max request body |
| `[server]` | `request_timeout_secs` | `30` | Request timeout |
| `[database]` | `url` | `sqlite://./data/darkhold.sqlite` | DB connection string |
| `[database]` | `max_connections` | `5` (dev) / `10` (prod) | Connection pool size |
| `[database]` | `acquire_timeout_secs` | `10` | Pool acquire timeout |
| `[storage]` | `root` | `"./data"` (dev) / `"/var/lib/darkhold"` (prod) | Blob storage root |
| `[security]` | `cookie_secure` | `false` (dev) / `true` (prod) | Secure cookie flag |
| `[security]` | `session_ttl_days` | `30` | Session lifetime |
| `[security]` | `csrf_required` | `true` | CSRF protection |
| `[security]` | `trust_proxy` | `false` | Trust `X-Forwarded-*` |
| `[logging]` | `filter` | `"info,darkhold_app=debug"` | Log filter |
| `[logging]` | `format` | `"pretty"` (dev) / `"json"` (prod) | Log format |
| `[accounts]` | `registration_open` | `true` | Allow new registrations |
| `[age]` | `threshold` | `14` | Age of consent threshold |
| `[age]` | `guardian_workflow_enabled` | `false` | Guardian authorization |
| `[rate_limits]` | `auth.burst`, `auth.per_minute` | `5`, `20` | Auth rate limit |
| `[rate_limits]` | `write.burst`, `write.per_minute` | `10`, `60` | Write rate limit |
| `[rate_limits]` | `search.burst`, `search.per_minute` | `20`, `120` | Search rate limit |
| `[rate_limits]` | `export.burst`, `export.per_minute` | `3`, `10` | Export rate limit |
| `[rate_limits]` | `default.burst`, `default.per_minute` | `30`, `180` | Default rate limit |
| `[imports]` | `solver_url` | `None` | CAPTCHA solver URL |
| `[imports]` | `archive_fallback` | `false` | Fallback to archive.org |
| `[imports]` | `honour_robots` | `true` | Respect robots.txt |
| `[theme]` | `mode` | `"thematic"` | Theme mode: `generic`, `thematic`, `adaptive` (§0.4.6) |
| `[theme]` | `allow_user_opt_out` | `true` | Whether the §16.5 dial can reach zero for theme influence |
| `[theme]` | `theme_dial_floor_bp` | `1000` | Dial lower bound (basis points) when opt-out is locked |
| `[theme]` | `adaptive_max_drift_bp` | `0` | Maximum adaptive drift in basis points (0 = no drift) |
| `[theme]` | `influence_sources` | `[{ kind = "operator_topics" }]` | Ordered influence sources (§0.4.6) |
| `[theme]` | `boost_tags` | `[]` | Tag substrings that boost discovery rank |
| `[theme]` | `suppress_tags` | `[]` | Tag substrings that suppress discovery rank |
| `[theme]` | `tag_gravity_bp` | `{}` | Per-tag gravity in basis points (overrides boost/suppress) |
| `[tts]` | `engine` | `"piper"` | TTS engine (`silent` for a pipeline check without a synthesizer) |
| `[tts]` | `piper_path` | `None` | Piper binary path |
| `[tts]` | `piper_voice_model` | `None` | Piper voice model |
| `[tts]` | `default_voice` | `None` | Default voice |
| `[tts]` | `monthly_spend_cap_cents` | `None` | Monthly TTS spend cap |
| `[directory]` | `page_size` | `50` | Entries per page |
| `[directory]` | `require_approval` | `true` | Submissions need operator approval before becoming visible |
| `[directory]` | `extra_categories` | `[]` | Operator-added categories beyond the seeded eight |
| `[directory]` | `vote_weighting` | `"trust_and_taste"` | `flat`, `trust`, or `trust_and_taste` |
| `[directory]` | `trust_vote_weights` | `"0.5,0.75,1.0,1.25,1.5,1.75,2.0"` | Multiplier per trust level TL0–TL6 |
| `[directory]` | `taste_vote_floor` | `0.75` | Vote multiplier at taste affinity 0 |
| `[directory]` | `taste_vote_ceiling` | `1.25` | Vote multiplier at taste affinity 1 |
| `[forum]` | `vote_budget` | `[(1,10),(3,30),(5,60)]` | Vote budget per TL |
| `[forum]` | `karma_decay_percent` | `5` | Monthly karma decay % |
| `[forum]` | `meta_mod_points` | `1` | Points per meta-mod verdict |
| `[forum]` | `meta_mod_min_verdicts` | `3` | Min verdicts to elect moderator |
| `[forum]` | `min_vote_weight_bp` | `100` | Min vote weight in basis points |
| `[forum]` | `work_discussion_default` | `"thread_only"` | Default discussion mode |
| `[browse]` | `default_sort` | `"for-you"` | Ordering for a signed-in reader with a profile (§43.4) |
| `[browse]` | `anonymous_sort` | `"top"` | Neutral ordering for anonymous traffic (§43.4) |
| `[browse]` | `surface_defaults` | unset | Per-surface overrides of `default_sort` |
| `[weighting]` | `mode` | `"trust_taste_contribution"` | `flat \| trust \| trust_taste \| trust_taste_contribution` (§16.16) |
| `[weighting]` | `taste_floor` | `0.75` | Taste multiplier floor; `taste_floor × taste_ceiling ≤ 1.0` enforced |
| `[weighting]` | `taste_ceiling` | `1.25` | Taste multiplier ceiling |
| `[weighting]` | `contribution_floor` | `1.0` | Contribution multiplier floor |
| `[weighting]` | `contribution_ceiling` | `2.0` | Contribution multiplier ceiling |
| `[weighting]` | `contribution_window_days` | `180` | How far back contribution is counted |
| `[weighting]` | `demand_diversity_percent` | `20` | Fraction of surfaced demand with no boost; must be `> 0` |
| `[discovery]` | `taste_sources` | `[{ kind = "admin" }]` | Taste sources and their members (§16.15) |
| `[discovery]` | `taste_source_min_members` | `5` | Cohort-size floor enforced at startup |

## 38.3 Hardcoded values that MUST become configurable

These are currently `const` values in application code. They must be moved to
`Config` with defaults and TOML keys.

| Current location | Constant | Proposed key | Default |
|------------------|----------|--------------|---------|
| `crates/app/src/exports.rs` | `RETENTION_DAYS = 7` | `[exports] retention_days` | `0` (forever) |
| `crates/app/src/exports.rs` | `GRANT_TTL_SECONDS = 3600` | `[exports] grant_ttl_secs` | `3600` |
| `crates/app/src/revisions.rs` | `REVISION_TTL_SECONDS = 604800` | `[revisions] ttl_secs` | `604800` |
| `crates/app/src/worker.rs` | `TERMINAL_JOB_RETENTION = 30d` | `[jobs] terminal_retention_days` | `30` |
| `crates/app/src/bulk_export.rs` | `DEFAULT_MAX_ITEMS = 50` | `[bulk_export] max_items` | `50` |
| `crates/app/src/bulk_export.rs` | `DEFAULT_MAX_BYTES = 1GiB` | `[bulk_export] max_bytes` | `1073741824` |
| `crates/domain/src/library.rs` | `UPDATE_CHECK_RETENTION_DAYS = 90` | `[library] update_check_retention_days` | `90` |
| `crates/app/src/library_updates.rs` | `CHECK_BATCH = 50` | `[library] check_batch` | `50` |
| `crates/app/src/administration.rs` | `webhook_timeout_secs = 10` | `[administration] webhook_timeout_secs` | `10` |
| `crates/app/src/administration.rs` | `webhook_max_attempts = 5` | `[administration] webhook_max_attempts` | `5` |
| `crates/app/src/administration.rs` | `webhook_base_delay_ms = 500` | `[administration] webhook_base_delay_ms` | `500` |

## 38.4 Migration path

1. Add new fields to the relevant `*Config` struct with `serde::Deserialize`.
2. Add defaults to `*Config::default()`.
3. Read values in `Config::load()` from the TOML file.
4. Replace `const` references with `config.xxx` calls.
5. Update `docs/spec.md` §38.2 and §38.3.
6. Update `docs/config-reference.md` (create if missing).

## 38.5 Acceptance

- Every hardcoded value in §38.3 has a corresponding `[section] key` in
  `darkhold.toml`.
- `darkhold.toml` with all defaults omitted produces a working instance.
- Setting `retention_days = 0` disables export cleanup entirely.
- A malformed config value (e.g., `retention_days = -1`) refuses to start
  with a clear error.
- All existing tests pass with default config values.

## 38.6 What this section deliberately does not do

- **No runtime reload.** Config changes require a restart. (Future: SIGHUP
  reload, but not now.)
- **No per-user overrides.** Instance config is global. User preferences are
  in `privacy_settings` and `reader_settings`, and in the other per-domain
  settings tables (§46.3) — user configuration lives in the database, never
  in the config file.
- **No feature flags.** This is about tuning values, not toggling features.
  Feature gating is done at compile time or via Cargo features.

## 38.7 `doctor` — make the existing command honest

> **2026-09-30 addition.** §38 makes the instance configurable; nothing made it
> *checkable*, and `crates/app/src/doctor.rs` already answers half of that. This
> section is therefore a **delta on shipped code**, not a new command. Every
> defect below was verified by reading the source, not inferred from the spec.

`doctor` already runs 18 checks across `config-file`, `migrations`,
`database-file`, `storage`, `disk-space`, `narration` and `assets`, and already
prints a `fix:` remedy line naming the setting to change on each failure. That
is the hard part and it exists. Three things do not, and each is a specific
omission rather than a missing feature:

**38.7.1 `--strict` is declared, accepted, and ignored.** `DoctorArgs.strict`
(`crates/app/src/cli.rs:204`) is documented as "treat warnings as failures", but
`doctor::run` binds it as `_args` (`crates/app/src/doctor.rs:88`) and never
reads it. An operator who passes `--strict` gets a zero exit code and a
`0 failing, 3 warning(s)` line. **A flag that does nothing is worse than an
absent flag**, because it converts a warning the operator asked to be treated as
fatal into a green run. `strict` must make `Severity::Warning` count as a
failure, and a test must assert the exit code differs between `strict: false`
and `strict: true` on a report that has warnings and no failures — otherwise the
parameter can be dropped again with the suite green.

**38.7.2 There is no skip state, so a check that could not run reads as a
check that ran.** `Report.checks` holds `Severity::{Ok, Warning, Fatal}` and
nothing else; `grep -rn 'skip' crates/app/src/doctor.rs` returns **zero**. A
check whose precondition is absent — a federation peer configured but
unreachable, a search index backend not compiled in, the dual-engine Postgres
arm when `DARKHOLD_TEST_PG_URL` is unset — is either omitted from the report
or reported as `ok`, and both readings tell the operator the instance was
examined. Add `Severity::Skipped` with a **mandatory reason string**; a bare
skip is a defect, and `render` must print it as its own line, never as a
warning. This is the same class as `docs/verification.md`'s rule that a partial
run be reported as partial, and as Trap 1 in this project's own goal doc (nine
e2e tests reporting `passed` having executed nothing).

**38.7.3 The report ends with a count and nothing else.** `render` finishes
with `"{n} check(s): {failures} failing, {warnings} warning(s)"`. **It does not
state what it did not check.** A diagnostic that overstates its coverage
converts "I have not looked" into "I have looked and it is fine", and the reader
of that line is the operator deciding whether to trust a production instance.
The report ends with an explicit **NOT CHECKED** block naming each omitted area
and why (backup restore needs a target host; worker network egress and TLS
termination are not observable from the box; any check skipped per §38.7.2).
Borrowed from GRAVITY SPEC §31.2, whose honesty clause is the half worth
copying.

**38.7.4 Never reachable over HTTP.** Unchanged and already true: `doctor` is a
CLI command with no route. §24.2's public statistics remain the reader-facing
surface and must not grow an operator mode — a diagnostic endpoint is a map of
an instance's internals, including which settings are unusual.

**38.7.5 Acceptance.**
- `--strict` on a report with warnings and no failures exits non-zero; without
  it, zero. One test, both branches. (Control: the test fails if `strict` is
  ignored again — which is the current state.)
- A check whose precondition is absent reports `Skipped` **with** a reason, and
  the rendered output contains that reason verbatim. Removing the reason string
  from the type must fail the test.
- The rendered report's final block is `NOT CHECKED` and is non-empty on a run
  where nothing was skipped, because the areas §38.7.3 lists are unobservable
  from the box on **every** run — an empty NOT CHECKED block would itself be the
  overstatement this clause forbids.
- Every check reporting `Ok` executed its probe. The snapshot check is the one
  to pin: it must decrypt-and-restore rather than read a manifest, because
  Trap 3 shipped an entire unmasked instance while every gate was green.

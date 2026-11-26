# Follow-up suggestions — 2026-11-26

Five suggestions from the closed review queue. This file is the working plan; each item records its own acceptance evidence.

1. Lifecycle timer — re-enable `darkhold-lifecycle.timer` after a peak-hour check. The unit fires at 04:45, so enabling now is safe even with current load average ~17 and 10 GiB used. Acceptance: `systemctl --user is-enabled/is-active` shows `enabled`/`waiting`.
2. AO3 tag backfill — provenance for the research work is `{}`, but its stored `units.current_revision.document_json` contains the source's own `Fandom:`, `Relationships:`, `Characters:`, `Tags:` and `Warnings:` lines. Extend `tag_backfill::backfill_from_provenance` with a fallback that parses those labelled lines from the stored document, only for `source_key='ao3'`. Never ship or print body text; only canonical tag labels are written. Acceptance: dual-engine test plus live `ta33-tag-probe.sh` returns a non-zero `work_tags` count.
3. OR source probe — `source:ao3 OR source:pawchive` matched both sources in the query plan, but popularity ranking put 500 Pawchive rows first. Use a grouped query with a shared term: `absolution AND (source:ao3 OR source:pawchive)`. Acceptance: one live result page contains both `ao3` and `pawchive`.
4. Scraper mirrors — the three mirror repos are absent on Forgejo. Acceptance: `git push` succeeds, or record the exact external block (Cloudflare 1010) and owner.
5. UI — add a visible `Search help` action wired to the existing Ctrl+K `DocsSearch` state, and give the footer Radio entry a tagline. Acceptance: targeted frontend tests, svelte-check, one build+deploy, desktop/phone DOM proof.

## Results so far

1. Lifecycle — DONE. `systemctl --user enable --now darkhold-lifecycle.timer` printed `ENABLED=enabled`, `ACTIVE=active`. It fires at 04:45, so enabling during the 10:25 peak did not start maintenance work.
2. AO3 tags — CODE DONE, live application pending the release build. `crates/db/src/tag_backfill.rs` now falls back to labelled lines in the stored `units.current_revision.document_json` for `source_key='ao3'`. SQLite and PostgreSQL both ran `tag_backfill` `3 passed`; pure parser `2 passed`; deleting the Warnings label made the integration test fail with `assertions: 6`, then was restored.
3. OR source — DONE. `source:ao3 OR source:pawchive` matched both sources in `EXPLAIN`, but popularity ranking returned 500 pawchive rows. The grouped query `absolution AND (source:ao3 OR source:pawchive)` returned one `ao3` and fifteen `pawchive` rows in a single live result page.
4. Mirrors — DONE. Forgejo API accepted Basic auth from the git credential store (no secret printed or stored in a repo), created repos 108/109/110, and the three local trees pushed (`8e9f73b`, `406b59b`, `d989faf`). Public API now answers `200` for all three.
5. UI — CODE DONE, live DOM pending the release build. `/docs` has a 44px `Search help` button; `App.svelte` listens for `darkhold:docs-search` and opens the same `DocsSearch` dialog. `Docs/App` `20 passed`, `svelte-check` `0 errors and 10 warnings`, App-listener mutant failed with `Unable to find role="searchbox"`. Estate `estate.toml` now gives Radio the tagline `Shortwave episodes in a tab`; `generated/nav.json` was regenerated and `/estate/nav.json` serves it live.

# docs/plans/followup-suggestions-2026-11-26.md — plan card

Source: `docs/plans/followup-suggestions-2026-11-26.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

2. AO3 tag backfill — provenance for the research work is `{}`, but its stored `units.current_revision.document_json` contains the source's own `Fandom:`, `Relationships:`, `Characters:`, `Tags:` and `Warnings:` lines. Extend `tag_backfill::backfill_from_provenance` with a fallback that parses those labelled lines from the stored document, only for `source_key='ao3'`. Never ship or print body text; only canonical tag labels are written. Acceptance: dual-engine test plus live `ta33-tag-probe.sh` returns a non-zero `work_tags` count.
2. AO3 tags — CODE DONE, live application pending the release build. `crates/db/src/tag_backfill.rs` now falls back to labelled lines in the stored `units.current_revision.document_json` for `source_key='ao3'`. SQLite and PostgreSQL both ran `tag_backfill` `3 passed`; pure parser `2 passed`; deleting the Warnings label made the integration test fail with `assertions: 6`, then was restored.
5. UI — CODE DONE, live DOM pending the release build. `/docs` has a 44px `Search help` button; `App.svelte` listens for `darkhold:docs-search` and opens the same `DocsSearch` dialog. `Docs/App` `20 passed`, `svelte-check` `0 errors and 10 warnings`, App-listener mutant failed with `Unable to find role="searchbox"`. Estate `estate.toml` now gives Radio the tagline `Shortwave episodes in a tab`; `generated/nav.json` was regenerated and `/estate/nav.json` serves it live.

## Copy-pasteable code / commands

No fenced command block was recorded in the source document.

## Verification command and expected output

2. AO3 tag backfill — provenance for the research work is `{}`, but its stored `units.current_revision.document_json` contains the source's own `Fandom:`, `Relationships:`, `Characters:`, `Tags:` and `Warnings:` lines. Extend `tag_backfill::backfill_from_provenance` with a fallback that parses those labelled lines from the stored document, only for `source_key='ao3'`. Never ship or print body text; only canonical tag labels are written. Acceptance: dual-engine test plus live `ta33-tag-probe.sh` returns a non-zero `work_tags` count.
2. AO3 tags — CODE DONE, live application pending the release build. `crates/db/src/tag_backfill.rs` now falls back to labelled lines in the stored `units.current_revision.document_json` for `source_key='ao3'`. SQLite and PostgreSQL both ran `tag_backfill` `3 passed`; pure parser `2 passed`; deleting the Warnings label made the integration test fail with `assertions: 6`, then was restored.

## RED/GREEN evidence

2. AO3 tags — CODE DONE, live application pending the release build. `crates/db/src/tag_backfill.rs` now falls back to labelled lines in the stored `units.current_revision.document_json` for `source_key='ao3'`. SQLite and PostgreSQL both ran `tag_backfill` `3 passed`; pure parser `2 passed`; deleting the Warnings label made the integration test fail with `assertions: 6`, then was restored.
5. UI — CODE DONE, live DOM pending the release build. `/docs` has a 44px `Search help` button; `App.svelte` listens for `darkhold:docs-search` and opens the same `DocsSearch` dialog. `Docs/App` `20 passed`, `svelte-check` `0 errors and 10 warnings`, App-listener mutant failed with `Unable to find role="searchbox"`. Estate `estate.toml` now gives Radio the tagline `Shortwave episodes in a tab`; `generated/nav.json` was regenerated and `/estate/nav.json` serves it live.

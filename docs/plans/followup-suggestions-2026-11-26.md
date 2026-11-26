# Follow-up suggestions — 2026-11-26

Five suggestions from the closed review queue. This file is the working plan; each item records its own acceptance evidence.

1. Lifecycle timer — re-enable `darkhold-lifecycle.timer` after a peak-hour check. The unit fires at 04:45, so enabling now is safe even with current load average ~17 and 10 GiB used. Acceptance: `systemctl --user is-enabled/is-active` shows `enabled`/`waiting`.
2. AO3 tag backfill — provenance for the research work is `{}`, but its stored `units.current_revision.document_json` contains the source's own `Fandom:`, `Relationships:`, `Characters:`, `Tags:` and `Warnings:` lines. Extend `tag_backfill::backfill_from_provenance` with a fallback that parses those labelled lines from the stored document, only for `source_key='ao3'`. Never ship or print body text; only canonical tag labels are written. Acceptance: dual-engine test plus live `ta33-tag-probe.sh` returns a non-zero `work_tags` count.
3. OR source probe — `source:ao3 OR source:pawchive` matched both sources in the query plan, but popularity ranking put 500 Pawchive rows first. Use a grouped query with a shared term: `absolution AND (source:ao3 OR source:pawchive)`. Acceptance: one live result page contains both `ao3` and `pawchive`.
4. Scraper mirrors — the three mirror repos are absent on Forgejo. Acceptance: `git push` succeeds, or record the exact external block (Cloudflare 1010) and owner.
5. UI — add a visible `Search help` action wired to the existing Ctrl+K `DocsSearch` state, and give the footer Radio entry a tagline. Acceptance: targeted frontend tests, svelte-check, one build+deploy, desktop/phone DOM proof.

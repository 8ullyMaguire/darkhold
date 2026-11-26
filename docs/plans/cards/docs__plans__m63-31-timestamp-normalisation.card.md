# docs/plans/m63-31-timestamp-normalisation.md — plan card

Source: `docs/plans/m63-31-timestamp-normalisation.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

Status: planned → implementing. Authority for the catalogue: `docs/plans/m63-02-window-grammar.md`
| `YYYY-MM-DDTHH:MM:SS+00:00` | 46,281 | `crates/app/src/imports.rs` (`Rfc3339` on an `OffsetDateTime`) |
`crates/db/src/catalogue.rs` therefore carries a shape-deciding `CASE` per row
  seconds, and `library_input` in `crates/app/src/imports.rs` formats the source's date through it
The bare dates are produced by an importer that is **not in this workspace** — `crates/scrapers`
returns `OffsetDateTime` and `crates/app/src/imports.rs` formats RFC3339, so neither can produce
| `migrations/postgres/0132_catalogue_timestamp_canonical.sql` | backfill, `CHECK … NOT VALID` + `VALIDATE`, plain btree index |
| `migrations/sqlite/0132_catalogue_timestamp_canonical.sql` | backfill, insert/update triggers, plain btree index |
| `crates/db/src/catalogue.rs` | delete `pg_instant`, the `CASE`, and the "three formats" comment block; predicates become one cast per column |
| `crates/db/tests/catalogue_window.rs` | fixtures move to the canonical shape; add the two new tests below |
| `crates/app/tests/milestone_8.rs` | its one `…Z` library fixture moves to `…+00:00` |
   - `db/tests/catalogue_window.rs::every_shape_is_now_one_shape` — the four legacy strings, inserted

## Copy-pasteable code / commands

   UPDATE library_items SET source_updated_at = CASE
       WHEN source_updated_at IS NULL THEN NULL
       WHEN source_updated_at ~ '^\d{4}-\d{2}-\d{2}$' THEN source_updated_at || 'T00:00:00+00:00'
       ELSE to_char((CASE WHEN source_updated_at ~ '(Z|[+-]\d{2}:\d{2})$'
                          THEN source_updated_at::timestamptz
                          ELSE source_updated_at::timestamp AT TIME ZONE 'UTC' END)
                    AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS"+00:00"')
   END;
   

   ALTER TABLE library_items ADD CONSTRAINT library_items_canonical_timestamps CHECK (
        (source_updated_at IS NULL OR source_updated_at ~ '^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\+00:00$')
    AND (last_synced_at   IS NULL OR last_synced_at   ~ '^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\+00:00$')
    AND (created_at       IS NULL OR created_at       ~ '^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\+00:00$')
   ) NOT VALID;
   ALTER TABLE library_items VALIDATE CONSTRAINT library_items_canonical_timestamps;
   CREATE INDEX IF NOT EXISTS idx_library_items_source_updated_at ON library_items (source_updated_at);
   

   sudo -u postgres psql -d darkhold -c "select count(*) from library_items where source_updated_at !~ '…\+00:00$'"   → 0
   

## Verification command and expected output

   verified with a throwaway test), so `+00:00` would have made the database refuse its own
   SELECT count(*) FROM s WHERE r_text <> r_inst;      -- expected 0

## RED/GREEN evidence

   writer's output. Thirty-odd fixtures that mirror production failed, which is how the mistake

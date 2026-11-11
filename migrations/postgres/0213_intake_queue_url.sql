-- M69-23 step 3 — a held arrival can be released, so the queue needs the URL to re-run.
--
-- Step 2 stored the candidate (host, language, words, tags, title, complete) and the decision sentence, which is enough to show an
-- operator *what* was held and *why*. It is not enough to release one: releasing means re-running the import through the ordinary path,
-- and that path is addressed by URL (`create_import_job` takes `source_url`). Storing the host and rebuilding a URL from it would invent a
-- URL — the canonical URL the adapter produced is the only one that resolves to the same work.
ALTER TABLE import_intake_queue ADD COLUMN IF NOT EXISTS source_url TEXT NOT NULL DEFAULT '';

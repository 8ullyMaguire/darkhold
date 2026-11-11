-- M69-23 step 2 — the arrivals a source's intake policy chose to hold.
--
-- The policy (`crates/domain/src/intake.rs`) gives three answers per arrival: bring it in, queue it for a person, or (no rule matched)
-- queue it because nobody wrote a rule for it. **Bring-in already has a home — `library_items` — and queued arrivals had none.** This
-- table is that home, and it is a table rather than a job state for a measured reason: `import_jobs.state` allows
-- queued/running/paused/completed/failed/cancelled and there is **no resume route**, so a paused job would strand the work (a second
-- import of the same URL is refused while a live job exists) and an operator would have no way to release it.
--
-- What is stored is the **candidate as it was judged**, verbatim. A queue row that kept only a title would make the operator re-fetch
-- the page to decide, and the page may have changed since; the decision sentence is stored beside it for the same reason — "queued" with
-- no reason is a support ticket.
--
-- `(account_id, source_key, source_work_key)` is unique: importing the same work twice must not fill the queue with two rows saying the
-- same thing, and the retry after a policy change replaces the earlier row rather than adding to it.
CREATE TABLE IF NOT EXISTS import_intake_queue (
    id              TEXT PRIMARY KEY,
    account_id      TEXT NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    source_key      TEXT NOT NULL,
    source_work_key TEXT NOT NULL,
    -- The candidate, as JSON: host, language, words, tags, title, complete. Nothing queries inside it.
    candidate_json  TEXT NOT NULL,
    -- The sentence from `IntakeDecision::describe()`, which names the rule or says no rule matched.
    decision        TEXT NOT NULL,
    created_at      TEXT NOT NULL,
    UNIQUE (account_id, source_key, source_work_key)
);

CREATE INDEX import_intake_queue_created ON import_intake_queue (created_at DESC);

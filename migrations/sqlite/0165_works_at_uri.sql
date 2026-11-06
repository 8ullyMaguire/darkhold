-- W6 §5: the record's identity is written down, not re-derived.
--
-- The same column and the same partial index as the postgres arm: the two dialects must be equally correct, not equally
-- fast, and SQLite supports a partial index — so this one is a real index rather than a documented no-op.
alter table works add column at_uri text;

create index if not exists works_at_uri_idx on works (at_uri) where at_uri is not null;

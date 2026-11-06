-- W6 §5: the record's identity is written down, not re-derived.
--
-- `docs/FEDERATION.md` §9 makes rollback a purge that walks `at_uri`; a reader can then answer "is this work published
-- to Bluesky, and where" without asking the PDS. The rkey this bridge uses is deterministic in the work id, so a purge
-- *could* recompute it — but a hash cannot say which repo or which PDS a record went to, and a column can.
alter table works add column if not exists at_uri text;

create index if not exists works_at_uri_idx on works (at_uri) where at_uri is not null;

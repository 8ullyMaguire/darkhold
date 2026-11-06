-- M64-09: the reader's own gravity dial.
--
-- One row per pseud, holding the strength that reader asked for. Absent means "whatever the instance says", which is not the
-- same as 0.0 and must not be stored as it: a reader who never opened the dial should follow the instance when the operator
-- changes it.
--
-- The dial narrows and never widens: a stored value is clamped to the instance's strength on read, so an operator who lowers
-- gravity does not leave every reader pinned above the new setting.
create table if not exists reader_gravity (
    pseud_id   text primary key references pseuds (id) on delete cascade,
    strength   real not null,
    updated_at text not null
);

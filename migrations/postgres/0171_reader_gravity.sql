-- M64-09: the reader's own gravity dial. See the sqlite arm.
create table if not exists reader_gravity (
    pseud_id   uuid primary key references pseuds (id) on delete cascade,
    strength   double precision not null,
    updated_at timestamptz not null
);

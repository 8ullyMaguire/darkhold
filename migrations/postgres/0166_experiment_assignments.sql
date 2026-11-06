-- M64-02: a reader's experiment bucket is assigned ONCE and then read, not recomputed.
--
-- See the sqlite arm for the reasoning: storing the assignment is what makes it sticky, and stickiness is the whole
-- difference between an experiment and a random number.
create table if not exists experiment_assignments (
    id uuid primary key,
    pseud_id uuid not null references pseuds(id) on delete cascade,
    surface_id text not null,
    bucket integer not null,
    assigned_at timestamptz not null,
    version integer not null default 1,
    unique (pseud_id, surface_id)
);

create index if not exists experiment_assignments_lookup on experiment_assignments (pseud_id, surface_id);

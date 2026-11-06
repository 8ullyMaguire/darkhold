-- M64-15: the operator can stop an experiment by hand, and hand actions are recorded as such. See the sqlite arm.
create table if not exists experiment_holds (
    surface_id text primary key,
    reason text not null,
    held_at timestamptz not null
);

alter table experiment_decisions add column source text not null default 'automatic';

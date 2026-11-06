-- M64-04: a promotion has to act on something, and the config file is not it. See the sqlite arm.
create table if not exists surface_algorithm_overrides (
    surface_id text primary key,
    algorithms text not null,
    decision_id uuid references experiment_decisions(id) on delete set null,
    created_at timestamptz not null
);

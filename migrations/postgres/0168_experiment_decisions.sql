-- M64-04: a promotion is one row, and a rollback reads it. See the sqlite arm for the reasoning.
create table if not exists experiment_decisions (
    id uuid primary key,
    surface_id text not null,
    day text not null,
    decision text not null check (decision in ('promote', 'demote', 'rolled_back')),
    incumbent_arm integer not null,
    challenger_arm integer not null,
    lift double precision not null,
    evidence double precision not null,
    guardrail_name text,
    guardrail_value double precision,
    consecutive integer not null default 0,
    algorithms_before text not null,
    algorithms_after text not null,
    created_at timestamptz not null,
    rolled_back_at timestamptz
);

create index if not exists experiment_decisions_surface on experiment_decisions (surface_id, created_at);

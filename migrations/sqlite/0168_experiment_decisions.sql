-- M64-04: a promotion is one row, and a rollback reads it.
--
-- The decision function (`darkhold_domain::experiment_promotion`) is pure and reads no storage; this table is what makes the
-- decision survivable. Storing the numbers the decision was made on — not a pointer to a query that has since moved — is
-- what lets an operator argue with a promotion a month later, and what makes a rollback a read rather than a re-derivation.
--
-- `algorithms_before` / `algorithms_after` are JSON arrays of algorithm names, written as text in both dialects: the swap is
-- the material fact a rollback has to undo, and it must travel with the decision rather than be inferred from a config that
-- may have changed since.
--
-- Only verdicts are stored (`promote`, `demote`). A nightly `undecided` row would turn the record into a log nobody reads and
-- bury the two rows that matter.
create table if not exists experiment_decisions (
    id text primary key,
    surface_id text not null,
    day text not null,
    decision text not null check (decision in ('promote', 'demote', 'rolled_back')),
    incumbent_arm integer not null,
    challenger_arm integer not null,
    lift real not null,
    evidence real not null,
    guardrail_name text,
    guardrail_value real,
    consecutive integer not null default 0,
    algorithms_before text not null,
    algorithms_after text not null,
    created_at text not null,
    rolled_back_at text
);

create index if not exists experiment_decisions_surface on experiment_decisions (surface_id, created_at);

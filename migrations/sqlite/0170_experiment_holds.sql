-- M64-15: the operator can stop an experiment by hand, and hand actions are recorded as such.
--
-- Two things live here because they answer the same question — "who decided this?".
--
-- `experiment_holds` is the pause: a surface with a row here is skipped by the sweep, so an operator who sees a guardrail
-- drifting can stop the machinery without editing config or stopping the process. One row per surface: a surface is either
-- held or it is not, and a history of holds would leave the reader to work out which one is live (the history is in
-- `experiment_decisions` for anything that actually changed the algorithms).
--
-- `source` on the decision rows distinguishes an automatic promotion from one an operator ordered. Both are real promotions
-- and both must be recorded the same way, but a month later "why did this change" has two very different answers, and a row
-- that cannot tell them apart is an argument nobody can have.
create table if not exists experiment_holds (
    surface_id text primary key,
    reason text not null,
    held_at text not null
);

alter table experiment_decisions add column source text not null default 'automatic';

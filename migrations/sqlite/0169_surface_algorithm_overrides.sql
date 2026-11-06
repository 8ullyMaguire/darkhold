-- M64-04: a promotion has to act on something, and the config file is not it.
--
-- The operator's algorithm choice lives in `config.toml`, which a running process cannot rewrite and a promotion must not
-- edit behind the operator's back. A promoted experiment therefore writes an *override* here, and the read path takes it in
-- precedence over the configured set. One row per surface: the incumbent is whatever the override says, and rolling back is
-- deleting the row (or writing the previous algorithms into it, which is what the decision row already recorded).
--
-- `decision_id` is the row that argued for this swap. It is not decoration: clearing an override without knowing which
-- promotion installed it is how a platform ends up with a surface nobody can explain.
create table if not exists surface_algorithm_overrides (
    surface_id text primary key,
    algorithms text not null,
    decision_id text references experiment_decisions(id) on delete set null,
    created_at text not null
);

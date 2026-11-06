-- M64-02: a reader's experiment bucket is assigned ONCE and then read, not recomputed.
--
-- The bucket has to be stored for the same reason a hash of the algorithm list cannot give it: when the operator adds
-- or removes an algorithm, everything the hash is computed from changes, and every reader would move arm mid-experiment.
-- Recomputing would silently invalidate the measurement the table exists for, so the row is the truth and the first
-- insert decides.
--
-- `surface_id` is the canonical dotted id from the closed registry (`home.trending`, `discover.feed`), not a route path:
-- a route rename must not split one experiment into two.
create table if not exists experiment_assignments (
    id text primary key,
    pseud_id text not null references pseuds(id) on delete cascade,
    surface_id text not null,
    bucket integer not null,
    assigned_at text not null,
    version integer not null default 1,
    unique (pseud_id, surface_id)
);

-- The read path asks "which bucket is this reader in for this surface" once per request; without this index that is a
-- table scan of every assignment ever made.
create index if not exists experiment_assignments_lookup on experiment_assignments (pseud_id, surface_id);

-- M64-03: per-arm metrics are a daily snapshot, not a query over the raw log at decision time.
--
-- A promotion is a decision made once and argued about later. Reading `recommendation_slots` at decision time gives a
-- number that changes while the argument happens — every new impression moves it — and no way to say what the arm looked
-- like when the decision was taken. One row per (surface, arm, day) is that record, and it is written idempotently so a
-- re-run of the job repairs a day rather than doubling it.
--
-- `guest_readers` is stored, not derived, because it is a guardrail input: the requirement is that one bot cannot decide a
-- winner, and "how much of this arm is guest traffic" has to be answerable from the snapshot the promotion read.
-- `events` counts slots that were followed by the choice event (`chosen_at`), which is the definition this schema fixes.
create table if not exists experiment_snapshots (
    id text primary key,
    surface_id text not null,
    arm integer not null,
    day text not null,
    impressions integer not null default 0,
    readers integer not null default 0,
    guest_readers integer not null default 0,
    events integer not null default 0,
    computed_at text not null,
    unique (surface_id, arm, day)
);

-- The read path asks for one day at a time, newest first.
create index if not exists experiment_snapshots_day on experiment_snapshots (day, surface_id, arm);

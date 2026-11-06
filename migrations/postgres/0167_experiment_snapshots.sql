-- M64-03: per-arm metrics are a daily snapshot, not a query over the raw log at decision time.
--
-- See the sqlite arm for the reasoning. `day` is a date string (`YYYY-MM-DD`) in both dialects rather than a date column:
-- a snapshot is a label for a window, and the window boundaries are computed once by the caller that names the day.
create table if not exists experiment_snapshots (
    id uuid primary key,
    surface_id text not null,
    arm integer not null,
    day text not null,
    impressions integer not null default 0,
    readers integer not null default 0,
    guest_readers integer not null default 0,
    events integer not null default 0,
    computed_at timestamptz not null,
    unique (surface_id, arm, day)
);

create index if not exists experiment_snapshots_day on experiment_snapshots (day, surface_id, arm);

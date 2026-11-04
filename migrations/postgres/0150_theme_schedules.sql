-- M45-50: a taste vector that applies for a window, then stops.
--
-- `ends_at` is stored, not derived, and the rule that reads it is half-open
-- (`starts_at <= at < ends_at`): two adjacent windows — the 25th, then the 26th —
-- must not both apply for an instant, and an event must stop without anybody
-- editing a row at midnight.
--
-- Revoking sets `revoked_at` rather than deleting the row: an event that was called
-- off is a fact worth keeping, for the same reason a saved query keeps its runs.
CREATE TABLE theme_schedules (
    id           TEXT NOT NULL PRIMARY KEY,
    instance_id  TEXT NOT NULL,
    label        TEXT NOT NULL,
    theme_vector TEXT NOT NULL,
    starts_at    TEXT NOT NULL,
    ends_at      TEXT NOT NULL,
    created_at   TEXT NOT NULL,
    revoked_at   TEXT
);

CREATE INDEX theme_schedules_window ON theme_schedules (instance_id, starts_at, ends_at);

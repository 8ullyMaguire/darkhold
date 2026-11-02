-- 0144 — §9.7.6 badges, beginning with the debut badge (audit item 75, M47-14).
--
-- The reasoning (uniqueness is the one-time rule, the bonus is recorded rather than recomputed, why
-- there is no `show_badges` column yet, why "debut" is derived) is in the SQLite file.
--
-- `credits` is BIGINT: ADR 0004 — a value decoded as `i64` is INTEGER on SQLite and BIGINT here.
--
-- Dialect: PostgreSQL.

CREATE TABLE IF NOT EXISTS badge_award (
    id         TEXT NOT NULL PRIMARY KEY,
    account_id TEXT NOT NULL,
    badge      TEXT NOT NULL,
    context    TEXT,
    credits    BIGINT NOT NULL DEFAULT 0,
    awarded_at TEXT NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS badge_award_once
    ON badge_award (account_id, badge);

CREATE INDEX IF NOT EXISTS badge_award_badge
    ON badge_award (badge, awarded_at);

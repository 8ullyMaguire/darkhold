-- M16-14: the operational half of webhooks — secret rotation with a grace window, a per-subscription
-- rate limit, and an endpoint that switches itself off after a receiver stops answering.
--
-- The rate limit and the failure count live on the endpoint because they are properties of the
-- subscription, not of a delivery: a limit stored per delivery would have to be counted and compared
-- somewhere, and "somewhere" is where two processes decide differently.

ALTER TABLE webhook_endpoints ADD COLUMN previous_secret TEXT;
ALTER TABLE webhook_endpoints ADD COLUMN rotated_at TEXT;
ALTER TABLE webhook_endpoints ADD COLUMN rate_per_minute INTEGER NOT NULL DEFAULT 60;
ALTER TABLE webhook_endpoints ADD COLUMN consecutive_failures INTEGER NOT NULL DEFAULT 0;
ALTER TABLE webhook_endpoints ADD COLUMN disabled_at TEXT;
ALTER TABLE webhook_endpoints ADD COLUMN disabled_reason TEXT;

-- 0139 — the profile the estate vouches for, kept beside the link.
--
-- `oidc_links` says "this Polaris subject is this account". It did not keep the name and picture that
-- subject carries, so a site had to call the provider on every render (a network hop per page view) or
-- ignore the display name entirely.
--
-- Both are nullable and are refreshed on every sign-in: they are a cache of what `/userinfo` said, not a
-- second source of truth. A site that cannot reach the provider keeps rendering the last known values, which
-- is the right failure for a profile.
ALTER TABLE oidc_links ADD COLUMN picture TEXT;
ALTER TABLE oidc_links ADD COLUMN display_name TEXT;

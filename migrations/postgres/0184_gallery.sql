-- M16-08: installations and ratings, the two facts a gallery is made of.
--
-- One active installation per (scope, package): the partial unique index is the rule, not a check a route performs,
-- because two concurrent installs of the same package on the same scope would otherwise both pass a check and both
-- insert. The scope is a pseud, so the same account may hold the package under two identities, which is the honest
-- reading of "per pseud scope".
--
-- `version` is the pinned version. History is not kept per version: an update moves this column and the previous
-- version is not a record anybody needs, because the grant that names the version is the consent record.
--
-- Install counts are computed as `uninstalled_at IS NULL` at read time, never stored. A stored counter is the classic
-- way to end up counting reinstalls.
CREATE TABLE extension_installations (
    id            TEXT PRIMARY KEY,
    scope         TEXT NOT NULL,            -- pseud id: an account's identity, not the account
    manifest_id   TEXT NOT NULL,
    version       TEXT NOT NULL,
    update_policy TEXT NOT NULL DEFAULT 'pinned',   -- pinned|minor|any
    installed_at  TEXT NOT NULL,
    updated_at    TEXT NOT NULL,
    uninstalled_at TEXT
);

CREATE UNIQUE INDEX idx_extension_installations_active
    ON extension_installations(scope, manifest_id)
    WHERE uninstalled_at IS NULL;

CREATE INDEX idx_extension_installations_package
    ON extension_installations(manifest_id, uninstalled_at);

-- One rating per scope per package: a scope that could rate twice could move a package's score by editing its own
-- opinion, and the second rating would have no meaning to attach to. `version_rated` records what the reader used,
-- so a review of 1.0.0 is not read as a review of 2.0.0.
CREATE TABLE extension_ratings (
    scope         TEXT NOT NULL,
    manifest_id   TEXT NOT NULL,
    version_rated TEXT NOT NULL,
    stars         INTEGER NOT NULL CHECK (stars BETWEEN 1 AND 5),
    review        TEXT,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL,
    PRIMARY KEY (scope, manifest_id)
);

CREATE INDEX idx_extension_ratings_package ON extension_ratings(manifest_id);

-- M16-13: the theme model. A theme is data -- a palette of named tokens and, for an instance theme,
-- a placement per region -- never a stylesheet. That is what makes "a theme cannot hide the safety
-- controls" a property of the vocabulary rather than a promise in prose.

CREATE TABLE theme_profiles (
    id TEXT PRIMARY KEY,
    scope TEXT NOT NULL,            -- 'instance' | 'reader' -- whose appearance this states
    owner TEXT NOT NULL,            -- '' for the instance, account id for a reader
    name TEXT NOT NULL,
    palette TEXT NOT NULL,          -- JSON map of token name -> value; absent tokens fall back to core
    layout TEXT NOT NULL,           -- JSON map of slot name -> placement; always {} for a reader theme
    state TEXT NOT NULL DEFAULT 'draft',   -- draft | published
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- One published theme per scope and owner: publishing is a move, not a second row. Two published instance
-- themes would make "the instance's appearance" depend on row order.
CREATE UNIQUE INDEX idx_theme_profiles_published
    ON theme_profiles(scope, owner) WHERE state = 'published';

-- A name is how a person refers to a theme in a form; two drafts with one name are a bug in the console.
CREATE UNIQUE INDEX idx_theme_profiles_name ON theme_profiles(scope, owner, name);

-- A preview is session-scoped and expires. Storing it as a row rather than a cookie means the server, not
-- the client, decides whether a preview is still a preview.
CREATE TABLE theme_previews (
    theme_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    PRIMARY KEY (theme_id, session_id)
);

-- Safe mode is a state, not a theme: one row, so turning it on cannot race with turning it on.
CREATE TABLE theme_safe_mode (
    instance_key TEXT PRIMARY KEY,
    active INTEGER NOT NULL DEFAULT 0,
    reason TEXT,
    updated_at TEXT NOT NULL
);

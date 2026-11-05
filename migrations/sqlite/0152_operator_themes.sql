-- M45-69: what the instance is FOR, stated by the operator, visible on its own.
--
-- `instance_themes.theme_vector` is computed from readers' private bookmarks; a season overrides it for
-- a window. Neither lets an operator say "this instance is a dark-progression instance" and be
-- contradicted by the data. This table holds that statement.
--
-- It is NOT blended with the computed vector. A blend would make the claim unfalsifiable: nobody could
-- tell whether `kink` is in the theme because the operator asked for it or because forty readers
-- happened to bookmark it. `seeds` keeps every line with its outcome, so the row can answer "why is
-- this tag here" and "why is my fifth seed not in the vector".
CREATE TABLE operator_themes (
    instance_id    TEXT NOT NULL PRIMARY KEY,
    theme_vector   TEXT NOT NULL,
    top_tags       TEXT NOT NULL,
    seeds          TEXT NOT NULL,
    seed_count     INTEGER NOT NULL,
    resolved_count INTEGER NOT NULL,
    sourced_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL
);

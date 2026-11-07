-- M16-07: a package may have more than one version.
--
-- `extension_manifests` was keyed on `id` alone, so a second version of the same package was a UNIQUE
-- violation. That made the review workflow impossible in exactly the case that matters: publishing 1.1.0 over a
-- published 1.0.0 -- and it made "roll back to the previous published version" a promise about a row that could
-- never exist. The key becomes (id, version).
ALTER TABLE extension_manifests DROP CONSTRAINT extension_manifests_pkey;

ALTER TABLE extension_manifests ADD PRIMARY KEY (id, version);

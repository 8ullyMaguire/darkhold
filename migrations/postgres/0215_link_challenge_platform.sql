
-- M45-43 step 4 — a link challenge can say WHO it is for.
--
-- The §23.2 flow is owner-initiated: the owner mints a code in a browser and hands it to a reader. A guild bot cannot do that — it has a bearer
-- credential, not a session, and it needs to start the flow for a reader inside Discord and then learn whether that reader confirmed. Both halves need
-- the platform identity on the challenge: the bot mints `for discord user 1234`, the reader confirms on the site, and the bot can then ask which of
-- its links belongs to 1234.
--
-- Empty default, deliberately: an owner-minted code (the browser flow) is for nobody in particular, and that is still a valid flow — the reader who
-- holds the code is the one it is for.
ALTER TABLE link_challenges ADD COLUMN platform TEXT NOT NULL DEFAULT '';
ALTER TABLE link_challenges ADD COLUMN platform_user TEXT NOT NULL DEFAULT '';

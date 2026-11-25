# Owner unblock steps (plain English)

Only one row still needs an owner decision. Everything else was retired on
2026-11-25: TA3-2 (keep workers), TA3-3 (browser clears AO3 wall), TA3-B1
(full gate declined), TA3-B2 (no other session) and TA3-B4 (global Discord
registration done and verified).

## TA3-B3 — M63-32 likes/favorites half

What it means: the row needs a safe source for favorites/endorsements. The
public Pawchive favourites API does not exist, so a credential alone may not
help.

1. Pick one: (a) provide a safe, read-only credential path plus the endpoint
   you approve, or (b) approve using the `library_items.likes` values we
   already store as the engagement source.
2. If (a), put the credential in a `0600` file under
   `/personal/secrets/estate/` and tell me only the path.
3. If (b), say "approve library_items.likes as engagement". I will spec the
   backfill before touching data.

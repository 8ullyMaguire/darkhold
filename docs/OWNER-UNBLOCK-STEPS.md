# Owner unblock steps (plain English)

This file is for the rows I cannot clear by myself. For each one: do the step,
then tell me which row number you cleared. I will verify it with one targeted
command, update the state files, and continue.

Run host commands from your usual shell with `ssh thinkcentre '<command>'`.
Never paste a password, token, or cookie into chat — put it in the Hermes
`.env` or a `0600` file under `/personal/secrets/estate/` and tell me the path.

## TA3-2 — import queue is still huge

What it means: about 15,000 jobs and 14,300 imports are queued. The row closes
only when that reaches zero or a small steady tail with no new adapter errors.

1. Decide whether to keep waiting for the workers or to slow/pause an adapter.
2. If you want me to change worker posture or retry policy, say so explicitly.
3. To see the current numbers yourself:

       ssh thinkcentre "sudo -u postgres psql -tA -d darkhold -c \"SELECT state,count(*) FROM import_jobs GROUP BY state\""

I do not change import policy without that decision.

## TA3-3 — AO3 returns 429

What it means: a plain public GET of `https://archiveofourown.org/works/1`
answers HTTP 429, so tag coverage cannot advance from AO3.

1. Re-check from a normal browser. If it works for you, tell me and I will
   re-probe from the host.
2. If it still shows "too many requests", wait for the rate limit to clear.
3. If you have a public cache or mirror URL, tell me the URL. I will use it
   only if it is public and needs no bypass.
4. I will not add auth, rotate identities, or bypass the limit.

## TA3-B1 — full estate gate

What it means: you chose targeted-only verification. A full sweep is the only
thing that closes this row.

1. Say explicitly: "run the full estate gate".
2. Accept that it is a long, full-workspace run, unlike my usual targeted tests.

## TA3-B2 — M69-24 shared taste crate

What it means: another session owns the checkout that has the shared crate.

1. Tell me which checkout is authoritative now, or ask that session to hand it
   over / merge it into `~/code/rust/darkhold`.
2. Then say "M69-24 is handed over". I will adopt the existing crate rather
   than write a second one.

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

## TA3-B4 — global Discord command registration

What it means: registering commands globally (rather than in one test guild)
changes behaviour for every server the bot is in.

1. Approve the global registration explicitly.
2. Put the bot application ID and the target guild/server ID in the Hermes
   `.env` or a `0600` secret file; tell me the path, not the values.
3. Say "global Discord registration approved".

## Already cleared this pass

- M64-06: live data has tagged-and-engaged works; tag_jaccard is served.
- M45-75-live: signed-in pawchive canonical query returns a hit with an old
  source clock; live `source:ao3` returns 200. No visibility was widened.

# The remaining roadmap, ordered for execution

`docs/requirements.csv` holds **66 planned rows** as of 2026-11-08 (10 of them the new M72 batch below). This file is the order they will be taken in and the
reason for that order, so a later session does not have to re-derive it. The ledger stays the source of truth: a row
moves when `scripts/ledger.py upsert` says so, not when this file says so.

Two rules that decide the order:

* **Things a reader can see come before things an operator can see, and both come before infrastructure.** A row that
  changes what a stranger can read or find is worth more than a row that changes what an operator can tune.
* **A row that gates other rows is taken as soon as its blocker clears.** Nine of the M64 rows read a snapshot built by
  M64-01..M64-03; taking them first would mean writing the same aggregation three times.

##### M63-05b unblocked — the download route, 2026-11-11

`GET /derivatives/{id}/download` (`crates/app/src/routes/derivative.rs`) serves the **built bytes**, not the metadata:
`Content-Type` from the derivative's own `output_mime_type`, `Content-Disposition: attachment; filename=work-<work>-<derivative>.<kind>`.

Three refusals, each naming a different fact:

* the work is not readable → the work door's own answer, so a rendition of a work the caller cannot read is not disclosed;
* **no built file** → `404 no such built derivative`, deliberately not `422`: nothing about the request is wrong and a
  client polling a job should not be told it made a mistake (the metadata route carries `state` and `error_message`);
* **a checksum with no blob** → `500`, because that is a broken instance and an empty 200 would hide it.

`crates/app/tests/m63_05b_derivative_download.rs` — **3/3 on SQLite and 3/3 on PostgreSQL**:

| test | what it pins |
|---|---|
| a built rendition is served as the file it is | 200, the recorded content type, an `attachment` disposition ending `.epub`, and body == stored bytes |
| a queued rendition has no file to serve | 404 — a job that has not finished has no bytes |
| a stale rendition is not served even though its bytes are still there | 404 — the state filter, which the queued case alone does **not** exercise |

That last one exists because the first mutant was useless: removing the `state == "ready"` filter left the queued test
green, since a queued row has no `output_checksum` either. The filter's real job is the row that **was** built and then
went stale, and only a stale fixture reddens it (`left: 200` when the filter is removed, restored from `/tmp`).

**Remaining:** `dh rec --link epub` still refuses. The route it needs now exists; wiring the CLI is a second change that
asks the server for each row's derivatives and prints the download URL where one is ready.

## M45-43 — the bot already exists; measured 2026-11-11

The row was planned as an interactions endpoint inside this server, and that plan is **withdrawn**: a census of the estate
found the bot already built, and a second one would have been the third copy of a solved problem.

**Measured (read-only, `rg` over `~/code-local/rust` and `~/code/rust`, 2026-11-11):**

| what | where | lines |
|---|---|---|
| a multi-platform bot **for darkhold** | `~/code-local/rust/darkbot` — `darkbot-core`, `darkbot-adapter` (trait + conformance suite), `darkbot-api`, and `discord`/`matrix`/`telegram` adapters | 12,489 |
| a second multi-platform bot framework | `~/code/rust/fanfic-archivist-bot` — `archivist-core` (`PlatformMessage` IR) + discord/matrix/telegram/slack/irc/fediverse/webhook adapters | 30,691 |
| hand-rolled HTTP-interaction endpoints | asc **818**, darkhold **223** (mine, deleted this turn), suki **170** | 1,211 |
| gateway clients (`serenity` / `twilight`) | none | — |

**What darkbot needs from darkhold, and what exists:** `public/search`, `public/works/{id}`, `works/{id}/kudos`,
`me/tokens`, `me/tokens/{id}` — all present. Its link handshake (`POST bot/link/start`, `POST bot/link/exchange`) does
**not** match §23.2, which is already implemented and better: `/me/bots/{id}/link-challenge` → `GET /bots/link/{code}` (the
reader is shown what is being asked for) → `POST /bots/link/{code}` (confirm, token issued once, to the reader's own
session). darkbot's version has the *bot* holding the code and exchanging it; §23.2 never lets a bot hold a reader's
credential. **darkbot is the one that should adapt**, not this server.

**Verdict on sharing, with the estate's own scoring (leverage = dup × platforms ÷ complexity):**

* *Discord signature verification* (hex decode, `timestamp || body`, PING/PONG) is ~120 lines repeated three times —
  dup 3, platforms 3, complexity 1 → leverage **9**. That is a kit nobody should build: two of its three copies are
  per-product endpoints with no shared API, and the third is a bot that does not use the transport at all. **No crate.**
* *The bot framework* is the real shared component and **already exists**: `darkbot-adapter`'s `PlatformMessage` IR,
  `Adapter` trait (`parse` / `render` / `send`, `capabilities`, `CannotSendPrivately`) and conformance suite. Two
  independent implementations of the same architecture (`archivist-core` and `darkbot-core`) is the census finding worth
  acting on; converging the two IRs is a crate-plus-wiring job for whoever needs the *second* non-darkhold bot.

**So the row becomes:** point darkbot at the live instance, adapt its two link calls to §23.2's three doors, and prove one
command end to end in a real guild. No new Discord code in this server; the endpoint written this turn was deleted rather
than kept as dead surface (`git status` clean at `4aa91888`).

## Deployed 2026-11-11 08:53 — M45-45 step 3 (`0.1.0+4b686729`, schema 0207)

One deploy carried the proposal tables and the three doors. Measured, from `/tmp/deploy-m4545.log` on the host:

| check | value |
|---|---|
| tree | fast-forwarded to `4b686729` before the build |
| dump | `darkhold-20261111-082250-pre-m45-45.dump`, **3,740,163,448** bytes, **399** `TABLE DATA`, verified with `pg_restore -l` |
| build | `== built 08:52:59`, 16 min from the dump's finish |
| artifact | `taste-proposal-strings=2` — the path is inside the binary, not only in the tree |
| migrate | schema **0207** (0206 `taste_lens_proposals`, 0207 `taste_lens_cause`) |
| tables | 2 of 2 present (`taste_lens_proposals`, `taste_lens_votes`) |
| service | `active` |
| meta | `0.1.0+4b686729` |
| doors, anonymous | `open-anon=401`, `read-anon=401`, `vote-anon=401` — all three exist and answer the session gate |

**401 is the pass condition here**, and it is the only cheap one available unauthenticated: a 404 would mean the mount is
still wrong, and 405 would mean the method is. That is the check this deploy exists to make, because the same three
routes answered 404 for two sessions' worth of theories about handlers before the nest was found.

## M45-52 — DSA notice-and-action, measured and planned 2026-11-11

What exists today, read rather than assumed: `routes/governance.rs` serves `POST /reports`, `GET /reports`,
`GET /reports/{id}`, `GET /moderation/queue`, `POST /moderation/reports/{id}/assign` and
`POST /moderation/tasks/{id}/decide`. The decision itself is `governance::decide_task(db, task_id, reviewer, outcome)` —
an outcome string and an audit entry. There is **no ground, no statement of reasons, and no delivery to the person whose
work was acted on**: the notifier gets an answer, the author gets a sanction with no explanation, and nothing records
*why* in a form an appeal or a regulator could read.

**What the row needs, in the order it can be checked:**

1. **A decision carries a ground.** `statement_of_reasons(id, task_id, outcome, ground, facts, legal_ground, decided_by,
   decided_at, appeal_until)` — migration `0208`. `ground` is a closed vocabulary (`illegal_content`,
   `incompatible_with_terms`, `spam`, `copyright`), because Article 17 requires the *category*, and free text is not a
   category. `facts` is the reviewer's one paragraph; `legal_ground` names the rule invoked.
2. **The statement is written with the decision, not after it.** `decide_task` gains a required statement argument; a
   decision without one is refused `422` naming `ground`. If it is optional, the queue will contain decisions with no
   reason within a week.
3. **Both sides are told.** The notifier receives the outcome and the statement id; the author receives the same
   statement when their work is acted on. The existing notification machinery carries it; no second inbox.
4. **An appeal window is a field, not a convention.** `appeal_until` is stored (`+30 days` default) and `POST
   /appeals` refuses a notice after it, naming the deadline — the appeal door exists already (`open_appeal`).
5. **A transparency log is a count, not a list.** `GET /statements/summary` returns counts per ground and per outcome,
   the DSA's periodic report in miniature. No statement ids, no accounts: an aggregate that cannot be deanonymised.
6. **DMCA intake stays separate.** A copyright notice has statutory elements (identification of the work, the
   infringing material, a good-faith statement, a signature) that DSA notices do not; folding them into one form makes
   one of the two non-compliant. `POST /reports` gains a `kind: dsa|dmca` discriminator and the DMCA arm validates its
   own four elements.

**Acceptance, per step:** (1) `a_decision_without_a_ground_is_refused_naming_the_field`; (2)
`the_statement_written_with_the_decision_names_the_reviewer_and_the_deadline`; (3)
`the_author_is_told_when_their_work_is_acted_on`; (4) `an_appeal_after_the_window_is_refused_naming_the_deadline`; (5)
`the_summary_counts_grounds_and_never_names_an_account`; (6) `a_dmca_notice_missing_a_statutory_element_is_refused`.

Steps 1–2 are one sitting and are the spine; 3 uses the notification machinery already in the tree; 6 is the only part
that touches `submit_report`'s contract. The row is `planned` and this plan is the thing to implement from.

## Deployed 2026-11-11 11:53 — `0.1.0+ab718f39`, schema 0209

One pass shipped M45-52 steps 3 and 6, M45-55 and M63-15. Measured on thinkcentre, verbatim from
`/tmp/deploy-m4552c.log`:

```
applied 1 migration(s) to the postgres database:  0209_dmca_notices
== active=active
== version=0.1.0+ab718f39
== summary-status=200   == summary-body={"grounds":[],"min_count":5}
== forms-status=200     == forms-dmca-fields=1
== templates-anon=401   == dsar-anon=401
== dmca-partial={"error":{"code":"VALIDATION_FAILED","message":"a copyright notice must contain this element (17 U.S.C. 512(c)(3))","field_errors":{"infringing_material":…
== schema=0209          == tables=dmca_notices,statement_of_reasons
== doctor-live:
ok    instance   http://127.0.0.1:8081 version 0.1.0
ok    clock      local time is 0s from the server
ok    version    dh 0.1.0 matches the server's API line
ok    tls        plain HTTP to localhost: nothing leaves the machine
warn  token      no token configured: public verbs work, session verbs will answer 401
```

The `401`s are the pass: both operator doors are reached and the session gate answers. The `422` is step 6 working on a
live instance — a DMCA notice with one element missing is refused **naming that element**, not with a generic message.

**Two failed attempts, and what they cost.** The first pass (`112909a6`) shipped steps 1/2/4/5 at 11:00. The second
(`b8811a2b`) died at 11:07 **mid-compile** when the ssh that launched it timed out and took the parent shell with it — the
log simply stops, with no build error and no OOM line. That is the recorded lesson re-learned: a deploy launched as a
foreground ssh dies with its caller. The third pass ran under `nohup` and was polled by reading its log for `== done`,
**not** by `pgrep -f deploy-m4552c.sh` — which matches the polling command's own command line and reports "finished"
while the build is still running. Two ways to lie to yourself about a build, both of them mechanical.

## M63-15 `dh doctor` built and proved — 2026-11-11

The row: "diagnoses token scope, clock skew, TLS, DNS and API version drift, one suggested fix per problem". Five checks,
one request each where possible, and **nothing is repaired** — a diagnostic that fixes what it diagnoses cannot be run to
find out what is wrong.

| check | measured from | fails or warns |
|---|---|---|
| `instance` | `GET /api/v1/meta` | **fail** when nothing answers (the fix names the tunnel, the service, `--base-url`) |
| `clock` | the server's own `Date` header, signed skew in whole seconds | **warn** past `SKEW_TOLERANCE_SECONDS = 60`, fix `timedatectl set-ntp true`; a missing header is a warn, because "cannot rule skew out" is not "clean" |
| `version` | `major.minor` of the server's version against this binary's | **warn** on an API-line change; a patch or a `+sha` suffix is not drift |
| `tls` | the base URL's scheme | **fail** on plain HTTP to anything but localhost — a token in clear text is a token in every log on the path |
| `token` | `GET /api/v1/me/credential` with the resolved token | **warn** when absent (public verbs still work), **fail** on `401`/`403` |

`--json` prints the same report as one object. **Only `fail` changes the exit code** (`1`); a warning leaves it `0`, because
scripts run `dh doctor` as a preflight and a preflight that fails on a warning is a preflight nobody keeps.

`crates/cli/tests/doctor.rs` — **2/2**, against a one-shot stub that serves `meta` with a `Date` header: a matching
instance reports `instance`/`version`/`clock` ok, `token` warn and exits **0**; a server two hours behind reports
`clock: warn` with `7200` in the detail and `NTP` in the fix, and still exits **0**. The unit tests in `doctor.rs` cover the
signed skew, the API-line comparison, and that a warning is not a failure (`3/3`). Mutant: the skew comparison disabled →
`left: "ok" / right: "warn"`.

**DNS is deliberately not a separate check**: it is what `instance` fails on, and a second check that says "could not
resolve" after the first already said "nothing answered" is two rows for one fact. The row's list is answered by five
checks, not five modules.

## M69-15's gate reached a door — publishing, and only publishing — 2026-11-11

The rule had no caller, so nothing was gated. It has one now, and the choice of *which* door is the record:

* **`POST /works` refuses, and rating does not.** A probation on ratings would be a gate nobody can graduate from, because
  rating is how the floor is reached. Publishing is the act a taste gate is actually about: a newcomer may read and may
  rate, and may not put work in front of strangers until the floor is met.
* **The refusal is `422` with `field_errors.membership`.** `AppError` has no variant that pairs a `403` with field detail,
  and a reader told "access denied" learns nothing. `"rate 3 more work(s) to finish probation; the window closes in 14
  day(s)"` is the whole point of the row's "so it is written with its exits". If a `403`-with-fields variant appears later,
  `membership::refusal()` is the one place that changes.
* **`GET /auth/membership` tells the reader where they stand** — `full`/`probation`/`redirect`, the floor, the population,
  days left, samples needed, and the passport link for a lapsed account. A gate a person cannot see is a gate they cannot
  finish.
* **Off means free.** `evaluate()` returns before any query when `membership_probation` is false: an instance that does not
  use the feature pays one comparison, not a rating count per publish.

Store: `membership_facts` (age through the existing tenure parser, `taste_samples` = the reader's undeleted ratings,
`grandfathered` = `created_at < membership_grandfather_before`) and `reader_population` (active, **non-guest** accounts —
counting guests would let the gate satisfy itself).

`m69_15_membership_gate` — **4/4 SQLite and 4/4 PostgreSQL**: off publishes as before and reports `gate_off`; on probation the
publish is `422` naming `3 more work(s)` and the standing reports `samples_needed: 3, days_left: 14`; after three ratings the
publish is `201` and the standing says `graduated`, no restart; an account older than the cutoff publishes and reports
`grandfathered`. Mutant: the guard removed → both probation tests fail `201` vs `422`.

**Two faults found by running the thing.** `accounts.is_guest` is an **INTEGER** on PostgreSQL — unlike `bookmarks.is_public`,
which is a boolean — so `is_guest = FALSE` was a `500 operator does not exist: bigint = boolean` on every gated request, and
SQLite could not see it. And `route_inventory` caught that `/auth/guest` had **never been tabled** since GS-1: adding a
sibling route is what exposed it. Both rows are in `ROUTE_TABLE`; the suite is 5/5.

**Second door, same rule (same day).** `POST /api/v1/works/{id}/comments` is gated too: a comment is visible to strangers,
which is exactly why publishing is gated, and the guard sits *before* the body validation so a refused reader is not told
their words were the problem. `m69_15_membership_gate` is **5/5 on both engines**; the new case builds a full member with
three ratings, publishes a work, and a probation reader's comment on it is `422` naming `membership`. Mutant: the comment
guard removed → the comment is accepted and the test fails.

**Third and fourth doors, and the row closes.** `POST /forums/{category}/topics` and `POST /topics/{id}/replies` are gated
the same way; the forum guard sits *above* the category's `min_trust`, because "how much standing this room asks for" and
"this reader is on probation" are different questions and a room with `min_trust = 0` must not be a way around the gate.
The test proves both directions in one room: the probation reader is refused `422` naming `membership`, and a reader who met
the floor opens a topic in that same category — so the refusal cannot be the category.

**What is gated, and what is deliberately not** (the boundary is the row, so it is written down): publishing, comments,
topics and replies — every public write act a stranger can see. **Ratings are not gated**, because rating is how the floor is
earned and a gate on ratings is a gate nobody can graduate from. **Private acts are not gated** (messages, group posts):
they are not what a stranger sees, and their own room rules already apply.

`m69_15_membership_gate` — **6/6 SQLite and 6/6 PostgreSQL**. Mutants: the works guard removed → two tests fail `201` vs
`422`; the comment guard removed → the comment is accepted.

## M70-13 ratchet pass — 38 → 37, and three kept on purpose — 2026-11-11

Method (the row's method): regenerate, then count references for every orphan; definition-only is `1`.

Of 38, **four** were definition-only. One is deleted:

* `atproto::published_uris` — it existed so the bridge could ask "what is already published?", and the bridge does not need to ask:
  `publish_work` derives the record key from the work id (`rkey_for(work.id)`) and PUTs, so republishing overwrites the same record.
  **Idempotence by key beats a pre-check**, and a pre-check nothing calls is debt.

Three are kept, and the reason is that each is a **surface**, not dead code:

* `recommendation_slots::list_response_slots` returns `SlotExplanation` — the "why was this served to me" evidence. Deleting it deletes
  the evidence of a feature, not the feature; what it needs is a route, which is a product decision about who reads it.
* `trust_events::list_all` — the operator trust trail, same shape of decision.
* `sessions::mark_email_verified` — the odd one: darkhold has **no mailer**, so this is not a half-built feature with a route missing but
  a column and a setter for a flow that does not exist. The decision is whether darkhold verifies email at all; until that is answered
  the function stays in the baseline rather than being deleted on a guess.

`cargo check -p darkhold-db` clean, `orphan_queries` 2/2, `gate.sh repo` ok, baseline now **37**.

## M69-24 censused — the shared taste crate, and the one dependency that blocks the copy — 2026-11-11

**census: REUSE the mechanism, BUILD the crate, and the crate cannot be `darkhold-taste` as it stands.** Measured, no code
written:

| fact | measurement |
|---|---|
| a shared taste crate exists | **no** — `~/code-local/rust` has `polaris-candidates`, `polaris-eval`; no `polaris-taste` |
| the duplication is real | `rg -l -i 'fn blend|affinity'` over the estate: darkhold **55** files, suki **38**, ascend 7, whitebois 4, tessera 4, orbital 4, commons 1 |
| darkhold already has a taste crate | `crates/taste` — "the catalogue, the graph, the ranker and the budgets", lib + binary |
| can it be the shared one | **not as it stands**: it depends on `darkhold-domain` |
| the sharing mechanism | already in use — `crates/taste` depends on `polaris-eval` by relative path, commented *"path dependency until the Forgejo registry answers"* |

That last line is the row's shape: the estate already publishes a shared crate by path and intends the registry later, so the
mechanism is REUSE. What is not reusable is the *contents*: `darkhold-taste` speaks in `darkhold-domain` types, so extracting
it means deciding which types move first — a design decision, and the actual work of this row.

**Decision recorded.** The shared crate is `polaris-taste`, holding the pure parts that speak only in domain primitives
(`budget`, `diversify`, `ladder`, `embed`) plus the affinity/blend math; the product-shaped parts (`catalogue`, `graph`,
`collab`, `health` — each reads a table or a product's own rows) stay in each product, behind a trait the shared crate
defines. The JSON schema set ships versioned with the crate, and the **passport** schema is the one that must be byte-identical
on both sides, because a passport whose semantics differ between sender and receiver is worse than none.

**Not started, deliberately.** No crate was created: a crate nothing depends on is the "component nobody calls" failure this
repo pays for repeatedly, and an adoption row's first commit must be a dependency, not a `pub mod`. The next step is the
domain split, then the four pure modules, then darkhold depends and **deletes its copies** (adoption proved by deletion), then
suki.

## M69-23 half built — the intake policy, and why the Saved Filter could not be it — 2026-11-11

**The census found the object the row names, and measured that it cannot do the job.** A saved view's document is either a
`LibraryQuery` (`shelves`, `tags`, `statuses`, `source`, `updated_since`, `sort`) or a catalogue query *string* for
`GET /catalogue`. The first three fields are per-account facts about a reader's shelf — an arriving work has no shelf, no
reading status and no "updated since" for this instance — so a policy written in that language would parse, evaluate, and
mean nothing. The second is a query for a route that reads existing rows, not a predicate over an arrival. BUILD.

`crates/domain/src/intake.rs` is an ordered-rule policy over a flat candidate (host, language, words, tags, title, complete),
with three properties the tests exist to hold:

* **first match wins** — two overlapping rules must not both fire, and the operator's ordering has to mean something (proved
  by putting an exclusion above a take rule);
* **no match queues, never admits** — a policy that admitted by default would make a missing rule indistinguishable from a
  deliberate one;
* **every decision is explainable** — `IntakeDecision` carries the rule index and a sentence, which is the row's actual
  claim: the difference between a policy an operator can inspect and one they have to trust.

7/7 domain tests (matching rule names itself; one failing clause falls through; unmatched queues; **an empty policy admits
nothing**; first-match-wins; tag-any and title-excludes; every decision renders a sentence). Mutant: the no-match default
changed to `BringIn` → three tests fail.

**Second pass, same day — the store and the operator door.** Migration `0211` adds `sources.intake_policy_json` (both engines,
default `'{}'`, which the domain reads as *admit nothing, queue everything* so an unconfigured source cannot import on its own).
`Source` carries the document; `imports::intake_policy` distinguishes *no such source* from *a source with no rules*, because one
is a 404 and the other is a policy. Three operator routes: `GET /admin/sources`, `PUT /admin/sources/{key}/intake-policy`, and
`POST /admin/sources/{key}/intake-policy/test`. The `PUT` **refuses a document that does not parse and stores nothing** — the
stored form is what the importer will run, and `"then": "Bring"` would match nothing while looking configured. The dry run writes
nothing and answers with `IntakeDecision::describe()`, so the operator reads *why*.

`m69_23_intake_policy` **4/4 SQLite and 4/4 PostgreSQL**. The PostgreSQL run found two engine bugs the SQLite run could not: `LIMIT 1`
on `accounts` picked whichever test's account came first in the shared scratch DB (now looked up by handle), and `pseuds.account_id`
is `UUID`, so reading it into a `String` needs `::text`. Mutant: the parse validation replaced with `unwrap_or_default` → the
refusal test fails `200` vs `422`.

**Still missing, and it is now one thing:** the call at the importer's intake point. M67-03/04 decide by host and rule alone, so
the policy today is storable, inspectable and testable and **nothing consults it**.

### The wiring census, and why it is three steps rather than one call

Measured 2026-11-11 before touching the import path:

| question | answer |
|---|---|
| where an arrival is decided | `crates/app/src/imports.rs`, after the M69-04 tombstone check and immediately before `imports::upsert_library_item` — the only point where title, language, words, status and source URL are all in hand |
| what a candidate can know there | title, language, word count, host, complete — **not tags** (derived later from the text), so a tag rule is inert at intake and must say so rather than evaluate against an empty list |
| where a Queue verdict lands | **nowhere today.** `import_jobs.state` has no `held`, there is **no resume route** (`/cancel` and `/retry-failed-units` exist, resume does not), and a paused job strands the work because a second import of the same URL is refused while a live job exists |
| is there an intake queue table | no. `federation_inbound` is the federation inbox, a different feature; reusing it would put an AO3 arrival in a federation queue |

**Decision, recorded rather than coded**, in three parts:

1. **The policy is inert when its rule list is empty.** `{}` is the default on the 14 live sources, and "configured nothing" must keep
   behaving exactly as before — otherwise the deploy that ships the wiring pauses every import on the instance. This is a different
   question from `intake::decide` (which answers "what does this policy do" and says *admit nothing* for no rules); the importer asks
   "is there a policy at all".
2. **A Queue verdict writes a row** to a new `import_intake_queue` (id, account_id, source_key, source_work_key, candidate_json,
   decision, created_at) and marks the job `completed` with a report naming the queue — so the arrival is visible and the same URL can be
   retried once the policy changes.
3. **The release door** lists queued arrivals and re-runs one through the normal import path, because a queue nobody can drain is exactly
   the doorless-surface fault M70-13 exists to name.

**Step 2 built (same day), and the census corrected.** Tags *are* available at the decision point — `SourceWork.tags` carries them — so a
tag rule is evaluated, not inert. Migration `0212` adds `import_intake_queue` (unique on account+source+work, so a re-import replaces the
row); `intake_candidate(&SourceWork)` builds the candidate from what the adapter reported (host from the canonical URL — a missing host is
the empty string, which no `host_in` rule matches, so an unparseable URL queues rather than sneaking in); the guard sits after the tombstone
check and before `upsert_library_item`, behind `is_configured()`, and a held arrival finishes `completed` with
`{intake:{queued,decision}}`.

`milestone_6` **49/49 SQLite**, and both new tests pass on PostgreSQL. Mutant: `is_configured()` forced false → the held test fails.

**Step 3 built (same day) and the row closes.** Migration `0213` adds `import_intake_queue.source_url` — releasing means re-running the ordinary
import, and that path is addressed by the canonical URL the adapter produced; rebuilding a URL from the stored host would invent one.

* `GET /imports/intake-queue` (Authenticated) — the caller's own arrivals with the candidate and the decision sentence.
* `POST /imports/intake-queue/{id}/release` (Pseudonymous) — **re-consults the policy** rather than trusting the stored decision. An operator
  who widens a source's rules has changed the answer for every held arrival of that source; a door that trusted the row would make them release
  each one by hand, and a door that skipped the policy would be a way round the gate. Still held → `422` naming the reason; admitted → the
  ordinary import path, and the queue row is cleared **after** the job exists so a failure between the two leaves the arrival queued.

`m69_23_intake_policy` **6/6 SQLite and 6/6 PostgreSQL**. PG again earned its keep: the insert had 7 binds for 8 parameters because the new
`source_url` bind had been added to the SQLite arm only. Mutant: the release door's policy check forced false → `202` vs `422`.

**Deviation, recorded:** the document is not a *Saved Filter*. A saved view's document is a `LibraryQuery` of per-account shelf facts or a
catalogue query string, and neither can be a predicate over an arriving work. `darkhold_domain::intake::IntakePolicy` meets the row's actual
claim — an operator can inspect and test what a source will take, and what it will hold.

## M69-15 half built — the membership rule and its switches, with no door enforcing it yet — 2026-11-11

**census: REUSE nothing, and the near-hit is a word collision.** `probation` appears four times in `tessera` — and it is a
*crawler-politeness* rule (`crates/tessera-kernel/src/scout.rs`: a new domain gets a small allowance that grows with
observed politeness). Its subject is a remote host, its state is a request allowance, and it has no idea what a reader is.
darkhold has none. BUILD.

`crates/domain/src/membership.rs` is the rule with no clock and no database, because all five of the row's exits are
boundary claims and a boundary is only testable where the clock and the store are absent:

* **off unless switched on** — a switch that is off cannot put a reader on probation;
* **below `membership_population_threshold` nobody is gated**;
* **the floor rises with the instance** (`floor_base + floor_per_thousand` per thousand readers beyond the threshold) and
  **never falls** — a falling floor would put readers who already graduated back on probation;
* **early graduation** — a newcomer who meets the floor on day one is a full member, because the gate is about taste and
  not about waiting out a clock;
* **time-bounded, then grace, then redirect** — `Probation { days_left, samples_needed }` during the window and the grace
  that follows it, then `Redirect { samples_short }`. Never deletion: the account keeps its bookmarks, positions and pseud,
  and the person may come back.

**Config is wired, because the row says "the floor is configurable".** Six fields on `GovernanceConfig` (default off, 1000,
14, 30, 3, 2), the same six as optional keys in `[governance]` — under `deny_unknown_fields`, so a typo is an error rather
than a setting that silently does not apply — the merge into the file config, and `GovernanceConfig::membership_rules()`,
a conversion that exists so a field the mapping forgets is a compile error instead of a switch that reaches nothing.

**Evidence**: 7/7 domain tests, and one in `config_sections` that loads a real `darkhold.toml` through the real `--config`
door and asserts the *configured* rules are the ones that decide. Mutants: the floor's rise removed → two tests fail (3 vs
11); `membership_rules()` hardcoding `enabled = false` → the config test fails, which is the switch-reaches-nothing case.

**The row stays `planned` and the missing half is enforcement.** `decide()` is called by nothing: no reader is gated yet. It
belongs at the read/write doors (the `Audience` extractors and the `Scope::require` path) with the passport redirect target,
it is a live-surface change, and it has to ship behind the switch and be verified on the deployed instance. The `taste_samples`
count and the grandfathering date are two store queries that do not exist either.

## M66 GS-5 half built — the jury rule, and the four things still missing — 2026-11-11

**census (measured, not assumed): REUSE nothing.** The rule is spec'd in `docs/spec/14-governance.md` (random draw, level ≥
`jury_min_level` default 3, size `jury_size` default 5, excluding the reporter and the reported, a fresh panel on appeal, an
elastic fallback of `jury_elastic_min` default 3). The config exists — 29 references in `app/src/config.rs`. The privilege
exists — `Privilege::JuryService` on level 5. And a grep for `jury` over `crates/db/src` and `crates/app/src/routes` finds
**two comments and no code**. BUILD.

`crates/domain/src/jury.rs` is the rule with no database and no clock — that split is the point, because it makes the two
properties that matter testable:

* **random but reproducible.** The seed is a parameter, not `thread_rng()`, so an audited decision can be re-drawn months
  later. Candidates are sorted by id before the shuffle, so the database's row order is not part of the draw.
* **exclusions are structural.** The reporter, the reported and (on appeal) the original panel are removed *inside* the
  draw. A caller that forgets is a jury containing the person who filed the report, and nobody notices until the decision
  looks strange.

**The elastic fallback was a decision, not a transcription.** §14 says "drawn from the top of the actual ladder with
`jury_elastic_min` (default 3) members". Two alternatives were rejected: pad *down* from the next rung (a panel with
members the rules did not choose) and quietly return a short panel (a two-person jury on a twenty-person instance can be
stacked). Below `elastic_min` the draw refuses with `PoolTooSmall { available, required }` — the honest answer to "there is
no pool" is an operator, not a panel of whoever is left.

**7/7 domain tests**: the panel comes from the eligible rung; the same seed draws the same panel and a different seed does
not; candidate order does not change the draw; the reporter and the reported never sit across 40 seeds; the elastic fallback
is 3 members, not 5; a two-person pool is refused; an appeal's exclusions produce a disjoint panel. **Mutants**: the
exclusion filter disabled → both exclusion tests fail (the panel literally contained `reported`); the elastic size set to
`jury_size` → the fallback test fails 5 vs 3.

**The row stays `planned`,** and this is the missing half rather than a formality: (1) the store read of the pool and the
persistence of a drawn panel — a panel that is not recorded gives the appeal rule nothing to exclude; (2) the route that
draws when a report needs a jury, behind `JuryService`; (3) the appeal path honouring `appeal_fresh_jury`; (4) a decision
surface that records each juror's verdict and closes the report. A false `implemented` on a governance surface is worse
than an open row.

## M66 GS-4 built and proved — trust reads the acts, and the karma ledger is refused — 2026-11-11

The row said "elastic trust from karma, not a parallel score", naming 99 karma references and 52 trust references as
evidence that both numbers were already here. They are — and the tree had already decided, in the header of
`crates/db/src/typed_votes.rs`, that **nothing outside that module and its display route reads `forum_karma`** (spec §35.2:
karma never gates trust, moderation, ranking or credits).

**So the row's work was the opposite of fusing them.** Wiring the ledger into trust would make standing farmable by a vote
ring — and *demotable by silence*, because karma decays and trust deliberately does not (the velocity work floors a level
rather than take one away). The reconciliation the row is about already exists and is the right one: both numbers are
derived from **the same acts**, and only one of them is ever read as standing. Trust's `vote_score` axis counts public
ratings on published work; karma is a weighted, decaying display of a subset of the same votes; the ledger stays closed.

That is cheap to lose, so `m66_gs4_trust_from_karma` (**2/2 SQLite and 2/2 PostgreSQL**) pins it from both ends:

- **the derivation is real**: twelve published works carrying public five-star ratings raise the `vote_score` axis above an
  identical account with none, so the axis is not a constant dressed up as a ladder;
- **the ledger buys nothing**: an account holding five million basis points of `forum_karma` and no acts whatsoever has
  exactly the level of an identical account with no ledger, and a karma-holding *newcomer* does not outrank an account that
  simply stayed for forty days.

**The mutant is the fake the row warned about, inverted so that it kills itself**: summing `forum_karma` into `vote_score`
takes the second test from `(2, 0)` to `(4, 50000)` and fails. A test that "trust rises when karma rises" would have passed
on that mutant and proved nothing. PostgreSQL note: every id column there is `uuid`, so the test derives its ids from names
with two FNV-1a passes rather than adding the `uuid` v5 feature for a helper.

## M66 GS-3 built and proved — and the CSRF layer was mounted on one router of two — 2026-11-11

**What the row actually was.** GS-1 created the guest and issued the cookies; GS-2 proved the in-place upgrade. The row's
claim — *a cookie-backed pseudonym that owns what it writes* — had never been measured through a door, because GS-2's test
inserted its bookmark with SQL. Written as a door test, it failed on the first run, and not for the reason expected:

```
a_cookie_alone_cannot_write ... left: 201, right: 403
```

`verify_csrf` was layered on **`account_routes`** (the auth/pseud subtree) and nowhere else. Everything merged into the
`api` router — bookmarks, kudos, comments, reading positions, the roadmap — accepted the session cookie alone. And the
guard is written to skip a request with no `SessionUser` in the extensions, which is exactly what a *missing mount* looks
like: it was in the file, running on one subtree of four, refusing nothing and logging nothing.

**The fix is three lines and an ordering argument.** `verify_csrf` is now a layer on `api`, added *before*
`load_session` in source order so that `load_session` remains the outermost layer — the extension has to exist when the
guard runs. Reversing them would restore the silent skip while the layer looked present, which is the failure this row
was about.

**Proof** — `m66_gs3_guest_pseudonym`, **3/3 SQLite and 3/3 PostgreSQL**:

1. the session cookie is the whole identity: a second client built from nothing but the `Set-Cookie` values is the same
   account and the same pseud, and the bookmark the guest wrote is on that shelf;
2. a cookie alone cannot write: the same POST without the CSRF header is 403 and leaves no row, the same POST with the
   token is 201 — so the refusal is the token, not a blanket denial of guests;
3. what the guest wrote through the door is what registration keeps: the reader logs in the next day with the new
   password and their bookmark is there.

Mutant: the api-level layer removed → `a_cookie_alone_cannot_write` fails (201 vs 403). Tests 1 and 3 assert wiring that
already existed; test 2's mutant is the one that proves the change.

**Repaired alongside:** `m54_bot_link` had 3/25 failing for a reason the CSRF fix surfaced — `sign_in_as` registered the
second account before clearing the first session's cookies, and GS-1 refuses registration from a registered session (422
`saying session`). The clear now precedes the register; the suite is 25/25. Blast radius of enforcing CSRF on `api`,
measured rather than guessed: `milestone_45_roadmap` 55/55, `reader_page` 7/7, `m45_59_multi_link` 3/3,
`m45_55_compliance_tooling` 3/3 — the clients already send the header, which is why the hole survived.

## M66 GS-2 built and proved — what a guest did stays theirs — 2026-11-11

**The decision, first.** Registration from a guest session **upgrades the account in place**. It does not fold one account
into another. §14.2's fold is for merging two *distinct* identities — and the re-parenting engine it describes does not exist
in this tree (`identity.rs` says "the fold itself is M20's"; no re-parenting code ships), so a fold-shaped GS-2 would have
been a new engine whose first user is the one case that does not need it. In place is also strictly safer for the rows the
row is about: `bookmarks.account_id` and `reading_progress.account_id` **are** the key, so leaving the id alone means the
artifacts cannot be lost by a partial fold.

That required two small things and one subtle one:

* `identity::upgrade_guest` — `UPDATE accounts SET email, age_state, is_guest = 0 … WHERE id = ? AND is_guest = 1`,
  answering `bool`. A non-guest is *refused*, not silently registered over: the predicate is in the statement, so a race
  cannot turn "register" into "rewrite somebody's address".
* `identity::rename_pseud` — a handle move, which `update_pseud` deliberately refuses to do.
* **the pseud is renamed, not replaced.** `reading_progress.pseud_id` names it, so a fresh pseud would orphan exactly the
  rows this row exists to keep. The test asserts the pseud id is unchanged across the upgrade.

`register()` now branches on an existing guest session: same account id, onboarding defaults stored (the guest never had
them), age assessment recorded, trust recomputed, and the answer is **200** where a brand-new account answers **201** —
because no account came into existence.

`crates/app/tests/m66_gs2_guest_upgrade.rs` — **2/2 SQLite and 2/2 PostgreSQL**. The test writes the artifacts as the guest
(a bookmark and a reading position at 42%), registers, and then reads the rows back: same account id owns both, the pseud id
is unchanged and its handle is the chosen one, `is_guest` is false, the synthetic `guest+…@guest.invalid` address is gone, and
the new credential logs in as that same account. A second register is refused with `session` named — the email was fine —
and nothing about the first one is rewritten.

Mutants: the guest branch removed → **both** tests fail; the pseud rename replaced by a fresh pseud → the pseud-id assertion
fails. PostgreSQL dialects this test paid for: `bookmarks.id` and `subject_id` are `UUID`, `reading_progress.id` is `UUID`,
and `bookmarks.is_public` is `BOOLEAN` (so `0` is a type error and `FALSE` is not).

## M66 GS-1 built and proved — read before registering, and the gate comes first — 2026-11-11

**Deployed 14:07** — `0.1.0+4ea4fb9a` at schema **0210**. Live: `POST /api/v1/auth/guest` with no body answers `422 VALIDATION_FAILED` — *"accept the current terms (version 1) before reading as a guest"* — with `field_errors.consent`, and `select count(*) from accounts where is_guest = 1` is still **0**, which is the assertion that matters: the refusal created nothing. `consents` exists (1 table), and `/api/v1/meta` publishes `content_policy.consent_required_for_guest = true`, so a frontend can render the interstitial from the instance rather than hard-coding it.

**A merge nobody scheduled, caught before it deployed.** The thinkcentre checkout had one commit the workstation did not (`75319a06`, two new files: a seed-from-reading-list roadmap and its parser), and the workstation had two the checkout did not. The first deploy attempt silently built the *old* HEAD because `git fetch` failed on a transient DNS error and `--ff-only` then reported nothing to do — the log said `== head 75319a06` while I believed I was deploying GS-1. Caught by reading the head line in the log instead of the build finishing. Fixed by fetching, `git merge` (the two sides touched disjoint files), and a fetch-retry loop in the deploy script so a DNS blink cannot gate a release behind a stale HEAD.


`census: WIRE` — the hard part was already in the tree and had no door.

| already here | where |
|---|---|
| `accounts.is_guest` (spec §14.1, D-6) read by trust, the experiment snapshots and `/admin/accounts/{id}/facts` | `migrations/*/0116` |
| the ladder's guest rung | `db::governance` (`guest_level`) |
| `consent.required_for_guest`, already **published** by `/meta` | `app::config`, `routes::meta` |

What did not exist: anything that *creates* a guest, and any consent record at all. So the row is wiring, and the wiring is
the part that has to be right.

**The gate comes first.** `POST /auth/guest` refuses with `422` naming `consent` — and the version it expects — **before** it
creates an account or issues a session. Silence is not consent, and the refusal test asserts the guest count is still zero,
not just the status code.

**A guest is an account, not an island.** The row is written by `create_guest` with `is_guest = 1`, a synthetic
`guest+<uuid>@guest.invalid` address (`accounts.email` is `NOT NULL` under a unique index on `lower(email)`, so a shared empty
string would have allowed exactly one guest in the whole instance), a pseud — the thing a bookmark belongs to, and what GS-3
upgrades in place — and a trust level from `recompute_trust`, so the ladder's guest rung is a level the pools can count.

**The record is a row.** `consents(account_id, version, categories, accepted_at)`, one per account: a re-ask is an
`UPDATE`, and "has this reader accepted the current terms" is one comparison rather than a `MAX` over a history nobody
queries. `accepted_consent_version` returns the number; `None` means "asked and did not answer".

**`is_guest` is on every account view** (register, login, `/auth/me`), not only on the guest route: a client that cannot tell
cannot offer "keep what you did" at the moment the guest registers. And a registered reader asking for a guest gets `422`
naming `session` — upgrading is one direction.

`crates/app/tests/m66_gs1_guest_consent.rs` — **5/5 SQLite and 5/5 PostgreSQL**: silence creates nothing (status *and* row
count); a guest reads with a session, `is_guest` true on `/auth/me`, one consent row, one pseud; asking twice returns
`created: false` and leaves one guest; `required_for_guest = false` skips the interstitial and records **no** row (the record
means "they agreed", not "they passed through"); a registered reader is refused and stays `is_guest: false`.

Mutants: the gate disabled → `left: 201 / right: 422`; `record_consent` skipped → `left: 0 / right: 1` rows; the
already-registered guard disabled → `left: 200 / right: 422`.

**Two PostgreSQL-only decode faults, both found by running both engines.** `consents.version` is `INT4` and decoding it into
`i64` needs `version::bigint` — and it only fails **when a row exists**, so the empty-table probe answered `Ok(None)` while
the second guest request answered 500. (The first fix attempt went the other way: casting `resonance::bigint` broke a column
that is `DOUBLE PRECISION` and decoded as `f64`. Both engines, every time.)

## M45-59 built and proved — a link is per reader, not per bot — 2026-11-11

**Deployed 12:48** — `0.1.0+da8fa4f2`, no migration (the linkage was already in `api_tokens`). Live reads: `/api/v1/meta` 200, `/statements/summary` 200, `/notices/forms` 200, `/bots/link/nosuch` 404, and `bot_registrations=0`, `api_tokens(kind=bot)=0` — so the single-reader bug was fixed before the first real link existed, rather than after somebody was logged out by a friend joining the same server.


Found while starting M45-43, and it stops that row dead if left alone.

**§23.2's confirmation rotated the registration's credential.** `confirm_link_challenge` issued a token, then called
`rebind_bot_token` to point `bot_registrations.token_id` at it and revoked the credential it replaced. Two consequences, both
silent:

1. **The bot could only ever have one reader.** The second person to link revoked the first person's token. In a guild, one
   member linking logs another member out.
2. **`bot_for_token` could not see links at all.** It matched `bot_registrations.token_id` only, so once a link had been
   rotated the suspension check stopped resolving the linked credential — an operator suspending a bot would leave every
   linked reader's token working.

Neither is a property of the calls; it is **cardinality**, which is why "darkbot adapts, not the server" (the census
finding) was true about the *shape* of the flow and wrong about this.

**The fix, two places.** The label was already on the row: `issue_token_acting` stores `kind = 'bot'`, `name = <bot id>` and
`acting_pseud_id`. So:

* `confirm_link_challenge` no longer touches the registration. It revokes **that (bot, pseud)'s** previous credential —
  `bot_link_token_ids(db, bot_id, pseud_id)` — and nothing else. Re-linking replaces your own token; it is no longer a
  revocation of the bot's.
* `bot_for_token` gains a second arm: a token resolves to its registration through `api_tokens` when it carries
  `kind = 'bot'` and `name = <bot id>`. Suspension therefore reaches every linked reader, because the state is read from the
  registration on each use rather than baked into the credential.

**No migration**: the linkage is already in `api_tokens`, which is where the token model lives.

`crates/app/tests/m45_59_multi_link.rs` — **3/3 SQLite and 3/3 PostgreSQL**, plus `m54_bot_link` **25/25** and
`m70_13_bot_link_doors` **2/2** on both engines:

* `two_readers_link_to_one_bot_and_both_credentials_work` — two people link; both credentials work; each acts as its own
  pseud; and `bot_registrations.token_id` is identical before and after (the regression the row exists for).
* `a_relink_replaces_one_person_and_leaves_the_other_readers_alone` — the newest credential works, that person's previous one
  answers 401, **the other reader's still answers 200**. The boundary is (bot, pseud), not (bot).
* `suspending_the_bot_stops_every_linked_reader` — one operator action 401s both a linked credential and the registration's.

Mutants: the link arm of `bot_for_token` removed → suspension test `left: 200 / right: 401`; the per-pseud revoke disabled →
re-link test `left: 200 / right: 401`. Both reverted from `/tmp`.

**PostgreSQL type note**: `api_tokens.id` is `UUID`, so the EXISTS arm binds `$1::uuid` and the link lookup selects
`id::text` — the SQLite-only run passed while all three tests 500'd on PG, which is why both engines run before a row is
called done.

**What this does not do**: darkbot still calls `bot/link/start` and `bot/link/exchange`, which this server does not have.
Under §23.2 the reader confirms on the site and the token is issued to *their* session, so darkbot's `/link` cannot hand the
bot a per-user credential — the bot's personalized commands have to become "open the site" links, and its public commands
(search, work, kudos) keep working with the registration credential. That is a UX decision for M45-43, which is otherwise
blocked on a Discord application for darkhold (the host holds only ascend's).

## M45-55 built and proved — 2026-11-11

`census: BUILD` — the vocabulary (`dsar`, `subject access`, notice templates) appears in this repo and in
`darkhold-roadmap`, in **no sibling checkout**; and inside here the assembler already existed: `GET /me/data`
(M45-51) builds a reader's own copy from `db::erasure::subject_data`. So the row is composition, not construction —
the operator's half, the forms, and the templates — and it reuses that assembler rather than writing a second one.

Three doors in `crates/app/src/routes/compliance.rs` (module `compliance_tooling`, `default_on`):

* **`GET /notices/forms`** — public, because a complainant needs the form before they have an account, and a form behind a
  sign-in is a form most complainants never fill in. It serves the two regimes as **data**: the DSA's Article 16 notice,
  and the DMCA's four statutory elements. A frontend rendering from its own copy is a second declaration of the same
  contract, and it drifts the first time one side adds a field — with the failure landing on a complainant whose notice is
  refused for a field the UI never asked for.
* **`GET /statements/templates`** — operator-only (`ResolveReports`). One template per ground, walked from
  `StatementGround::ALL`, so **a new ground cannot ship without prose**: the unit test fails if any ground has no template
  or a template has no `{facts}`, and `placeholders` reports `legal_ground` exactly for the two grounds that are claims
  about law.
* **`GET /admin/subjects/{account}/data`** — the operator's read, which `routes/erasure.rs`'s module doc had explicitly
  deferred here. It **requires `?basis=`** from a closed set (`legal_obligation`, `legal_claim`, `vital_interests`,
  `consent`), refuses `422` naming `basis` otherwise, and **writes an audit entry on every successful read** — a read that
  cannot be recorded refuses the disclosure rather than answering it silently. The response is a **document**: the same
  body shape as the reader's own copy, sent with `Content-Disposition: attachment; filename="dsar-<account>-<date>.json"`,
  because a subject-access answer is handed over, archived and produced again if asked.

`crates/app/tests/m45_55_compliance_tooling.rs` — **3/3 SQLite and 3/3 PostgreSQL**: the forms are public and name every
field the validator refuses without; the templates are `401` to an anonymous caller and cover all four grounds with the
right placeholders; and the operator read refuses without a basis (and with an invented one) **without leaving a receipt**,
discloses with one, arrives as a named file, and a plain reader is refused before any of that.

Mutants, one assertion each: both basis checks disabled → `left: 200 / right: 422`; the audit append removed →
`left: 0 / right: 1` receipts. Both reverted from `/tmp`.

`route_inventory` 5/5 — the public form door takes `State` + `MaybeSession`, which is the shape the inventory checks.

## M45-52 steps 1–2 built and proved — 2026-11-11

The spine of the row, and the half that makes the rest possible: **a decision now carries its reasons, or it is not a
decision.**

* **Migration `0208_statement_of_reasons`** (both dialects): `task_id`, `report_id`, `outcome`, `ground`, `facts`,
  `legal_ground`, `decided_by`, `decided_at`, `appeal_until`. `ground` is a closed set in
  `darkhold_domain::governance::StatementGround` — `illegal_content`, `incompatible_with_terms`, `spam`, `copyright` —
  because Article 17 asks for the *category*, and a paragraph that contains the word "spam" is not one.
  `requires_legal_ground()` marks the two grounds that are claims about law, so the instrument cannot be omitted.
* **`APPEAL_WINDOW_DAYS = 30` lives beside the enum**, and the deadline is **stored** rather than derived: a window a
  person was told about must not move when a constant does.
* **`decide_task` refuses without reasons.** `DecideTaskBody` gained three fields that are `Option`-shaped at the type
  level so the refusal is `422` **naming the field** (`ground`, then `facts`, then `legal_ground`) instead of serde's
  anonymous "missing field". The reasons are validated *before* the decision is recorded, because decide-now-explain-
  later is how a queue fills with decisions nobody can defend.
* **Both the audit entry and the response carry the statement.** The response is
  `{ok, statement_id, ground, appeal_until}`; the audit document records the ground and the statement id, so the log
  and the explanation are the same fact.

`crates/app/tests/m45_52_statement_of_reasons.rs` — **3/3 on SQLite and 3/3 on PostgreSQL**:

| test | what it pins |
|---|---|
| a decision without a ground is refused naming the field | `422` with `error.field_errors.ground`, and **no statement row left behind** |
| the statement written with the decision names the reviewer and the deadline | reviewer's own words verbatim, `decided_by` = the deciding account, one deadline reported and stored, and `appeal_until > decided_at` |
| a lawful ground must name its instrument | `copyright` with no statute → `422` naming `legal_ground` |

Three mutants, each reddening exactly one assertion: `ground` falling back to `spam` (`left: 200 / right: 422`),
the lawful-ground check disabled (`left: 200 / right: 422`), and the window set to zero days (the deadline assertion).
All reverted from `/tmp`, never `git checkout`.

**The gate this row deliberately reddened:** `trust_demotion`'s helper called `decide` with `{"outcome":"uphold"}` only.
It now sends ground and facts — that suite is 5/5 again, and the failure was the row working, not a regression.

**Step 4 also built (same turn) — the window is enforced.** `POST /appeals` loads the sanction, refuses `404` when it
belongs to somebody else (a stranger learns nothing, not even that it exists), and when `reason_ref` is
`statement:<id>` it parses the stored `appeal_until` and refuses **`422` naming `appeal_until` with the deadline in the
message** once it has passed. Two tests: inside the window → accepted; after it → refused naming `2020-01-01T00:00:00Z`.
Mutant: the comparison disabled → `left: 200 / right: 422`. 5/5 SQLite and 5/5 PostgreSQL for the suite.

**Step 5 also built (same turn) — the transparency summary is a count.** `GET /statements/summary` is public (Article 24's
obligation is owed to the public) and returns counts per ground and outcome with **`TRANSPARENCY_MIN_COUNT = 5`**: a cell
below the threshold is `{count: null, suppressed: true, reason: "below the transparency threshold of 5"}`, because a count
of two is two people. No statement id, task id or account appears in the body — the test asserts their absence, not just
their omission. Mutant: suppression disabled → `left: Number(2) / right: Null`. Suite 6/6 SQLite and 6/6 PostgreSQL.
`route_inventory` 5/5 (the public door takes `MaybeSession`, which is how this tree records that a door is offered to an
anonymous caller).

**Step 3 also built (same turn) — both sides are told.** The decision notifies the **reporter** (`report_decided`, carrying
the outcome, the ground and the statement id) and the **author** (`moderation_decision`, carrying their own `facts`
verbatim, the ground and the deadline they may appeal by). The author is resolved from the report's subject — an account
directly, a work through `work_owner_account` (works → pseuds → account) — and the existing `notifications::notify` carries
both, so an account that muted this event kind is honoured without a second inbox. Test: the author signs in and finds the
notification with the reasons, the ground and the exact deadline; mutant filtering the author out makes the inbox empty
(`items: []`). Suite 7/7 SQLite and 7/7 PostgreSQL.

**Step 6 built (same turn) — DMCA is its own form, not a flag on the DSA one.** `POST /reports` gained `kind: dsa|dmca`
(default `dsa`, so every existing caller is unchanged). The DMCA arm requires 17 U.S.C. 512(c)(3)'s four elements —
`copyrighted_work`, `infringing_material`, `good_faith_statement`, `signature` — each refused **`422` naming its own
field**, and stores them in `dmca_notices` (migration `0209`) beside the report. A DSA notice needs none of them, and the
test asserts that too: folding the two together makes one of the two non-compliant. Mutant: the element check disabled →
`left: 200 / right: 422`. Suite **8/8 SQLite and 8/8 PostgreSQL**.

**All six steps are built.** The row moves to `implemented-fully-tested`: every acceptance clause from the plan is a test
in `crates/app/tests/m45_52_statement_of_reasons.rs`, both engines, with a mutant per refusal.

**Superseded note — the plan above is now the record of how it was built, not a remainder.**

 (3) tell both sides through the existing notification machinery, (4) enforce
`appeal_until` at `POST /appeals`, (5) `GET /statements/summary` as counts per ground and outcome, (6) the `dsa|dmca`
discriminator with DMCA's four statutory elements. Steps 1–2 are the part the other four read from, which is why they
came first.

## M71-13 measured — 2026-11-11 (two of three clauses built and tested)

| clause | state | evidence |
|---|---|---|
| a page that enumerates a dimension states its share | **built + tested** | `db::taxonomy::dimension_coverage` (one statement, `Coverage{dimension,value,works,catalogue,share,sentence}`), emitted by `/tags/{tag}` (`browse.rs:488`) and the fandom page (`browse.rs:665`); `crates/app/tests/m71_13_coverage.rs` 2/2 covers the API sentence, the document page printing the same sentence, drafts/private/deleted excluded from both numbers, and the zero case |
| the count names the archive it is a part of | **built + tested** | `tag_page.rs` asserts `2 of N works in this archive are tagged …` on the HTML |
| fandom and source pages **split hosted from referenced** | **not built** | `Coverage` has no hosted/referenced pair; nothing in the schema row shape tells a hosted work from one this instance only *references*, so the split needs the origin concept first |

So this row is not "add coverage to pages" — it is one clause away, and that clause is blocked on a distinction the
schema does not yet draw. Widening `Coverage` with two numbers before an origin concept exists would produce a split
that is really `lifecycle=published` twice under two names.

# M45-45 step 3, as built 2026-11-11

`crates/db/src/taste_lens_proposals.rs` + migrations `0206_taste_lens_proposals` and `0207_taste_lens_cause`:

* a proposal carries the **whole lens**, so a vote is for a feed a reader can picture;
* the **threshold is stored per proposal**, so raising the default later cannot retroactively pass an old vote;
* **one vote per account**, enforced by the primary key — a reader who changes their mind replaces their vote rather
  than counting twice (proved: yes-then-no leaves `(0, 1)`);
* **no votes is not a rejection**: the proposal stays `open` with no `decided_at`, so an operator sees that nobody
  engaged instead of reading a decision the instance never took;
* a passing proposal writes through `write_as` with `cause = proposal:<id>`.

The measurement that changed the design: `instance_taste_settings_history.changed_by` is a **UUID column** on PostgreSQL,
so `proposal:<id>` is not storable there, and the proposal's UUID in that column would be indistinguishable from a person.
0207 adds `cause TEXT NOT NULL DEFAULT 'operator'`; `updated_by` keeps naming the operator who opened the proposal.

Tests `crates/db/tests/m45_45_taste_lens_proposal.rs` 4/4 on **both engines**; mutants (threshold ignored, cause fixed to
a constant) each redden their own assertion, restored from `/tmp`. Remaining: the HTTP doors — `POST /taste-proposals`
and `POST /taste-proposals/{id}/vote` — without which an operator under `vote` has a policy and no way to change the lens.

# M45-45 step 3, measured before it is written — 2026-11-11

The `vote` path needs a proposal the instance can vote on, and the tree already has one machinery of that shape:
`crates/db/src/retention_proposals.rs` — `open_proposal`, `create_proposal`, `cast_vote`, a threshold, and a decided
state. The question worth answering before copying it is whether it can be *reused*:

- its identity is `source_key` (`open_proposal(db, source_key: Option<&str>)`), i.e. a proposal is about **one import
  source**'s retention window;
- a taste-lens change is about a **singleton** row (`instance_taste_settings` has `CHECK (id = 1)`), and the thing being
  voted on is a set of knobs, not a per-source window.

So the honest modelling is a sibling table with the same shape, not a `kind` column bolted onto `retention_proposals`:
`taste_lens_proposals(id, proposed_gravity_strength, proposed_signal_weight_mode, proposed_admin_weight,
proposed_diversity_injection_percent, opened_by, opened_at, closes_at, threshold, state, decided_at)` plus
`taste_lens_votes(proposal_id, voter_account, vote, cast_at)`. Copy the *shape* — vote counting, threshold, decided_at —
rather than the table, because two proposals about different subjects sharing one table means every query carries a
`WHERE kind = …` and one of them will eventually forget it.

`changed_by` on the applied version then records `proposal:<id>`, which is the whole point of the step: the history row
would say *why* the version changed, not only who pressed the button.

### M45-45 step 4 does not exist as written — corrected 2026-11-11

The plan above lists "succession: if the lens owner is erased or deactivated, the lens reverts to `operator` and the
history row says so". Checked against the code, there is no *lens owner account* to erase: `lens_owner` is a **mode**
(`operator` | `vote`), not a person, and the person who pressed the button is already recorded per version as
`updated_by` / `changed_by`. A rule that reverts a mode when an account disappears would be reverting the instance's
governance because one operator left, which is the opposite of what succession should do.

The succession the note asks about is the instance's, and it is **already built**: `succession_mirror` (M45-39) names the
peers who inherit an instance when the operator cannot act, and `PUT/DELETE /admin/succession/mirrors` maintains it. The
taste lens is a row in that instance, so it inherits with it — no second succession mechanism, and no erasure-path rule.

So M45-45 has **one** remaining step, not two: the vote path (step 3). The ledger note is corrected to say so; the
earlier "two halves remaining" was my reading of the note rather than of the code.

### M45-45 doors — fixed and proved through HTTP, 2026-11-11

The open door was mounted at `/operator/taste-proposals`. `/operator/*` is a **nest**, and a nest owns its whole subtree:
a route merged beside it at the outer level can never match, and the request falls to the fallback (`404 page not found`).
That is why POST `/taste-proposals/{id}/vote` answered `401` (not under `/operator`) while POST `/taste-proposals`
answered `404`. One cause, two symptoms, and the missing door was the one that had a prefix.

All three doors now live at `/taste-proposals`, `/{id}` and `/{id}/vote`; the inventory rows moved with them
(`route_inventory` 5/5).

`crates/app/tests/m45_45_taste_proposals.rs` **4/4 on SQLite and 4/4 on PostgreSQL**:

* a signed-in reader opening a proposal gets **404, not 403** — `require_estate_hidden` conceals the operator surface
  (ADR 0003), and asserting 403 would demand that the instance disclose the door's existence;
* an anonymous vote is `401`;
* a second vote from one account replaces the first through the door (`(0, 1)`, not `(1, 1)`);
* a due proposal is applied by the read, and the version's history says `cause = proposal:<id>`.

Mutant: dropping `require_operator` turns the reader's `404` into `200` (red), restored from `/tmp`.

**Lesson kept**: a `404` on a route you believe you mounted is a statement about the *mount level*, not the handler. Probe
the same path with two methods first — `405` means registered-but-wrong-method, `404` means not registered where you think
it is — before editing anything.
## M45-45 as measured — 2026-11-10 (`planned`, two thirds built, one half designed)

The row is "taste as versioned blendable object with governance mode", and three of its four words are already true in the
tree. Measured, not assumed:

| part | state | evidence |
|---|---|---|
| **versioned** | built | `instance_taste_settings` (0077) carries `version` + `updated_by`, `instance_taste_settings_history` (0078) stores the row *before* each write with `changed_by`/`changed_at`/`rolled_back_from`, `GET` history route at `routes/discovery.rs:1772` |
| **object** | built | `domain::taste_vector::{TasteProfile, TasteVector}` with `euclidean_distance`, `normalized_distance`, `compute_user_vector`, `update_vector_incremental` |
| **blendable** | built as a primitive | `blend_vectors(a, b, t)` in `crates/domain/src/taste_vector.rs:181` — a weighted merge of two vectors, but nothing composes *instance* taste with anything else |
| **governance mode** | **not built** | no `council` anywhere in `crates/`, no lens-owner concept, no succession contract for the operator who owns the lens |

So the row is not "build a versioned taste object": it is **who may change the lens, and what happens when they cannot**. The
plan, in the order the pieces can be checked:

1. **`[taste] lens_owner = operator | council | vote`** in `ContentPolicyConfig`'s neighbour — a new `TasteGovernanceConfig`,
   default `operator` (today's behaviour, so the knob changes nothing until an operator sets it).
2. **The write door asks the owner, not the role.** `PUT /admin/taste/settings` currently gates on `Audience::Operator`.
   Under `council` it must accept a *council member*; under `vote` it must accept nothing and require a proposal. The refusal
   names which mode is in force (`422` naming `lens_owner`), the way the RPF knob does.
3. **`vote` reuses the existing proposal machinery** rather than inventing a second vote: a taste change becomes a proposal
   the governance module already knows how to open, count and close. The knob write happens on the proposal's outcome, and
   `changed_by` records the proposal id so history shows *why* the version changed, not only who pressed the button.
4. **Succession**: `succession_mirror` (M45-39) names the peers who inherit an *instance*; this row needs the smaller
   contract — if the lens owner's account is erased or deactivated, the lens reverts to `operator` and the history row says
   so. That is a rule in the erasure path, not a new table.

**Acceptance tests, per step:** (2) `refuses_a_lens_write_when_the_owner_is_a_vote_and_names_the_mode` — 422 naming
`lens_owner`, plus `a_council_member_may_write_the_lens_under_council`; (3)
`a_vote_backed_lens_change_records_the_proposal_id_in_history`; (4)
`erasing_the_lens_owner_reverts_the_lens_to_operator_and_says_so_in_the_history_row`.

Steps 1 and 2 are one sitting; 3 depends on the governance module's proposal API and should be read before it is written;
4 is a small erasure-path rule. The row stays `planned` until all four are green.

## Deployed 2026-11-11 01:07 — M45-65 and M45-54 (`0.1.0+d4b718f6`)

Build-only for the last pass (no DDL, so no second dump; the named backup for the batch is
`darkhold-20261111-001448-pre-m45-65.dump`, 3,740,163,926 bytes, 399 `TABLE DATA`). Build 14m56s, `strings` count for
`policy/content` = 2, service active, `/api/v1/meta` → `0.1.0+d4b718f6`.

`GET /api/v1/policy/content` live:

```
{"honor_rights":{"basis":"honour, personal and family privacy, and one's own image","jurisdictions":["ES"],
 "laws":[{"instrument":"Ley Orgánica 1/1982","jurisdiction":"ES",...}], ...}, "rpf":{"mode":"tag_required",...}}
```

A build for `b89eff14` was killed by PID mid-compile once `d4b718f6` landed, rather than letting it finish and paying for
a second 16-minute build. Cargo replaces the binary atomically, so the live artifact stayed at `0.1.0+09ec61b1` until the
replacement linked — verified before and after the kill.

## Deployed 2026-11-10 23:55 — M45-46 preview + the nav fix (`0.1.0+d2942fc3`)

The batch that was in flight when the previous ssh was killed.

| step | measured |
|---|---|
| dump | `/personal/backups/darkhold-20261110-2312xx-pre-nav-fix.dump`, 3,740,163,393 bytes, 399 `TABLE DATA` (the dump alone takes ~18 min on this database, so a "small" deploy is ~45 min) |
| frontend | `vite build` → `dist/assets/index-DQl-Jj6t.js`; the bundle contains `floating` (the panel's fixed class), which is how the built artifact is proved rather than the source |
| backend | `cargo build --release -p darkhold-app --bin darkhold`, binary 23:55 |
| live | `/api/v1/meta` → `0.1.0+d2942fc3`; `POST /admin/discovery/preview` unauthenticated → **401** (the route exists and is operator-only); `/discover` serves the new bundle |
| CSS | `--surface-2` now resolves (`oklch(18% var(--brand-chroma-surface) var(--brand-hue))` after-hours, `#ffffff` clear-day) — it was the empty string before, which is what made every raised surface transparent |
| browser, live | on `https://darkhold.polarisocial.xyz/discover` the Read panel is `position: fixed`, `rgb(36, 49, 42)`, bottom 358px against a row bottom of 60px, and **0 of 6 links unhittable** |

## M45-65 — done 2026-11-10: both identity gates read the schema, not the migration text

The row is BOTH gates seeing `account`, `created_by` and `pseud_id`. The snapshot half widened `IDENTITY_NAME_RE`,
classified 87 columns and left the builder emitting 119 `snapshot_account` projections. The erasure half needed the
scanner rebuilt, and the two failed attempts are the useful part of this record.

**Attempt 1 — widening the text matcher.** Named 52 columns, five of which do not exist: `0013_community.sql` does
`DROP TABLE IF EXISTS blocks; DROP TABLE IF EXISTS mutes;` and recreates both, and the scanner unioned every
`CREATE TABLE` block it found. A treatment for one of those made `erase_account` answer `no such column:
source_pseud_id`.

**Attempt 2 — an ordered text walk** (`CREATE TABLE` replaces, `DROP TABLE` removes, `ALTER … ADD/DROP COLUMN/RENAME`
applies). The phantoms went and the set fell to 28 — but 13 *existing* treatments became "stale", because the walk could
now see their `REFERENCES accounts` while two behaviour tests still depend on those treatments. That is the lesson:
`measured` is a claim about the SCHEMA and `classified` is a claim about the ERASURE CODE, and comparing them directly
conflates "the database already handles this" with "nobody thought about this".

**What shipped.** The scan reads the database: `sqlite_master`, `pragma_table_info`, `pragma_foreign_key_list`; it
computes which tables reach `accounts` by following foreign keys and measures only the columns outside that reach — 23
of them today. The `stale` assertion now asks the schema whether a column exists (a rename or typo), which is a different
question from whether the schema already links it. 23 treatments: 9 `Rekey("id")` on NOT NULL attribution, 2 `Sever` on
nullable attribution, 12 `Delete` for the person's own act.

**Proof.** `cargo test -p darkhold-db --test erasure_covers_fk_free_accounts` 4/4 (the scan announces its skip on
PostgreSQL in one line rather than passing silently); mutant removing `search_history.pseud_id` names it and reddens;
`check-snapshot-pii.py` OK 2878/395. Ledger 53 planned.

## M45-65 as measured — 2026-11-10 (`planned`, inventory done)

The row says both identity gates must see `account`, `created_by`, `pseud_id`. Widening `IDENTITY_NAME_RE` in
`scripts/check-snapshot-pii.py` and running it produces **87 undecided columns**, not the 37 the requirement names:
29 bare `account`, 13 `created_by`, and 45 `pseud_id` (the requirement's 37 came from the foreign-key analysis, which
never looked at pen names). The full list is `docs/plans/m45-65-inventory.md`, grouped by the spelling and by what the
column is for.

The widening is **reverted** in this commit. The gate's contract is that it fails in both directions until every column
carries a treatment, and each treatment is a judgement read off the writer — `RequirePseud` or `RequireSession` in the
handler that inserts the row. A green gate over 87 guessed treatments is the failure mode this row exists to prevent, so
the row stays `planned` with the inventory attached and the detector untouched.

**Fixed on the way through, and this one was live:** the gate was already **red** at `d2942fc3`, on four tables this
session shipped — `work_claim.account_id`, `work_claim.token`, `curator_lens.account_id`, `share_code.account_id` had no
policy rows, and `POST /works/{id}/share` stores a live token. Four decisions added; `check-snapshot-pii.py` now reports
`OK: 2878 columns across 395 tables`.

**Two zeros in the deploy log that are the probes' fault, not the build's — recorded because they read as failures.**
`surface2-in-css=0` searched for the literal `--surface-2:var(--color-surface-raised)`, i.e. the *cycle the fix removed*:
0 is the pass condition, and the label was wrong. `bundle-has-floating=0` fetched `/`, which this server renders
**without any script tag** (the SPA bundle is only referenced on app routes), so the extracted asset name was empty and
`curl` returned the landing page. Re-measured properly afterwards: `grep -c floating frontend/dist/assets/index-DQl-Jj6t.js`
on the host → **1**, and `curl /discover` lists `/assets/index-DQl-Jj6t.js`. The lesson: a built-artifact check has to
fetch the asset the **route** serves, not the asset `/` happens to mention — and a counter whose expected value is 0 must
say so in its own label, or the log argues against the build.

**Two deploy lessons, both measured:** a SIGTERM'd `ssh` does **not** stop the remote script — this session's killed deploy kept compiling and linked the binary eight minutes later, so a retry launched on the same tree runs `cargo` against a build that is still going. Check `pgrep -af` for the *remote* script before re-running, and never assume a killed ssh means a stopped build. And a script whose output only goes to the ssh session leaves nothing to read when the session dies: write to a log file on the host.

## Batch 0 — the family of products (M72, 10 rows) — take this first

Added 2026-11-08 at the user's direction, ahead of everything else, because it changes the *shape* of the work that
follows: the estate now runs four products on one host from one maker, and three of them have a frontend that shares
nothing with the other two. The survey, the decisions and the rejected alternatives are in
`docs/plans/estate-coherence.md`; the rows are `M72-01` … `M72-10` in the ledger.

The order inside the batch is a dependency chain, not a preference:

1. ~~**M72-01** publish `@polaris/ui` to the estate registry~~ **DONE** — 0.1.2 and inbox-client 0.1.1 published; a clone
   with no `estate/` sibling installs and builds. — today it is a `file:` tarball path that only resolves on
   this machine, so nothing else is adoptable until there is a version to depend on.
2. ~~**M72-02** one token vocabulary, palettes as data and a contrast gate over every palette~~ **DONE** — 0.2.0: six
   palettes, a written light ramp, per-palette literal CSS, 0 contrast failures; darkhold's three presets now come from
   it (browser-measured identical), and the gate caught a 1.8:1 control boundary in darkhold's own palette.
3. ~~**M72-03** the six primitives the estate has nowhere~~ **DONE** — @polaris/ui 0.3.1: checkbox/radio, avatar,
   breadcrumb, tooltip, kbd, action row; 11 render tests, six mutants red, adopted in WorkCard/Account/Docs/VoteBudget.
4. ~~**M72-04** one error envelope~~ **DONE on all four** — `{error:{code,message,request_id}}`, `code` UPPER_SNAKE,
   product extras allowed. Two classes escaped in this tree (an unknown `/api/` path and framework refusals — a 422 with an
   empty body was measured, then fixed); ficnexus keeps its legacy `err`/`msg` beside the envelope behind an env switch;
   ascend's frozen `CONTRACTS.md` was amended in the same commit as its code; kindred's middleware now puts the id in the body
   (its first test only exercised the helper and two mutants survived — the wiring is now tested through the router).
5. ~~**M72-05** one meta/health shape~~ **DONE and live** where it runs: `/api/v1/meta` (six named fields) and `/healthz`
   (`{status, version}`) on darkhold, ascend and kindred; `estate/scripts/meta-probe.mjs` exits 0 across the three, and the
   mutant the row named — `api_version` renamed in ascend — fails it by name. Two survey errors came out of probing: `:8050`
   is shortwave, not ficnexus (which has no process here), and ascend deploys from a non-git tree on the host.
6. **M72-06** adoption, darkhold → kindred → ascend, each proved by deleting the local copy.
7. **M72-07** the family nav module fed by `estate/generated/nav.json` — **done for darkhold** (`FamilyNav` in the site
   footer, three tests, three mutants, live-verified at https://darkhold.polarisocial.xyz/works); the sibling adoptions are
   one line each and belong with item 6 since both need `@polaris/ui` in those trees first.
8. **M72-08/10** the auth matrix and the noun/URL vocabulary — decisions plus their small code changes.
9. **M72-09** the design review as a gate — **done** (`gate.sh design`, 22 renders across three products, exit 0). It earned its place twice over: it is what proved ascend's `/fics` adoption renders, and it is what produced 14 contrast 'failures' on kindred that turned out to be the gate mis-measuring screen-reader labels.

**Batch 0 is complete: M72 10 of 10.** What the batch leaves behind is a family that shares a token vocabulary, one error envelope, one meta/health contract, one blurb order, one nav module, a written auth matrix, one noun vocabulary, and a gate that fails when any of it drifts.

What this batch is not: one skin. kindred reproduces AO3's visual grammar on purpose (`web/src/styles/kindred.css`
cites the OTW stylesheets) and ficnexus has no frontend at all (`docs/EXPORTER-SCOPE.md`). The family resemblance is
the grammar — component anatomy, spacing, focus treatment, motion budget, state language, URL shape — with each
product's own paper. A row that would restyle kindred into the Archive palette is out of scope and should be refused
in the ledger rather than half-done.

### A query nobody runs cannot fail to decode (exports.rs, 2026-11-08)

Closing `exports.rs` took three functions and turned up four defects, none of them in the code being added:

* `grants_for_export` selected `single_use` as INT4 into an `i64` field — PostgreSQL only.
* `bulk_items_for_export` had **no** PostgreSQL casts at all: three UUID columns and a `byte_size` INT4.
* `::text` on a `timestamptz` answers `2026-02-01 00:00:00+00` where SQLite's canonical TEXT answers
  `2026-02-01T00:00:00Z`. Same instant, different string: every RFC 3339 client parsed one engine and
  broke on the other. The grant columns are now rendered with `to_char(... AT TIME ZONE 'UTC', …)` so a
  read out of PostgreSQL matches the format the table stores.
* The fourth was not a bug but a duplicate: `owned()` in the route layer fetched an export and compared
  account ids in Rust while `find_export_for` did the same thing in SQL. It had never been called.
  `owned()` now *is* `find_export_for`, so the ownership rule exists once, in the place that cannot
  forget it.

The pattern holds across this whole ratchet: the orphan is never the bug. The orphan is the reason the
bug survived — a SELECT list, a cast, or a timestamp format is only exercised when something asks for
the row.

## Batch 1 — DONE

| row | what shipped |
|---|---|
| M71-10 | the reader's no-JavaScript guarantee as one test (`crates/app/tests/reader_page.rs::the_reader_needs_no_javascript_at_all`), plus the mutation that proves it bites. The position form it asserts already existed; the attempt to add one was a duplicate and was removed. |

## Batch 4 — ranking (M64 / M69)

M64-09 first half (71adfa72): gravity is a bounded mixture `(1-g)*base + g*taste` with a per-surface strength, a load-refused
ceiling and a published pull histogram; `discover.feed` and the concierge (`home.curated`, 1f05deff) have a candidate pipeline to pull on, and the reader's dial shipped in c9e4527d (`/api/v1/discovery/gravity`, narrowing only).

Shipped: M69-08 (blend decay, floor, score-first ordering, 0109d288), M69-19 (NPMI, da36de8e), M69-12 + M71-06 (reason +
match percentage, e93f94e6), M64-01 (surface registry, load-time validation, refusals by name, be0aead0).

M64-01 is now **fully tested** (0debecd9): `discover.feed` reads its configured engine set, an engine outside the set
does not run and costs no query, and the surfaces endpoint separates "no engine implements this" from "its table is
empty". Three callers still pass `None` with the reason written at the call site: the concierge queue (`home.curated`),
the draw (`draw.hand`, not yet a registry surface) and recipe runs (recipes compose, they do not read a surface set).

**M64-04 decision half shipped** (91a27078): `domain::experiment_promotion::decide` — mixture martingale, guardrails,
sample floor, seven-day floor, demotion on two consecutive breaches of the same guardrail. The record table shipped in a231f678
(`experiment_decisions` + `evaluate_surface`, promote/demote only, swap written on the row). The swap shipped in 111b7d00/5c9053df: promote installs `surface_algorithm_overrides`, demote clears it, and
discover.feed reads the override before the configured set. The sweep shipped in 5434fc26
(`POST /operator/experiments/sweep?day=`, keyed on (surface, day)). Guardrails shipped in 17e84537: `conversion_rate`
(challenger ≥ 50% of the incumbent) and `traffic_share` derived per day from the snapshots, so demotion fires for real.
M64-15 read half 7e58e239
(`GET /operator/experiments`), write half 0612cdf7 (hold/release, promote and demote by hand, `source` on the record).
`preview-as-persona` remains: it is a reader-shaped view, not a write.

Batch 4 also has M64-10's first half (7e746375): `GET /api/v1/facets` answers length, status, rating and cadence from one
scan, reporting `unknown` beside `known` and a sentence instead of an empty histogram. It is embedded in `/api/v1/search` (with `facets_over` naming the set), `/api/v1/tags/{tag}` and
`/api/v1/fandoms/{fandom}` as of b870d6e4, which closes the row.

**M64-03 shipped** (3d40065f): `experiment_snapshots` (surface, arm, day) with impressions / readers / guest_readers /
events, `snapshot_day` idempotent so a re-run repairs a day, `GET /api/v1/recommendation/experiments[?day=]`, and `null`
vs `[]` distinguishing "never computed" from "computed with no traffic". The nightly scheduler and M64-04's sequential
test are the next pieces; the latter computes from these counts rather than from the log, which is the row's whole point.

**M64-02 pseud half shipped** (cc6cab63): migration 0166 `experiment_assignments`, `bucket_for` insert-if-absent with
FNV-1a, `algorithms_for_bucket` splitting by position, `[discovery] experiment_buckets`. Still missing from that row and
written in its ledger: **anonymous session arms and the capped share** — a session is not a pseud, so it needs a
session-keyed assignment rather than a row in the same table. That is the next M64 piece, and it is also what M64-03's
per-arm metrics need before an experiment can be run on logged-out readers.

## Batch 2 — the visible contract

Shipped from this batch: M69-08 (blend decay/floor/ordering), M69-12 + M71-06 (reason and match percentage on
recommendation rows), M71-16 (sitemap), M71-04 (provenance), M71-05 (absent-not-empty), M71-10 (no-JS reader),
M71-02 and M45-58 (closed by triage).


Triage of all 96 rows, and the order it implies: `docs/plans/planned-triage.md`.

| row | approach | blocker |
|---|---|---|
| ~~M71-04~~ | **DONE 7b3e8f5f.** `routes/provenance.rs`: four words (`hosted`, `hosted_locked`, `referenced`, `known`), one grouped query per list. Wired into tag browse, search, library bookmarks, work page and tag page. Media and the author's own works list are excluded with the reason in the ledger row. |
| ~~M71-05~~ | **DONE 97d28354.** `test_support::{assert_no_empty_section, assert_names_degradation}`, applied to the three document surfaces; SPA surfaces take it as they migrate (M71-20). |
| M71-03 | dense list rows on browse, search, library, rails. Depends on M71-04's field existing (the row carries provenance, rating, length, status). | M71-04 |

## Batch 3 — audits that can only shrink the tree

| row | approach | blocker |
|---|---|---|
| M70-13 | a checked-in ratchet test: every `pub async fn` in `crates/db` must be called from a non-test source in another crate, or appear in `docs/plans/orphan-queries.txt` with a reason. Measured 2026-11-06: 291 db functions referenced only by tests or nothing. The ratchet fails on new ones and the file only shrinks. `content::list_public_works_by_freshness` — the row's own example — is now called by the landing page. | none |
| M63-33 | reconcile the live database with the migrations: 11 tables the migrations create are absent, 12 legacy tables remain. Needs the live DSN; the deploy is the operator's call. | deploy decision |
| M65-09 | decided **evaluated-and-rejected**: a process-wide "any overrides exist" flag is wrong for a second process sharing the database, and the read it removes is one index probe on a surface that is not hot. The row's own note says "not before a profile shows it". | — |

## Batch 4 — ranking engines (M69)

| row | approach |
|---|---|
| ~~M69-19~~ | **DONE da36de8e.** `taxonomy::related_tags_by_npmi` (log in Rust, one grouped count query, pairs seen once excluded), `GET /api/v1/tags/{tag}/related`, and a Related tags section on the tag page. No nightly matrix needed at this size. |
| ~~M69-08~~ | **DONE 0109d288.** `BlendOptions{decay 0.8, floor 0}` + `blend_with()`; ordering fixed to score-first. The floor is a parameter no surface sets yet — that is M64-01's per-surface config. |
| M69-09 | affinity as a stored 0..1 score per reader, recomputed nightly, replacing the per-work multiplier. |
| M69-10, M69-11 | Glicko-2 for the arena (deviation, volatility, divergence flag) and pairing that prefers similar rating with high deviation. The arena already stores per-match outcomes; this is a rating model swap plus a pairing policy. |
| ~~M69-12~~ | **DONE e93f94e6 on discovery.** `match_percent` + `reason` + `reason_code` per row; `SlotReason::sentence()` is the vocabulary. Draw, concierge and similar-to-shelf still emit bare ids — three call sites left. |
| M69-13, M69-14 | trust velocity and a live badge that is gained and lost, both derived from karma terms already stored. |
| M69-15 | probation above a configured population. Governance knob, default off. |
| M69-16 | three-layer content filtering (hardcoded, policy, taste) with reasoned overrides and an audit row. |
| M69-17, M69-18 | tag provenance (human/source/model/community, model may only propose) and trope clusters with a coverage checklist. Both are `work_tags` column additions plus write-path rules. |
| M69-20, M69-21, M69-22, M69-23 | the tag script, Saved Filters, the morning queue and import intake policy — one feature family: a Saved Filter is the object, and the other three are surfaces over it. Take M69-21 first. |
| M69-24 | one shared taste crate + JSON schema set with suki. A crate extraction; the estate rule says extract at the second caller, and suki is the second caller. |
| M69-26 | three-layer settings (platform default, operator override, reader personalisation) with bounds stated once. |
| M69-02..M69-07 | federation and preservation: peering by fingerprint similarity, signed taste passport, tombstones that survive re-import, mirrored works as visible attribution, hash-rescue ring, evacuation protocol. Each needs the shared taste crate (M69-24) and a signing key decision. |

## Batch 5 — the experiment platform (M64)

M64-01 (surface registry, 2–4 algorithms by config), M64-02 (append-only impressions), M64-03 (daily snapshots),
M64-04 (automatic promotion only on a sequential-test win with guardrails in bounds), M64-15 (console list with
hold/promote/preview) are one system and must be built in that order. The algorithms then plug in: M64-06
(tag_jaccard/tag_cooccurrence — exists, refused by name while sparse), M64-07 (crosspost_union), M64-08 (fandom_graph,
operator-managed adjacency), M64-09 (taste gravity, bounded, after the blend), M64-10 (length/status/rating/cadence as
first-class facets), M64-11 (co-shelving, reading lists first), M64-12 (co_read from session transitions), M64-13
(engagement_overlap with k-anonymity), M64-14 (embedding similarity, feature-gated on a configured AI provider).

## Batch 6 — the CLI (M63)

The `dh` surface is one binary with subcommands; these are independent and small relative to the engines:
M63-05b (`rec --link epub`, needs the download route), M63-10 (`lib sync --since`), M63-11 (`lib tag --from-csv`, rollback
file), M63-12 (`watch`), M63-15 (`doctor`), M63-18 (config profiles), M63-22 (`admin explain-rec`), M63-23
(`admin backup`/`health`), M63-24 (`bench`), M63-25 (`audit --since`), M63-26 (`stats`), M63-29 (`tui`), M63-30
(Calibre `metadata.db` as a source), plus catalogue rows M63-32 (fandom backfill), M63-37 (per-source engagement as
reported), M63-38 (per-operator quality weights).

## Batch 7 — extensions (M16)

~~M16-06~~ shipped: `crates/sandbox` (`darkhold-sandbox`, `wasmi` 2) with one caller,
`POST /api/v1/operator/sandbox/run`. Every ceiling in the row is enforced and has a module that hits it: fuel
(a loop with no imports, which nothing else can stop), memory (`trap_on_grow_failure`, so a denied grow reports
`memory` rather than the module's own out-of-bounds trap), host calls, output bytes (a refused write appends
nothing), wall clock, per-call host-I/O budget, and concurrency (`Concurrency` on `AppState`, refusing with 429
rather than queueing). Revocation is an `AtomicBool` polled at every host call and once before entry. The honest
limits are in the row's notes: `wasmi` has no epoch interrupt, so a module that never calls the host is bounded
by fuel alone. M16-07 the review workflow; M16-08 gallery mechanics; M16-11 the paid marketplace; M16-12 the
acceptance list (the tests that make 06/07 real); M16-13 the theme model beyond one stylesheet; M16-14
operational rotation, rate limits and auto-disable. M16-12 is now the natural next step: the sandbox exists, so
the acceptance list has something to accept or refuse.

M16-12 partly shipped (`crates/app/tests/m16_12_acceptance.rs`): five of seven claims are now tested — infinite
loops, memory excess, host-API permission denial, uninstall removing grants, and reset reachable after a broken
theme. Two things came out of writing them: host APIs are now **default-deny at link time** (`Grant`, so an import
the manifest did not get is `permission_denied` before an instruction runs), and re-granting after an uninstall
used to be a `UNIQUE` violation, because the grant's primary key is `(account, manifest_id)` and revocation is a
column. Still open, and named in the ledger rather than glossed: the recipe half of "custom engines and recipes honour
opt-out" (no extension-supplied recipe or engine exists, and the only AI-provider path — M64-14 embeddings — is
not built, so `privacy::custom_processing_allowed` is enforced at `request_narration` and is the one predicate a
future recipe dispatch must call). Forking has since shipped: migration 0181 adds
`extension_manifests.derived_from`, `POST /extensions/{slug}/fork` keeps attribution in that column
beside an untouched document, enters `pending`, and starts with **no grants** — consent named the original id.
M16-12 stays `planned` on the recipe clause alone.

## Batch 8 — community, governance and growth (M45, M66, M29, M44)

M45-58 (erasure reaches the routing record) is the only M45 row that is a correctness obligation rather than a feature;
take it first. Then M45-39 (instance-death protection), M45-45 (taste as a versioned blendable object), M45-46
(preview-before-apply), M45-52/54/55 (notice-and-action, RPF knob, compliance tooling), M45-40/41/42/43 (curator
views, share attribution, claim-your-works, Discord bot), ~~M45-44~~ (sister instances, shipped 2026-11-08 — see the
section below), M45-65 (identity gates see every spelling).

M66: GS-1..GS-5 are one feature (guest access with consent, the fold, the cookie pseudonym, trust from karma, juries)
and must be taken in order. M66-08 (LAN file sharing) is independent.

**M29-08 and M44-02 are the same product idea** (a community-ranked roadmap board). Implement once, under M44-02's Elo
board, and close M29-08 by pointing at it — a second implementation would be two sources of truth for the same list.

## Batch 9 — the remaining frontend (M71)

~~M71-06~~ (reasons shipped on the discovery feed, e93f94e6; rails take them as they migrate), M71-07 (console register), M71-08 (auto-hiding reader bar + typography panel), M71-09
(honest stale position), M71-11 (facet counts endpoint), M71-12 (ten-bucket histogram), M71-13 (coverage statements),
M71-14 (settings provenance), M71-15 (facet/query equivalence), M71-20  — M71-16 shipped as 4a7f8839 (sitemap + Audience::Anonymous)
(search, author, collection and series pages as documents — the last of ADR 0006's order, plus deleting the Svelte
routes of the pages already migrated).

## What is not in this file

Nothing is dropped. Four rows are `unsupported` for reasons already recorded in the ledger (M2-06, M6-10, M7-03,
M6-15), and six are `evaluated-and-rejected` with their reasons in the row.


M64-11 first half (ee413789): `co_shelving` is an engine over public shelves only (mutated predicate reddens the test);
following a list shipped in e62ab963 (0172_shelf_follows, refusals pinned by mutation); story.similar now reads its configured set and has a co-shelving arm (5dc85eae); library.because_you_shelved shipped its route (6bc3a869); story.also_enjoyed still has no route and no engine for co_read/engagement_overlap.

M69-21 first half (de8fb809): saved views take a per-account slug with PUT /saved-views/{id}/slug and GET
/saved-views/by-slug/{slug}; subscriptions, keybind and the blend half remain.

M69-16 layer two (cdeac9dc): content_policy_blocks with a required reason, revocation as a record, merged into
content_filter_sql::for_pseud so it reaches signed-out readers; per-work overrides remain.

M69-10 consumer half (48da17e2): volatility and divergence are carried from polaris-taste and gate ladder
eligibility through one is_settled() rule; the arithmetic stays in the service per ADR 0001.

M69-16 closed (cdeac9dc + b13fe04b): three layers — hardcoded SQL invariants, content_policy_blocks with a
required reason and recorded revocation, reader filters — plus per-work overrides that name the rule they
exempt and are rendered inside the single exclusion, so every surface inherits them.

M69-17 (ee606f1c): tags carry their claimant through the existing source/confirmation vocabulary — model=
inferred+unconfirmed is invisible to readers, a confirmation by a reader or wrangler reveals it, and an
inference cannot overwrite a person's row. Remaining: route every work_tags writer through assert_tag.

M69-20 (c6b253f9): tag scripts — a kept set of nodes applied to a work in one call, written through
tag_assertions so the tags carry a human claimant (author on your own work, curator on another's).
Batch apply and the browsing UI remain.

M69-18 (ed5f1f6e): trope clusters + coverage checklist. Advisory and read-only; the public route answers
404 until taxonomy.trope_coverage is switched on (decided OFF by default, noted in the ledger).

M64-06 (61f4da13): the refusal half shipped — a tag surface now refuses by name when tags and engagement
never meet, with the counts, instead of claiming to rank. Row stays planned on M63-32's backfill.

M63-32 (b387cb1e): tag-inference pass ships as proposals only (model claimant ⇒ invisible until
confirmed), with per-source coverage (items/tagged/with_likes/tagged_and_liked). The metadata backfill
still needs source category data the importer does not store.

M64-07 (f5471486): crosspost_union ranks a story once at the sum of its archives; feed wiring
proven by m64_07_crosspost_union (served once, first, 65 > 50).

2026-11-07 merge — thinkcentre f6bbb890 + 1b4797bf (W7 task 2 step 3: roles reach the authorisation
checks; the cross-site refusal and audit check mutation-proven) merged into master as 249a2bce.
Its migration came in as **0179_session_roles**, renumbered from 0178 because 0178_source_engagement
(M63-37) already occupied that number. Snapshot policy gained `sessions.roles` = drop_column
(2628 cols / 362 tables). The thinkcentre checkout is ff-merged to 249a2bce and clean.

Gate added alongside the merge (3cc2399e): `scripts/check-migration-numbers.py` in `gate.sh repo` refuses a repeated
number per engine, a number in one engine only, and an unnumbered file. Proven by planting each failure.

M64-08 (2e070f4f): the fandom adjacency map is configuration — [fandom_graph] version + weighted edges,
validated at load; db::fandom_graph::related_works orders by WEIGHT then likes and returns only
published/public works; GET /api/v1/fandoms/{fandom}/related publishes the map version, both neighbour
lists and the works. Row stays planned: tag.cross_fandom and author.similar have no route in this build,
so the map is not yet reached through the two surfaces the card names.

MERGED thinkcentre 3a32119d (W7 task 2 step 5 first batch: 4 routes classified, two admin doors that had no gate) as f1cb3ffe — no migration, no collision; route_inventory 5/5, admin_scope 1/1, clippy clean.

M71-12 backend (9fd5955a): GET /api/v1/facets/words returns 10 population-equal (quantile, not equal-width) buckets
over the same filtered set as /api/v1/facets, with low/high/count/share and measured/unknown/when_unknown; unmeasured
rows are excluded and stated, empty buckets dropped and renumbered. Row stays planned on the drawing control and on
M71-11's p95 measurement. M71-11's note corrected: /api/v1/facets has returned counts since M64-10, so its bookmark
("no route returns a facet count") was stale — the remaining work is the UI half and the measurement.

M71-13 (bf404997): db::taxonomy::dimension_coverage publishes works/catalogue/share/sentence in one statement; wired as
`coverage` on /api/v1/tags/{tag} and /api/v1/fandoms/{fandom} and into the document tag page (bare count replaced).
Zero-work and empty-catalogue cases have their own sentences. Row stays planned on collection coverage and on the
facet panel's leading line (UI; the API already publishes known/unknown/total).

M71-13 collections (a7664d54): events::collection_coverage counts entries vs readable entries INSIDE the collection
(published+public+undeleted), publishes hidden, three sentences (partial / all readable / empty). GET
/api/v1/collections/{id} carries `coverage`. Only the facet panel's leading line remains, which is UI — the API publishes
known/unknown/total per facet.

M71-09 (f3aca3fc): ResumeState (current/stale/unmeasured/conflict) with percent(): Option<u16> and display() '—';
positions_with_current_revision joins each stored row to its unit's current revision; /api/v1/reading/progress
publishes `resume`; save_progress now stamps the server's current revision when the client omits content_revision
(without that, a position could never be proven stale). Row stays planned on the reader bar/choice UI.

M71-14 (b799f828): settings provenance — GET /api/v1/settings/privacy publishes per-key {value, source, reset, note};
DELETE /api/v1/settings/privacy/{key}[?pseud_id=] clears one level (204/422/404 discipline). Migration 0180 adds
privacy_settings.origin because onboarding-written defaults were indistinguishable from choices and were reported as
the reader's own six decisions. Remaining: per-work level, deep links, and typography/gravity in the same vocabulary.

MERGED thinkcentre 9e65ab8b + 5375d879 (W7 task 2 step 5 batches 2-3: succession and retention move to the cross-site door, a fourth operator predicate dies) as c6d1a65f — no migration, no collision; route_inventory 5/5, admin_scope 1/1, clippy clean.

M71-15 (6c35aee9): the query grammar gained a third order — ValueKind::Rank — so `rating:<=mature` parses and renders as a
CASE rank; RATING_RANKS in query.rs is the single table (general<teen<mature<explicit). domain::filter_state holds the one state
{text, word window, ceiling, sort, pinned} and renders it twice: canonical query_string() and chips() (pinned ones neither
editable nor removable, with the reason) plus unexpressed_note(). GET /api/v1/search returns state{query,chips,unexpressed_note}.
Ideas rows M63-37/M63-38 removed because ideas_catalogue.py cannot run on this machine (two vault files absent).
Remaining: the panel UI, and provenance/source as real filters.

MERGED thinkcentre 2094ce7c (W7 task 2 step 5 batch 4: nine more operator predicates become one decision) as d2f95d57 — no migration, route_inventory 5/5, admin_scope 1/1, m71_15 4/4, clippy clean.

MERGED thinkcentre 2de6811a (W7 task 2 step 5 batch 5: the last config-gated door; refusals reach the modlog) as f7e237f5 — admin_scope 1/1, route_inventory 5/5, clippy clean. W7 task 2 step 5 complete.

MERGED thinkcentre 32a1b331 (W7 task 3 step 2: one-click envelope wired on the existing seam) as c8951a10 — new routes/admin_actions.rs, governance +66, server +7; route_inventory 5/5, admin_scope 1/1, m71_11 3/3, clippy clean.

M71-11 (41e847e4): facet counts measured, then cached. Pre-registered rule (82db6c0e): p50<=100 / p95<=200 on 50 sequential
requests -> FIRST RUN FAILED (p50 297 ms / p95 372 ms); EXPLAIN showed three seq scans + a disk sort, so no index helps.
crates/app/src/facets_cache.rs memoises per (database, filter set) for 60 s; responses publish computed_at_ms_ago and
cache_ttl_seconds. After deploy: warm p50 2.83 ms / p95 6.05 ms; cold 1,174 ms on the first request post-restart.
Same commit fixed two PG-only defects in the facet scan (SUM->NUMERIC decode, `?n` placeholders never rewritten) that made the
whole endpoint 500 on the live catalogue; crates/db/tests/facets_two_engines.rs is the guard. Remaining: the UI half.

## M16-07 — review workflow (implemented)

Stages live in `crates/domain/src/extension_review.rs` as a pure state machine; the routes in
`crates/app/src/routes/extension_review.rs` only store what it decided. Rules: no stage skipped, the submitter never
reviews, permission and security review are two different people, two distinct approvers publish, and a published
version is finished unless it is revoked.

Two things worth knowing:

* **Migration 0183** rebuilt `extension_manifests` with `PRIMARY KEY (id, version)`. Keyed on `id` alone, a second
  version was a UNIQUE violation — so "roll back to the previous published version" described a row that could not
  exist. The rebuild copies every existing row.
* **Automated checks are recorded, not dispatched.** `POST /extensions/reviews/{slug}/{version}/checks` takes the
  results an operator got from `/operator/sandbox/run`. Submit does not run the sandbox because it does not know where
  the package bytes live; when a package store exists, submit should call the runner and post its report.

Renewal-on-growth is evaluated at publish against each active grant's own capability set, which is why a 1.1.0 that
asks for `network` invalidates a 1.0.0 consent for `work.read` instead of inheriting it.

## M16-08 — gallery mechanics (implemented)

Installations and ratings in `crates/db/src/gallery.rs`; the rules in `crates/domain/src/gallery.rs`.

* **Idempotence is a statement, not a check.** `ON CONFLICT(id) DO UPDATE … WHERE extension_installations.uninstalled_at IS NOT NULL`
  gives three outcomes from one write: a new row, a silent no-op on an active row, and a reactivation after uninstall. The partial unique
  index on `(scope, manifest_id) WHERE uninstalled_at IS NULL` is what stops two concurrent installs from producing two active rows.
* **The count is a census.** `COUNT(*) WHERE uninstalled_at IS NULL` at read time. Reinstalling cannot inflate it, and it can go down.
* **The scope is the acting pseud**, taken from the session. A body field would let an account install as another identity.
* **An update never widens a grant silently**: the policy resolves the target, then the target's declared capabilities are compared with the
  grant, and the refusal names only what is new.
* **Licence compatibility is a rule about identity, not a lookup.** `CC-BY-SA-4.0` cannot be forked under `MIT`, and the classification is
  an explicit list because a prefix rule on `CC-BY` would put share-alike in the permissive bucket.

Two live PostgreSQL defects were found by `gallery_two_engines` (both engines green now): `pseuds.id` is a uuid so the review join needs
`::text`, and `stars` is `INT4` so it is cast to `BIGINT` to keep one decode type.

Not built: any UI for the gallery, and notifications on a new review — the routes are the mechanism, and a page is a separate row.

## M16-11 — the paid marketplace (implemented)

`crates/domain/src/payments.rs` (arithmetic) · `crates/db/src/payments.rs` (store) · `crates/app/src/routes/payments.rs` (doors).

* **Separate from credits, by construction.** Nothing here reads a credit balance and nothing in the credits tables reads this ledger. A
  credit balance must never be readable as a price, so the separation is structural rather than a rule in prose.
* **A purchase grants nothing.** It records a payment in `pending`; capture is the moment money exists, and the only moment the grant and
  the seller's share are written. With no provider configured, `Processor::Manual` returns `captured: false` and an operator settles it
  against a bank transfer — the honest behaviour, and the reason `pending` is a state readers see.
* **The split runs on the net of tax**, 85/15 with the remainder to the developer, so the halves sum to exactly what was charged.
* **Refunds are cumulative in one column** with `refunded_minor + amount <= amount_minor` in the statement itself. A full refund revokes the
  grant and reverses the seller's share; a partial one does neither.
* **Tax is inclusive and rounded half-up** from a stored profile, so `net + tax == gross`. Storing a profile is not registering for tax, and
  the response says so.
* **Money columns are BIGINT.** `INT4` overflows at ~21M euros; the two-engine test caught three `INT4` decode failures on PostgreSQL before
  the change, the same class the gallery row hit.

Gaps named in the ledger: no invoice document rendering, no processor webhook, and extension revenue settles through the existing
`author_earnings_ledger` → `request_payout` path rather than a separate payout ledger.

## M16-14 — webhook operations (implemented)

`crates/domain/src/webhook.rs` (grace window, `RateLimit`, `Breaker`) · `crates/db/src/webhooks.rs` (store) · `crates/app/src/routes/webhook_ops.rs` (doors) ·
`crates/app/src/webhook_delivery.rs` (the loop that enforces all three).

* **A rotation is a window, not a switch.** The old secret stays admissible for 24 hours because an instant rotation breaks every
  subscriber mid-deploy, and the failures it causes look like an outage rather than a configuration change. The header carries
  `v1=<current>,v0=<previous>` so a receiver needs no special case after rotating.
* **The rate limit counts deliveries, not attempts.** Throttled rows are excluded from the count — otherwise the limiter consumes its
  own budget and a burst extends its own silence — and each skip is *recorded*, because an absent delivery reads as an outage.
* **The breaker counts consecutive failures.** A subscriber that fails once a day for a year is flaky, not gone; twenty in a row is a
  receiver that has stopped answering. A success resets the count, but a success while switched off does not resurrect the endpoint:
  only an explicit, owner-scoped enable does.
* **The operator view omits secrets by construction.** An operator needs to know an endpoint is failing, not to be able to forge its
  deliveries.

Two tests run the real `deliver_notification` loop against a local receiver rather than asserting the arithmetic again — the mutant that
removed the limit check reddened exactly those. Remaining gaps (in the ledger): no retry backoff of its own (the outbox owns retries), no
per-event-type limit, and no notification to an owner whose subscription the breaker switched off.

## M16-13 — the theme model (implemented)

`crates/domain/src/theme_model.rs` (vocabulary, contrast, precedence) · `crates/db/src/theme_profiles.rs` (store) ·
`crates/app/src/routes/themes.rs` (doors) · `migrations/*/0187_theme_profiles.sql`.

* **A theme is data, not a document.** Ten tokens, a closed placement set, and `declarations()` emits custom properties and nothing
  else. That single decision is what makes "a theme cannot hide the safety controls" a property of the vocabulary: a theme has no
  selector, so it has nothing to aim at.
* **Slots cannot be removed.** A theme changes a placement from a closed list; a missing slot means *core*, never *gone*, and the
  renderer walks `Slot::all()`.
* **Scope is where the constraint lives.** An instance theme lays out the building; a reader theme recolours one room and keeps the
  building's layout. The check is in the model, so a future route cannot publish what `validate` refuses.
* **Publish is the gate, save is the form.** Both refuse a contrast failure, and publishing re-validates the stored row rather than
  trusting the save: a draft written when the rule was looser must not slip out later.
* **Reset deletes.** Writing the defaults would turn "reset" into a theme that drifts the next time the defaults change.
* **Safe mode beats the preview.** The preview is the theme most likely to be broken, so it is exactly what safe mode must override.

Gap, stated in the ledger: the model makes hiding *inexpressible*, but the renderer half is a contract, not a verified fact — no
frontend test asserts that the four protected surfaces sit outside the theme root, and there is no theme console UI.

## M16-12 — the acceptance list (implemented)

`crates/app/tests/m16_12_acceptance.rs` (one test per claim) · `crates/domain/src/recipe_document.rs` (the recipe schema and the
processing rule) · `crates/app/src/routes/extension_dispatch.rs` (the dispatch point).

The row closed on a decision rather than a feature: **"custom engines and recipes honour opt-out" has two answers, and they are not the
same answer.**

* An engine runs code or drives a process outside the host, so `custom_processing: refuse` governs it, and the refusal names the setting
  rather than the package.
* A declarative recipe composes strategies the instance already runs, so it hands nothing outside and consent does **not** govern it.
  Refusing an opted-out reader here would be a false positive that teaches readers the setting is noise.

That is structural, not conventional: a recipe's schema is closed (`stages[{strategy, weight}]`, weights in (0,1], at most eight, no
duplicate strategy), so a document that could carry an endpoint is refused *by name*. `processing(category)` is the one place that
answers which categories run, the route asks it before reading anything out of the body, and strategy names are validated against
`rec_engine::available_strategies()` rather than a second list that would drift.

Gaps named in the ledger: the blend does not read recipe documents yet (M69/M64 own that), so an applied recipe is validated and stored
but does not change the feed; and there is no engine runner to hand an engine package to.

### M69-09 — reader affinity (implemented 2026-11-07)

One stored 0..10000 score per reader with its three components (favourite overlap 0.5, tag similarity 0.3, arena
agreement 0.2), computed in a sweep that starts from the evidence tables. An absent signal is removed from both halves of
the mean instead of being counted as zero — a reader with no arena history is not "0.2 less aligned" — and a measured
zero is still a score. `evidence_bp` travels with `score_bp`.

The sweep is **operator-triggered** (`POST /operator/affinity/recompute`): there is no scheduler in this tree, so a
`tokio::interval` inside a handler would be a timer per request multiplied by the process count. The upsert means a
second run repairs rather than doubles, and a repeat over unchanged data reports zeros (a run that reports a change every
time is a sweep matching nothing). New evidence is absorbed by the next run.

Not wired yet: nothing consumes the score (trust speed, badges and membership are the intended consumers), so this is a
reading of evidence with a door, not a feature a stranger can see. The four PostgreSQL casts (`::uuid` on account and
work ids, `CAST(... AS TEXT)` on selects) are not decoration: the SQLite run passed while the PostgreSQL run failed on
three separate decodes before them.

### M69-11 — arena pairing (implemented 2026-11-07)

The round is chosen, not shuffled. A fandom group is sorted by rating and a window of four slides over it: **proximate**
minimises the span (a comparison that sharpens what the ladder already believes), **exploration** maximises it (a
comparison that locates a work), and both break an exact tie on the largest mean **deviation** — within an equally tight
window, the cards that can still move are the ones worth asking about.

The roll is passed into the domain function rather than drawn inside it, so "explores about epsilon of the time" is
testable: epsilon 0 never explores, 1 always does, and the route draws the roll from `rand` and takes epsilon from
`discovery.arena_epsilon` (refused at load outside 0..=1 rather than clamped — a probability outside its range is a typo,
and silently making it 0.15 leaves an instance believing it configured exploration).

**The calibration window is recorded, not applied.** A round names every card with fewer than five comparisons, a card
with no reading at all counts as inside it, and the ballot stores `calibrating` so a consumer can leave the vote out of the
global number without re-deriving which ballots were early — which it could not do anyway once more ballots arrive. Nothing
refuses the vote and nothing adjusts a rating here; the ladder arithmetic is `polaris-taste`'s (ADR 0001).

Gaps named: the deviation tie-break only breaks *exact* ties (a continuous cost was rejected because it makes the reason
sentence unquotable); no client renders `mode`/`calibrating` yet; `arena_appearances` counts best/worst only, because the
middle two cards of a round are never stored, so it is a floor rather than an exact count.

Two dialect facts paid for on the way: `arena_ballots.calibrating` is a BOOLEAN on PostgreSQL and INTEGER 0/1 on SQLite
(the repository's flag convention), which the PG campaign caught as `column "calibrating" is of type integer but
expression is of type boolean` — after the migration had been written IDENTICAL for both engines.

### M69-13 — trust velocity (implemented 2026-11-07)

The multiplier applies to the **score**, before the ladder is read: alignment factor `0.5 + affinity` (M69-09) and behaviour
factor `0.5 + Laplace-smoothed share of confirmed reports`, each **neutral at 1.0 when there is no evidence**, product
clamped to `[0.25, 2.5]`. The level written is `max(held, earned)`, so a multiplier can slow the next level and can never
take the one a reader holds — demotion stays the confirmed-report path, which has an appeal.

The basis carries the multiplier, both factors, and a sentence explaining the floor, so a support answer can name the
number. Off by default (`[trust] velocity`).

Honest limits: with today's factor ranges the product cannot leave `[0.25, 2.25]`, so the clamp is a guard proved on
`velocity_from_factors` rather than a participant; behaviour quality reads reports only, so it rewards good *reporting*,
not good judgement; and nothing renders the multiplier to a reader yet.

**A defect fell out of this row and is worth more than the row.** `days_since` parsed only RFC 3339, and PostgreSQL returns
`accounts.created_at` in its own rendering (`2026-03-02 20:38:18.441367+00`) — so on PostgreSQL the **tenure axis read
zero for every account**, silently, and every level on that engine omitted it. The parser now accepts both shapes and is
unit-tested against both; recorded against DM3-02. On the live instance the next recompute will raise levels for accounts
that were always entitled to them, which is a change to announce rather than to let happen.

### M69-14 — a live badge, gained and lost (implemented 2026-11-07)

Attuned is a **state**, so it cannot reuse `badge_award` (unique per account+badge, an event with a credit bonus). Two tables:
`live_badge_state` (what is held now) and `live_badge_events` (when it changed and why). Eligibility is two clauses —
`affinity >= 7500` **and** `measured weight >= 8000` — because M69-09 reweights over present signals, so a one-signal reading
can be 10,000 and a badge is a fact. The reason names the clause that failed.

A recompute that changes nothing writes no event; a change writes a row and enqueues `badge.gained` / `badge.lost` on the
outbox. `since` is cleared on loss. Events carry a `seq` so "newest first" is the order things happened in, not a tie-break.

Decided inside the affinity sweep (the requirement is "recomputed with affinity"; a second door is a second thing to forget).
Off by default (`[discovery] live_badges`): when off nothing is decided (no state row) and `/me/badges` answers 404, so a reader
cannot learn the instance measures them and hid it.

To enable: `[discovery]` → `live_badges = true`, then run `POST /api/v1/operator/affinity/recompute`.

Limits: no page renders it yet (frontend rows own that); the sweep is operator-triggered so a badge is as fresh as the last run;
only Attuned exists; a reader measured by exactly one signal is permanently ineligible by design.

### M69-02 — peering suggestions, with the hardcoded layer as a veto (implemented 2026-11-07)

Cosine over the published tag vector, the existing 0.25 floor, and — the half that was missing — a **veto**: a rule one side
declares and the other does not is a block, checked *before* any score is computed. Symmetric (a peer that enforces more is
also in collision), names the rule and the side enforcing it.

The layer is declared (`[discovery] enforced_rules`) and published in `/.well-known/taste-fingerprint.json` as `enforced`;
never inferred, for the same reason fandom adjacency is declared (M64-08) — a veto inferred from a sample can be outvoted by
a larger sample. A peer whose document predates the field collides with nothing, because an unanswered question is not an
answer.

Doors (both operator-only, both 404 unless `[discovery] peering_suggestions`):

    GET  /api/v1/operator/peering/suggestions   # ranked verdicts + reason per peer
    PUT  /api/v1/operator/peering/fingerprints  # record a peer's document (nothing fetches yet)

Order is the answer: suggestions, then vetoes, then near misses.

Limits: nothing fetches peer documents; the list does not write `federation_peers_v2`; the veto matches rule *names* only, so
differently-spelled rules collide spuriously — the fix is a controlled vocabulary, not a fuzzy match.

### M69-03 — a signed taste passport (implemented 2026-11-07)

ed25519 over canonical JSON (sorted keys, no whitespace, integers only in the signed surface — the tag vector is basis points,
because float formatting is the part of JCS that fails intermittently rather than loudly). Both doors sign/verify through one
function, so the round-trip test (export → import) is the real assertion.

The document is a **claim**: signed by the instance, not the reader. Its taste lands as `origin = 'imported'` taste signals, so
local evidence outvotes it as it accumulates. Unknown `version` refused rather than partially read; 180-day expiry checked in
both directions; caps on tags, sources and weights.

    GET    /api/v1/me/passport           # export, signed by this instance (needs [discovery] passport_key)
    POST   /api/v1/me/passport           # present one: verify, receipt, seed imported taste
    GET    /api/v1/me/passports          # receipts
    DELETE /api/v1/me/passport?origin=…  # receipt and the taste it seeded

Off by default (`[discovery] taste_passports`). The export carries canonical tag names (not local node ids) and only the
**public** key.

Found on the way: two renames the store had left behind — `chapter_revisions` → `revisions` (0122) and `taxonomy_nodes` →
`canonical_entities` (0124). Both queries compiled green and would have failed on any live instance.

Limits: nothing fetches a peer's passport; no UI; not linked to polaris-id, so "same person" rests on the reader presenting it;
`favourite_sources` is exported but unconsumed.

### M69-04 — a tombstone that survives re-import (implemented 2026-11-07)

Keyed by **work identity**, not URL: title + author + language, normalised (NFKD, lowercase, marks dropped, punctuation and
spacing collapsed) and hashed. A takedown that stops one string stops one import; the work comes back from a mirror
tomorrow, which is the failure a takedown exists to prevent. The body is not the key — imports arrive without one, a body
legitimately changes, and two mirrors rarely match byte for byte. `text_key` exists for a later second key, unused today.

Normalisation is deliberately conservative: "The Storm, part 2" is a different work from "The Storm" (an over-matching
tombstone removes a sequel nobody complained about), and an empty author is not a wildcard. A retitled copy escapes — the
cheaper failure.

The refusal is checked **before the library row is written**, in both the URL import and the bundle restore, so a refused
import leaves no item and no units. The queue row is `failed` with code `tombstoned` and a message citing the tombstone id,
its reason and its filing date — permanent, so the worker does not keep asking.

Every refusal increments `refusals` / `last_refused_at`: a tombstone with a zero is a mistyped title, and that is the only
way an operator can tell.

Withdrawal writes `withdrawn_at` + reason rather than deleting, because the row is the answer to "why did my work vanish?".
Re-filing re-arms it and keeps the paperwork reference.

    POST /api/v1/admin/tombstones                 # file by identity; the response echoes the key it derived
    GET  /api/v1/admin/tombstones                 # withdrawn ones included, with refusal counts
    POST /api/v1/admin/tombstones/{id}/withdraw   # needs a reason

Off by default (`[discovery] work_tombstones`), read at the point of refusal. Doors answer 404 while off, 403 for a
non-operator while on.

Found on the way: dropping combining marks **without decomposing** split `Á` from `A`+acute into two keys — caught by the
accent test; NFKD before lowercasing is the fix.

### M69-05 — mirroring by taste, attributed on the page (implemented 2026-11-07)

The reader-visible half of instance-death protection (M45-39 is the paperwork). Who may hold a copy is **computed** from the
fingerprints peers publish against this instance's own: `peering::assess` decides, so a collision veto beats a high score and
is reported as the rule with `similarity: null` — a veto is not a low score.

The mirror floor is **0.5**, twice the suggestion floor: a suggestion says a reader might like it there, a mirror commits
somebody's work to another host's keeping. `mirror_floor` in config can raise it, never lower it. Mutation testing caught
that the first cut of tests only exercised peers below 0.25, so deleting the mirror check left the suite green; a peer at
0.372 now holds the gap under test.

    GET  /api/v1/operator/mirroring/eligible            # who may hold a copy, and why the others may not
    POST /api/v1/operator/mirroring/{work}/mirrors      # record one (refused with the number if not eligible)
    POST /api/v1/operator/mirroring/mirrors/{id}/retire # lost or withdrawn, reason required
    GET  /api/v1/works/{work}/mirrors                   # the sentence a work page prints

One row per (work, peer): re-recording replaces, and re-recording after a retirement re-arms it. The similarity stored is the
one the decision was made on. Retirement keeps the row — a reader who saw the line yesterday is owed an explanation.

The public door is anonymous and narrower than the operator one: published, public, undeleted works only (404 for a draft).
A closed door answers 404 to *every* request, bodies parsed after the switch check — an extractor running first answers 422
and tells a caller the route exists.

Off by default (`[discovery] work_mirroring`).

Limits: nothing transfers bytes (no peer API client), nothing verifies the peer still serves it, and the SPA does not render
the sentence yet — the door returns it.

### M69-06 — the perceptual-hash rescue ring (implemented 2026-11-07)

Two hashes, two claims. The **content hash** is identity: a survivor reporting the same bytes may be credited without a human.
The **perceptual hash** is evidence — it survives re-encoding, which is why it finds a copy and why it cannot prove one; the
failure mode is a work's page showing somebody else's picture. So a perceptual match is a proposal, never an attachment:
nothing here writes availability links, because attaching is `media_resilience`'s audited path.

A tie is an answer: two equally close survivors (or two holding identical bytes) produce `Ambiguous` with both names and **no
stored match**; `accept` then requires the operator to name one. Dimensions are a second lock — a candidate more than 1.25×
off in either axis is excluded, because a thumbnail beside its original is a legitimate pair and the wrong rescue — while an
*unknown* dimension never disqualifies.

    POST /api/v1/operator/rescue/copies            # a survivor says what it holds
    GET  /api/v1/operator/rescue/copies
    POST /api/v1/operator/rescue                   # {dead_peer, media_reference_id} → outcome
    GET  /api/v1/operator/rescue                   # every hunt, settled ones included
    POST /api/v1/operator/rescue/hunts/{id}/resolve

Every hunt is a row, `not_found` included — "we looked and nobody had it" is the answer an operator needs most. The outcome
and the threshold travel with it; a later copy must not rewrite what the operator was told.

Two defects found by doing it: the dead peer was first a **path** segment (`https://…` is two segments to the router, so
every hunt 404'd), and `$n::timestamptz` binds into TEXT columns rewrote the stored stamp into PostgreSQL's rendering.

Off by default (`[discovery] media_rescue`).

Limits: nothing fetches a survivor's copy (no peer API client) — the ring matches claims; `accept` records who has it, the
transfer is the operator's next step; hunts read at most 500 recorded copies; nothing decides a peer is dead automatically.

### M69-07 — the evacuation plan and its receipt (implemented 2026-11-07)

An instance announcing it is going away hands its works to taste-similar peers and writes down **who took what**. The plan is
a pure function (`domain::evacuation::plan`): works in, ranked eligible recipients in, placements out. Deterministic — the same
inputs give the same plan on every run, or two operators disagree about who took what.

**Balance is a rule, not a preference.** Filling every placement with the closest peer is the naive plan and a bad one: the
peer closest to *everything* takes everything, and an evacuation that concentrates a whole catalogue on one host has recreated
the single point of failure it was performed to avoid. So the plan fills the least-loaded peer *inside* a 0.15 similarity
window first, and only outside that window does taste win.

**The fork is the failure.** `UNIQUE (notice_id, work_id)` stops the plan placing one work twice. The receipt's job is the
other half: a second evacuation — an instance that closed and announced again, or one revived under a new operator — cannot
hand a work to a different peer once someone accepted it. The refusal names the peer *and* the notice.

    POST /api/v1/operator/evacuation                        {reason, note?, limit?}
    GET  /api/v1/operator/evacuation                        notice + assignments + receipt + forks
    POST /api/v1/operator/evacuation/assignments/{id}/settle {state: accepted|declined, note?}
    POST /api/v1/operator/evacuation/close                  {state: withdrawn|completed}

Recipients are **computed** (`mirroring::eligible_peers`, same floor as mirrors), never typed: an operator picking hosts by
hand is how a shutdown becomes a copy of somebody's catalogue on a host nobody vetted. With no eligible peer the announcement
is **refused** rather than planned empty — a receipt of zero looks like the work was placed.

Off by default (`[discovery] evacuation`).

Limits, named honestly: (1) nothing transfers bytes — `settle` records what a peer said, and the transfer is the operator's
next step through existing machinery; (2) no peer authenticates a claim, so a recipient that says "we have it" and does not is
indistinguishable here from one that does; (3) the plan reads at most 1000 works per announcement and ignores works that are
not `published` + `public`; (4) nothing notices a shutdown automatically — an operator announces it; (5) a second acceptance
answers 422, not 409, because this error vocabulary's only conflict variant names two version numbers this refusal does not
have, and inventing a version pair to reach a 409 would be a lie in the payload.

### M69-10 — Glicko-2 for the arena, and the arena nobody could reach (implemented 2026-11-07)

Elo has no notion of confidence: a card with two ballots and a card with two hundred sit on the same ladder as if their numbers
meant the same thing. Glicko-2 carries a rating, a deviation and a volatility, so "provisional" becomes a number.

`domain/src/glicko.rs` is Glickman's algorithm, steps 1–8, reproducing the paper's worked example (1464.06 / 151.52 / 0.05999)
to within 0.05. 1500-mean scale preserved, so nothing a reader saw changes meaning. A card enters at its Elo rating with
**maximum** deviation — an Elo number is a position without an uncertainty — and the deviation then shrinks by being compared
and grows with silence (additive in variance: reaching the ceiling from deviation 60 takes about 1200 updates at σ = 0.06).

**The divergence flag was re-derived once, in the open.** The first rule was "a ballot that moves the rating further than
twice its own deviation"; a measurement killed it — a settled card (deviation 40) beating an opponent 600 points above moves
**26** points against an 80-point bar, because Glicko-2's rating step is bounded by the squared posterior deviation, not by the
surprise. A rule that cannot fire on a four-card ballot is decoration. The flag now means what an operator can act on: this
ballot **inverted a gap of at least 300 rating points**.

**Two defects in the Glicko-2 arithmetic, both caught by the paper's example:** the volatility step returns `e^(x/2)`, not
`e^x` (returning the square of the volatility makes every number look plausible and the deviation barely grow), and the
`k` counter in the Illinois bracket is an `f64`.

**The real find:** `GET /api/v1/roadmap/arena` and `POST /api/v1/roadmap/suggest` had **never worked over HTTP**. They lived in
`roadmap::router()`, nested into the outer router at `/api/v1` — and a router keeps only the last `nest` for a prefix, so the
`.nest("/api/v1", api)` further down silently discarded them, while `read_router()`'s public board (merged at the root) worked
next door. The store tests passed, the route table listed the paths, and a reader got a bodiless 404 from a surface that was
"mounted". Fixed by putting both routers in `api`, the router that is actually nested, and the new test requests the arena
through HTTP rather than trusting the table.

    [roadmap] glicko = true    # off by default: a rating plus an uncertainty is a claim about the board

Limits: the arena still pairs candidates by Elo and match count — "prefer uncertain pairs" is available from the deviation now
but not wired into `arena_candidates`; `diverged` describes the last ballot that moved a card, not its history; and the
federation ladder's numbers remain `polaris-taste`'s under ADR 0001 (M69-10's consumer half, `db::ladder::is_settled`).

### M69-22 — the morning queue and the daily surprise (implemented 2026-11-07)

Two shapes, two jobs, and neither is a new surface: both are drawn from a **saved filter**, so the operator retunes the queue
without a deploy and the response says which filter answered.

    GET /api/v1/discovery/morning
    → { day, filter: {slug, name, params, url}, queue: [...], surprise: {...} }

**The queue is diverse by rule, not by hope.** Five excellent works from one corner of the archive read as one recommendation
repeated. So the selection is a round — first from each fandom, then the second from each, stopping at two per fandom — which
keeps the filter's own ranking *inside* each bucket and still puts the pool's best work first. Round-robin is over
first-appearance order, so the result is deterministic; untagged works share one `unknown` bucket, because otherwise a
catalogue of untagged rows would look perfectly diverse. A pool too small to fill the queue is filled rather than padded: the
cap governs a *diverse* choice, not a short one.

**The surprise is stable for the day and different between readers.** Seeded by FNV-1a over (day, reader), not `DefaultHasher`
(which is explicitly not stable across Rust releases) and not `rand`. A surprise that changes on reload is a slot machine, and
one that is the same for everyone is not a surprise.

**Nothing here invents a filter.** The queue names a slug on the operator's account (`[discovery] morning_queue_view`); off,
unnamed, or pointing at a filter that does not exist all answer **404** — an empty 200 would say "we looked and found nothing"
when the truth is "nobody configured this". A **private** filter is refused with the reason, because serving it would publish the
filter's contents to every reader, and a **library**-scoped view is refused too: the queue runs a public catalogue filter.

Missing piece named: M69-23 (a saved filter as an import's intake policy) and M69-21's public life (subscriptions, keybinds,
blends) are separate rows; this one is the reading surface.

### M69-21 — a saved filter's public life: the keybind, the follow, the blend (implemented 2026-11-08)

The filter already existed (0009) and already had a slug (0173). What was missing was every way a reader *uses* one without
rebuilding it, and all three additions are relations to that object rather than a second storage shape for a filter:

    PATCH  /api/v1/saved-views/{id}/keybind   {keybind: 1..9 | null}
    GET    /api/v1/me/keybinds
    POST   /api/v1/saved-views/{id}/follow     DELETE the same path
    GET    /api/v1/me/followed-filters
    GET    /api/v1/saved-view-blends           POST the same path
    GET    /api/v1/saved-view-blends/{id}/items   (the blend, answered)

**Digits 1–9 and nothing else.** `0` is the key beside `backspace`: a binding there would be pressed by accident more often than
on purpose, and ten is not a number key. The CHECK lives in the migration as well as the route, because the route is not the
only writer a script has.

**A collision is refused, not resolved.** Two filters on one digit means the reader presses one key and lands on a surface they
did not expect, and the second filter is unreachable in a way they cannot see. The refusal names the digit so the choice is
theirs.

**Following is one row per (reader, view)**, and it is deliberately *not* `search_alerts`: an alert belongs to the pseud that
owns the view, and a subscription belongs to the reader who asked. A private filter cannot be followed — otherwise the owner's
next edit publishes into a stranger's feed — and that refusal is in the store as well as the door.

**A blend is two filters answering one question.** `interleave` alternates by position, keeping each side's own order;
`union` appends with the left-hand position winning a duplicate. Nothing is re-ranked: a blend that reordered a side would be a
third ranking, and the reader who built each filter would not recognise their own results. A third mode (weighted) was
considered and rejected — a weight is a ranking question dressed as a filter, and one filter with both conditions already
expresses it. Both sides must be the caller's own filters: naming somebody else's private view would be a read of it through a
new door.

Off by default (`[discovery] saved_filters`), so every door answers 404 on an instance that has not chosen it — and the switch
is on the `library` module, because a saved filter *is* a library object.

### M45-44 — sister instances: publish, subscribe, and a directory by topic (implemented 2026-11-08)

Four surfaces, one vocabulary. `GET /federation/picks` publishes an ActivityStreams `OrderedCollection`; an operator names a
peer's collection at `PUT /operator/federation/picks`; the worker's maintenance pass fetches; `GET /federation/peer-picks` shows
what came back; `GET /federation/directory?topic=` finds instances by the topics they publish.

**A pick is a public catalogue row at Aggregate order, and the alternatives lost for different reasons.** Operator-curated picks
would need an editorial surface that does not exist (§16.11), so the federation feature would depend on an unbuilt admin UI.
"This instance's own works" is nearly empty on an instance that imports more than it publishes — a peer subscribing would get
nothing, which is the failure mode that looks like a working feature. The anonymous discovery feed is ordered by trend and
freshness, which changes hourly: a polling peer would read every fetch as a new set of picks. Aggregate order is the catalogue's
own default, so the collection is stable between fetches and inherits the catalogue's eligibility for free (M71-16) — a
restricted work cannot appear in the collection for a different reason than it cannot appear on the page.

**A fetch replaces, it does not accumulate.** The collection is the peer's *current* list. A work they stopped recommending must
stop being shown here, or the surface becomes an archive of what a peer once liked and a reader cannot tell which is which. The
delete and the inserts are one transaction, so a failure mid-write cannot leave a peer with no picks at all.

**Every failure path records.** The first version recorded only the failures that got as far as an HTTP status, so a host that
does not resolve — the case an operator most needs to see — left the row saying "1 picks" from a fetch that had worked an hour
earlier. The test caught it. Now `last_status` and `last_error_at` are written on the transport error, the non-2xx, the parse
failure and the store failure, and a failing fetch changes no picks: an unreachable peer is not evidence that its list is empty.

**A failing peer still advances `last_fetched_at`.** The sweep orders by least recently *attempted*, not least recently
succeeded: a peer that never advances would be retried every pass, at the head of the queue, forever.

**The subscription URL must be the host it is filed under, and must be https.** A subscription is a request made on the
operator's behalf; one filed under host A while pointing at host B is a way to make this instance fetch somewhere the operator
did not say they were fetching. Loopback http is refused at the door too, which is why the fetch test seeds its subscription
through the store — the door's rule is proven by its own assertions, and the test is about what happens to a peer's document.

**Re-pointing is an update, not a second row.** The host is the identity: two feeds from one host would show every pick twice and
no reader could tell the rows apart. Disabling is `enabled = 0`, kept rather than deleted, so turning a peer back on does not lose
when it was first trusted — and `recent_picks` joins the subscription so a disabled peer's picks stop being shown without being
destroyed.

**A pick is never reconciled with a local work.** A pick that matches something in this library is still shown as the peer's, by
URL. Attributing a pick is the point; guessing an identity match would turn "they recommend this" into "we think this is the same
thing", which is M69-05's question asked badly.

**The directory matches whole topics, case-insensitively, and omits what it does not have.** A substring match would make `fluff`
find every `fluffy` instance, turning a directory into a guess. Instances that do not name the topic are omitted rather than
returned at weight 0.0 — "we have nothing on this" and "we have a little" are different answers, and only one is a hit (a mutant
that scored a miss 0.0 was caught by this). Each hit carries `topics_named`, because a weight of 0.4 means one thing on a
three-topic instance and another on an eighty-topic one. Private themes are never listed; a topic nobody names is an empty list
rather than the whole directory; a search with no topic is a 422 rather than a silent "everything".

Proof: `crates/app/tests/m45_44_peer_picks.rs` — three tests on SQLite and PostgreSQL, the fetch one against a **real loopback
HTTP server** (a mocked client proves the parser was called, not that a collection a peer actually serves is read). Six mutants
red: replace→merge (the second fetch hits `UNIQUE (subscription_id, work_url)`), `recent_picks` ignoring `enabled`, the host-match
rule off (200 for a URL on another host), the dedupe off, a missing topic scored 0.0 instead of omitted, and private themes in the
public directory. `route_inventory` 5, `migration_catalogue` 6, clippy clean, orphan baseline unchanged at 181.

### The ratchet found a missing half, not a missing door (federation.rs, 2026-11-08)

`crates/db/src/federation.rs` holds 8 orphans and they are not a surface gap like the coordinates and batch-window
modules were. Reading them before wiring produced the reason: **darkhold federates in, and has never federated out.**

- `ap_inbox` verifies signatures and stores activities; `ap_actor`/`ap_outbox` answer remote servers. That half exists.
- Nothing *sends*. `federation_queue` has one writer — `enqueue_activity`, itself orphaned — so nothing ever enqueues;
  `get_pending_queue`/`mark_queue_sent`/`mark_queue_failed` have nothing to drain.
- The two halves the sender would need are also missing: there is **no RSA signer** for outbound HTTP Signatures
  (`http_signature.rs` verifies and builds signing strings; `keys.rs` loads a public PEM and can generate a pair, but
  nothing signs), and there is **no reader for `ap_activities`** by id — the queue stores `activity_id` and the payload
  is never read back.

Building five admin doors over that would have been the most convincing possible fake: green tests over a queue that
cannot fill, marked sent by a drain that cannot sign, verified by a test that only ever talks to itself. The honest end
is a row with a spec — outbound AP delivery (signer, payload read, drain with the SSRF and backoff rules
`webhook_sender` already implements, and one real enqueue path: outbound Follow) — and it is recorded here so the next
pass starts from the finding instead of re-deriving it.

### The public door is a projection, not the row (library.rs, 2026-11-08)

`public_bookmarks_for` is the only query in `library` that takes no `account_id`, and its doc comment says
why: it serves other readers, and `is_public` is what makes that safe. What the comment does not say — and
what the door has to decide — is that the *row* is not the answer. A `Bookmark` carries the owner's account
id and their private note. A bookmark being public publishes neither, so `GET /works/{id}/bookmarks/public`
answers `{unit_id, position_permille, created_at}` and a count, and the bookmark's own id is left out too: a
stable row id is a capability nobody granted.

The companion read, `GET /library/items/{id}/updates`, is scoped by **account**, not by item — one reader's
check history is not another's, and the honest answer for an item you have never had checked is `null`
rather than an empty report.

One correction belongs here because it is the trap in this file's own neighbours: `update_checks.checked_at`
is **TEXT** in the PostgreSQL schema (`0009_library.sql`), unlike `download_grants.expires_at` and the
`export_jobs` timestamps, which are `timestamptz`. Wrapping it in `to_char(... AT TIME ZONE 'UTC', ...)` — the
fix that *was* right one module over — turned every item view into a 500. Cast-or-render is per column, and
the migration, not the previous module, is the authority.

### The catalogue was never recorded (imports.rs, 2026-11-08)

`upsert_source`'s doc comment, written when the function was, says: *"Called at startup for every registered
adapter, so the catalogue a reader sees cannot drift from the adapters the binary actually has."* Nothing
called it. The `sources` table was whatever a migration had seeded, so a deploy that added an adapter showed a
source nobody could pick, and one that removed an adapter left a row pointing at a parser that no longer
existed. `serve` calls `source_sync::sync_source_catalogue` now, straight after `AppState::new`, before the
listener binds.

Two things it deliberately does not do. It does not touch `enabled`, so a paused source stays paused across a
deploy, which is `upsert_source`'s other half. And it does not fail the boot: a source that cannot be written is
logged and skipped, because a stale catalogue row is a better trade than a server that will not start. The
adapter version recorded is the build id — a source's capabilities are its parser's, and the parser ships in
the binary, so "which build wrote this row" is the only version string that means anything.

**Two of the six have no live producer, and that is a finding, not a gap left open.** `list_credential_events`
and `pseuds_with_unconsented_credentials` read `source_credentials`, and every adapter in
`crates/scrapers/src/sites` declares `authentication: AuthKind::None` — no shipped source needs a login, so no
credential row can be created through the API at all. They have their doors and are exercised against rows the
store writes (`m70_13_imports_catalogue`), because the tables and the audit trail exist and a door is the only
way their SQL is ever decoded; the honest statement is that the doors are tested, not that they are reachable
live. The first authenticated source is what turns them on, and the same pass that adds it will find whatever
those two queries get wrong — as every other first caller in this ratchet has.

One limitation is worth naming because the test had to work around it: `credential_audit_events.occurred_at` is
second-resolution and the ordering tiebreak is a random UUID, so two events recorded inside the same second come
back in an arbitrary order. The test moves one row back a day to make "newest first" a real claim. A trail that
cannot order two events in the same second is not wrong, but it is not an audit trail either — a monotonic
sequence number belongs in that table the first time anyone needs to prove an order.

### A queue with no producer reads like a healthy instance (rating_integrity.rs, 2026-11-08)

`GET /admin/anomalies` answered `{count: n, items: []}`. It counted the queue and listed nothing, so an operator could
learn that seven anomalies existed and never learn which works they belonged to — and `insert_rating_anomaly_event`,
the one function that could have filled the queue, had no caller at all. "0 anomalies" from a detector that never ran
is indistinguishable from "0 anomalies" from a clean instance, which is the worst property a moderation queue can have.

The producer is `rating_watch::detect_bursts`, one rule: a work whose recent public ratings (six-hour window) came
from freshly created accounts (younger than seven days), at a threshold of five. Severity is the observation — eleven
fresh accounts is a stronger signal than five, and a constant would make the two identical to whoever reads the queue.
The detail carries the rule and its parameters, so a curator can argue with the numbers rather than the verdict.

Three refusals are deliberate, and each is written beside the code:

* **It never marks a work contested.** §33.2 gives that to a quorum of curators. A detector that can hide a work's
  aggregate by itself is a machine moderator, whatever its threshold.
* **It never duplicates an event.** A second run inside the same brigade finds the same works and records nothing, or
  severity would measure how often somebody pressed the button.
* **It is a button, not a timer.** There is no scheduler in this binary for it to ride; the scheduled form is a
  follow-up with a name, not a claim this file makes.

The mark itself got its doors (`POST`/`DELETE /admin/works/{id}/contested`, reason required) and its *point*: the
public `rating-summary` now refuses to publish a contested work's score and says `suppressed: "contested"` instead of
answering a null. §33.2 asks for both halves — never published as a public score, and an aggregate display that states
when a guard is suppressing a number — and a silent null would have lied in the second half: "nobody has rated this"
is a different and untrue claim about a brigaded work with five ratings.

One mistake worth keeping: the first version wrote `kind = 'fresh_account_burst'` and migration 0019's CHECK rejected
every insert. The schema is the authority on the vocabulary of a column; the rule's name belongs in `detail`.

**Deploy is batched, not skipped.** This pass adds four live admin routes and changes what `/admin/anomalies` answers,
but each thinkcentre build costs ~16 minutes and a named backup. The rows since M45-14 (coordinates, leakage, devices,
library reads, exports, imports, ratings) ship in one deploy with one `pg_dump` and one verification pass at the end of
Batch 1, rather than six builds nobody reads the output of.

### A job id was a capability (translation.rs, 2026-11-08)

Five store functions had no caller — `units_for_job`, `upsert_unit`, `record_publication`, `add_memory`,
`lookup_memory` — and reading them for the doors turned up a sixth problem that was not an orphan at all:
`GET /translations/{job_id}` answered **anybody who had the id**, with no ownership check anywhere. A
translation job carries the provider, the quote transaction and the language pair of somebody's work. It is
owner-scoped now and answers 404 to a stranger (ADR 0030), and the four new doors all go through one
`owned_job` helper so the refusal cannot drift between them.

The units are per paragraph, and that is the point of the read: one job can have three paragraphs approved and
seven pending, and a caller who only sees the job's state cannot tell. The upsert is keyed on `(job, unit,
paragraph)`, so recording the same paragraph twice updates it rather than appending a second translation of the
same sentence. A paragraph cannot be `translated` with no target text — the state is a claim about the text
beside it, and accepting the claim without the text puts a paragraph in the reviewed pile that nobody can
review.

Publication is gated on `approved` and the order is load-bearing: the job transitions first, the publication
row second. A publication row for a job that never moved is a claim about an event that did not happen.

Translation memory got its write and its match beside the existing list. Both are keyed by `memory_key`, one
SHA-256 of the **trimmed** source sentence computed in the app rather than accepted from the client: a client
that normalised differently — trimmed, lowercased, collapsed whitespace — would store a pair its own lookup
could never find, and the failure would read as "memory is not working" rather than "two hashings". The lookup
therefore takes the *sentence*, not the hash, so there is exactly one place that decides what a key is.

**A note on mutation discipline.** The unscoped-job mutant survived its first run, and the reason was not a
test gap: the predicate is duplicated — once inlined in the handler, once in the new `owned_job` helper — and
`replace(..., 1)` rewrote the helper while the test exercises the handler. Re-targeted at the handler it reddens
(200 vs 404). That trap is now recorded in `mutation-gate-discipline/references/mutation-survivor-triage.md`,
because it is the second time in this session a survivor was a misplaced edit rather than a blind test.

### Nobody could import a history (taste_import.rs, 2026-11-08)

The module with the longest header in the crate had no route and no mount: nothing outside `crates/db/src/taste_import.rs`
named it, so `import_signals` — the producer — had no caller, and a reader who arrived with five years of bookmarks could
not bring them. Unlike the earlier modules, the tests were not the problem: the store had five tested functions and no
surface at all.

Four doors later, §49.6–§49.8 are enforced at the door as well as in the store. The origin is **not a parameter** of
`import_signals`, so a door cannot mint an organic signal even by mistake; `origins` reports both the row counts and the
discounted weight because "how many" and "how much" are different questions with different answers — twenty imported
bookmarks and ten ratings given today are thirty rows and twenty-five units of evidence (§49.6 fixes the direction of
the discount and leaves the magnitude to us; the constant is now returned so the number is explainable without reading
the crate).

**A module decision, made rather than inherited.** These doors could have hung off `tasting`, which is the §49 section's
own module — but `tasting` is opt-in by design (`PerUser`, default off), and putting the import behind it would mean a
reader cannot bring their bookmarks unless they first answer questions about their taste. Importing a history you already
have is data portability, not calibration. So `taste_import` is its own module: `default_on: true`, `Toggle::Operator`,
`RouteClass::Write` (an import moves the reader's weights, so the limiter's class has to be the one that costs), and
`STABLE_ROWS` went 82 → 83 with the reason written above the constant.

**The mutant that survived for the right reason.** Dropping the credential-ownership check left the suite green, and that
was not a misplaced edit: the test named an all-zero credential id, so "not yours" and "does not exist" were the same
answer and the ownership filter was never the thing under test. Replaced with a sealed credential belonging to another
reader — plus the control that its owner *may* use it, which is what makes the refusal an ownership check rather than an
existence check — the mutant reddens (200 vs 422). Recorded because the lesson generalises: a negative test that cannot
distinguish its own two failure modes proves only that something failed.

Ratchet 136 -> 131.

### A table nothing could fill (taste_health.rs, 2026-11-08)

Five functions and no caller: the per-work vector's producer, its writer, its reader, and the two halves of the taste
probe. The damage was a table nothing could fill — `work_taste_vectors` was read by `get_work_vector` and written by
functions that nothing called, so every read answered `[]`. A cached vector that is never written reads exactly like a
work nobody has taste in common with, and no error anywhere distinguishes them.

Four doors, and the shape of the answer carries most of the decision. `[]` meant three different things and the door now
says which: `unavailable` (this instance has no admin taste profile, so no work has a vector — a configuration fact),
`computed` (there was a profile and no stored vector, so this call derived and stored one), `cached`. A bare `[]` would be
a claim about the work that nobody made.

**Serving a stale vector is a decision, written down rather than hidden.** A tag edit does not touch
`work_taste_vectors`; recomputing on every read makes a hot work pay for its popularity; and there is no hook in the tag
editor that would let the vector be invalidated where the tags change. So the read serves what is stored, `POST` on the
same path is the only refresh, and the test asserts the staleness (`cached` still answers the old numbers after prominence
30 → 90, then the refresh moves 0.8 → 1.0). The alternative — a silent recompute — would have looked correct and turned a
read into a write on every request.

**Operator-only, because the vector is a function of the operator's taste profile.** Publishing a work's vector would let
anybody with enough works invert the profile one coordinate at a time. A reader with no standing gets `404`, not `403`
(ADR 0030).

**The schema owned a word the header guessed wrong.** The probe header implies `shown/answered/dismissed`; migration
0054's CHECK says `positive | negative | neutral`, and `UNIQUE (account_id, work_id)` makes one answer per pair. The door
copies the migration's vocabulary, so a bad word is a `422` naming `engagement` rather than the CHECK surfacing as a `500`
— which is exactly what the mutant produced when the check was removed.

The ratchet moved six, not five: `taxonomy::tag_weights_for_work` was an orphan too and became reachable through the
vector producer. That is the transitive count doing its job — a door that calls a function that calls a query makes the
query reachable, and the number should say so.

Ratchet 131 -> 125.

### A promise the binary never asked it to keep (identity.rs, 2026-11-08)

`grant_guardian_authorization` carries the finding in its own header: before it, "a minor could never participate, whatever
the operator set up", because `guardian_authorizations` existed and nothing wrote to it. The function was written, the
documented reason was written, and no call site existed — so the instance advertised a guardian workflow it could not
perform. Four doors now, and each keeps one rule:

* **An age decision takes a reason.** It is the change with the least audit trail and the most consequence: a restricted
  account cannot participate, and the modlog's contract says every entry carries a reason. The `reason` field is
  `Option<String>` *in the type* so that its absence is this handler's 422 naming `reason`, not the JSON extractor's
  generic "Unprocessable Entity." — the field matters, so the error says which.
* **The guardian grant does both halves.** It records the authorisation *and* moves the state; the door reads the row back
  rather than assuming the second half, because a recorded authorisation that leaves the account stuck is a row nobody
  reads.
* **A fold points at a live account, and a tombstone is terminal.** Self-fold, a nonexistent target, a folded target and
  an already-folded source are all refused. The last one was found by the test, not by reading: the first version happily
  rewrote a tombstone's `folded_into`, which would silently re-attribute every row the first fold re-pointed.
* **Facts are operator-only, and `404` for a missing account.** The store returns `Option` precisely so the caller can
  tell "no such account" from "an account with nothing yet"; collapsing that into an empty object would have thrown the
  distinction away at the last step.

`count_excluding_system_accounts` got **no door**. Its own header said both of its callers were tests; inventing an
operator metric to lower a counter is bad faith, so it moved to `crates/test-support` and the store stopped exporting it.
Deleting a helper whose only callers are tests is the other legitimate end of this ratchet, and the one that needs no
surface invented for it.

**A live leak in the same module.** `GET /accounts/{id}/trust` took `RequireSession(_user)` — the underscore says nobody
looked — and answered any signed-in reader with another account's trust history: the modlog's record of one person, every
level change, why, and who made it. It is the account's own or an operator's now, `404` otherwise (ADR 0030).

Ratchet 125 -> 120.

### The exchange could only be switched on with SQL (exchange.rs, 2026-11-08)

`set_enabled` and `set_instance_id` had no caller. §11.17 makes the exchange a deliberately opt-in surface — "no request
shape turns on a server that did not enable it" — and the switch that turns it on was reachable only by an operator with a
database client. `list_latent_demand` had no caller either, so the instance accumulated demand it could not show anybody.

Three doors, merged into the existing `exchange` mount:

* **`GET|POST /admin/exchange/settings`** — the switch and the instance's own name in one place. `configured: false` is a
  different answer from `enabled: false`: a row that was never written is not a decision, and reporting the two the same
  way would make a fresh instance look like one an operator had deliberately closed. The first write creates the row with
  §11.17's 1000/hour default, so flipping the switch does not require knowing the second knob. The instance id is
  validated once (`[A-Za-z0-9._:-]`, ≤64 chars) because it travels as `source_instance` on every signal and is the dedup
  key for latent demand, and every accepted change writes a modlog row with a required reason.
* **`GET /admin/exchange/latent-demand`** — `signal_count` is how many times *this instance* has signalled, named that way
  and with no submitter or holder field anywhere, because §16.16.1 says re-signalling adds no weight and a reader count is
  not computable from the data the instance holds.
* **`GET /admin/exchange/works/{id}/provenance`** — `count_provenance_signals`, the function whose name exists to keep
  "provenance" from reading as "headcount". The response carries the number *and* a note saying it is not a count of
  readers or holders, and the door is `Operator` so the number does not appear on a public page where nobody could correct
  the misreading.

**The fifth orphan was a twin.** `exchange::list_review_queue` ran the same query as `taxonomy::list_unverified_nodes`,
with a narrower projection whose two extra columns (`curated_by`, `curated_at`) are always NULL — the query only selects
`review_status = 'unverified'`, and a curated row is not unverified. It was deleted, with the reason written where it
stood, and `GET /exchange/review-queue` keeps the taxonomy function. Deleting a redundant twin is the other end of this
ratchet, and the one that needs no surface invented for it.

**A guard mutant that survived for a reason worth keeping.** Removing the operator check from the latent-demand door left
the suite green: the test asserted the guard on two of the four doors and never asked the third. One reader assertion
later it reddens (200 vs 404). A guard mutant survives exactly the doors no test names, so the mutant is a map of the test's
blind spots as much as the code's.

Ratchet 120 -> 115.

### The mention that was never recorded (community.rs, 2026-11-08)

Five orphans, four of them missing surfaces and one a missing behaviour — and the behaviour was the interesting one.
`record_mentions` checked blocks and never checked mutes, and its own doc said *"Mute is checked separately in the
notification layer."* Nothing checked it anywhere. A reader who muted somebody still received mention notifications from
them: the one notification they had explicitly asked the instance not to send. `is_muted` gates the producer now, and the
comment says what it used to claim.

Making that true turned up three more defects, none of them in the code being added:

* **`post_comment` passed a pseud id where an account id belongs.** `mention_events.mentioned_by` references
  `accounts(id)`, so every comment mention failed its foreign key — and the insert was wrapped in `let _ =`, so the failure
  was silent in both directions: nothing recorded, nothing logged. Comment mentions had never worked. The same parameter is
  compared against each mentioned pseud's account to skip self-mentions, so that check could never fire either.
* **The mention parser refused hyphens.** `com-a` is a legal handle; the parser's character check was alphanumeric-or-
  underscore, so a hyphenated handle was permanently unmentionable and the symptom would read as "mentions don't work for
  that person". The trailing-punctuation trim keeps hyphens now.
* **`comment_by_id` selected eight columns and decoded eleven.** Its first caller — the permalink door — got a 500. The
  anchor columns are selected now, with the rule beside them: a `pub async fn` nobody calls cannot fail to decode, which
  is why this was invisible.

The four doors: `GET /comments/{id}` (anonymous; a deleted comment is the soft delete's tombstone, not a 404 — the comment
existed, and "no such comment" would rewrite history for everyone who replied), `GET /presence/{account}` (one person's
presence under the stream's **own** rule via `presence_visible_to`: muted or blocked is absent, and a person who turned
presence off is absent to others while their own read still answers `enabled: false` with no last-seen — an opt-out that
still reports when you were last here is not an opt-out), `member_role` in `GET /groups/{id}` as `viewer_role` (`null` is
"not a member", not "no access"), and `category_exists` at the **writer**: `create_topic_with_mode` refuses a category that
does not exist, which is what its own doc claimed and only one route enforced.

Ratchet 115 -> 110.

### A fresh instance had no categories (category_governance.rs, 2026-11-08)

This module was *partly* wired — `routes/directory.rs` imports it as `cg_db` and runs proposals, votes, vetoes, changelog
and the freeze — so the orphans were the edges of a working surface, and the edges were where the instance was broken in
ways nothing logged: `seed_categories` and `upsert_category` had no caller, so a fresh instance had **no categories at
all**. The directory's category filter offered nothing, and `create_proposal` answered "category not found" for every
§39.2 slug an operator would try. An empty list is a valid list; that is why it survived.

* **The seed runs at boot** through `crate::directory_seed::seed_directory_categories`, a named function so the count is
  what gets logged. It is idempotent *by construction* — `upsert_category` is `ON CONFLICT(slug) DO NOTHING`, which is
  §45.1's rule for a config upsert: it adds slugs the instance is missing and never resurrects a category the community
  removed. The test proves the second half by deprecating one and re-seeding. A failed seed logs and does not refuse the
  boot: an empty category list is a visible misconfiguration, a refused start is an outage.
* **The queue expires on read.** `expire_proposal` is a per-row sweep and the binary has no scheduler, so the read is the
  sweep — without it, `open` means "nobody has looked" rather than "still decidable". The route returns the number expired
  rather than implying it, and a second read finds nothing to do because the status already moved.
* **`count_active_categories` is the ceiling, visible and enforced.** `governance_state` reports `active_categories` and
  `at_ceiling`, and a create proposal is refused at **propose** time when the ceiling is reached: a proposal that cannot
  succeed spends quorum attention, and the queue is more expensive than the refusal.

**And a door that lied.** `POST /directory/categories/governance/max` ignored its body (`let _ = body.max;`) and answered
`200` with the unchanged configured value — a client reading that 200 could only conclude the change applied. The ceiling
is an instance configuration, so the honest answer is a `422` naming `directory.governance.max_active_categories` and
saying it is changed there; sending the value it already has is still a `200` the client can trust.

The boot wiring is asserted against `server.rs` source, the way `route_inventory` checks the route table against the
routers: `serve` binds a listener and no test may boot one, so the source is the only place the call can be observed.

Ratchet 110 -> 105.

### A table written and never read (story_identity.rs, 2026-11-08)

The grouping table was **written by the claim path and read by nobody**. `members_of` and `member_counts` had no caller, so
a work could accumulate external locations that no page ever listed, and "this work has one copy" and "this work has four"
were indistinguishable from outside — the import path did the expensive part (deciding two URLs are the same story) and
then kept the conclusion to itself.

`GET /works/{id}/preservation` now answers `editions` and `copies`. The list is **local copy first**, then each external
location: the copy a reader can open, then the ones they cannot. A work nobody has crossposted answers an empty list and
`0/0/0` rather than a 404 — "not crossposted" is a state, not a missing page, and the identity is created by the first
claim. `local` is computed as `total - external` from the same row set, so the three numbers cannot contradict each other.

**`merge_table_is_empty` was deleted rather than wired.** Its only caller was a test assertion, and its own doc said so:
"Phase E will have to delete this function or invert it, which is the point." The test now counts
`identity_merge_proposals` and `identity_merge_history` directly and carries the reasoning: a helper that always answered
`true` would have made the claim unfalsifiable, and that claim is the one standing between "the merge tables exist" and
"the merge tables are used".

Ratchet 105 -> 102.

### A monitor that was really a button (note_links.rs, 2026-11-08)

A note link was checked **only when its author pressed "re-check" on the work that carried it**. `find_needing_check` — the
queue — had no caller, so a dead link on an abandoned work was never noticed, and the `dead` count on that work's page was
a number that could not change on its own. The page reported a monitor; the instance had a button.

* **`POST /admin/notes/links/sweep`** (Operator, trust 5, `limit` clamped to 1..50) drains the queue's head through the
  same `check_note_link` the author button uses — one fetch path, one SSRF guard. A refused URL is recorded dead by the
  check itself, so one missing link never fails the sweep; a storage error is counted as `failed` and logged.
* **The queue has no "due" predicate**, so a sweep re-checks never-checked links first and then the oldest, and the handler
  says so. There is no scheduler in this binary (§37 puts recurring work on the operator): this is a button, and the
  scheduled form is named as the follow-up rather than pretended.
* **`GET /works/{id}/note-links?include=superseded`** answers the monitor's question instead of the reader's — every row
  the checker still holds, with `superseded` computed from the live set rather than guessed from `revision_id`. The plain
  read is unchanged: a corrected note still reports only what the work says now.
* **`count_dead_for_work` was deleted.** The route counts the dead rows of the same list it returns; a second source for
  that number could disagree with the list shown beside it.

**The fixture was the first thing the mutants found.** The first draft had a single link, so after the correction the live
set was empty — and a `superseded` flag that was always `true` passed, because there was nothing to compare it against. The
test now uses two chapters, one corrected and one kept, so the flag is exercised on a mixed set. Two further mutant attempts
never compiled and were re-targeted, not counted.

Ratchet 102 -> 99.

### A queue that could be neither filled nor emptied (engagement.rs, 2026-11-08)

`taste_notification_queue` had no producer and no drainer. Three store functions, a passing store test, and no caller in the
application — the table could be neither filled nor emptied, and migration 0055 states the intent in one sentence: *"When a
work is published, aligned users are queued for notification. The worker drains this table."* Neither half existed.

Both halves now do:

* **`queue_aligned_readers`** runs from `publish_work` (not on a replay: the queue is keyed by `(work, account)` so it would
  be a no-op, and a retried publish should not pay for a scan). It gates on the module switch itself, because the producer
  lives inside a core route where a module mount cannot gate it. It queues readers inside the operator's taste band, never
  the author for their own work, and skips a reader already holding `PENDING_CAP` unsent rows — which is what
  `pending_taste_notification_count` existed for.
* **"Aligned" is the instance's own notion**: distance to the operator's taste centroid, not a per-work relevance score.
  darkhold has no accumulated reader-by-work evidence (§49.9 refuses to invent it), and a notification that arrives for a
  reason the reader cannot name is worse than none.
* **`POST /admin/taste-notifications/drain`** delivers through `notifications::notify`, so the reader's own settings decide
  whether a row means anything. `delivered` counts rows *written* and the response says so.
* **`GET /me/taste-notifications`** answers the reader's own queue depth — a count, not the rows: a queued row is a plan,
  and showing it would deliver the notification without marking it sent.

**Three defects, none in the code being added.**

1. **`drain_taste_notification_queue` delivered twice.** It marked rows `sent_at = now` and then selected `WHERE sent_at =
   now` — and `now_rfc3339()` has **second** resolution, so a second drain in the same second re-read the first drain's
   rows. A queue whose entire purpose is exactly-once delivery handed everything to the worker twice. Rewritten as a single
   `UPDATE … RETURNING`. The test's second drain (same second) is the regression test, and it is what caught this.
2. **`list_users_by_centroid_distance` was undecodable on PostgreSQL** — `accounts.id` is a UUID column and the tuple
   decodes a `String`. Invisible while nothing called it.
3. **…and it failed on any account with no distance.** `taste_centroid_distance::float8` with no `IS NOT NULL`, on a column
   whose default is NULL on PostgreSQL (SQLite's is `0.0`): one unranked account and the producer queued nobody, silently.
   A reader with no distance is unranked, not unimaginably far.

**The first version of the test passed a mutant.** On the off instance the reader was also the publisher — and the author is
never queued for their own work, so author exclusion masked the module gate and removing the gate changed nothing. The
fixture now gives that instance a bystander reader and a separate author. That is the same lesson as the single-link note
test: a fixture whose two rules agree with each other cannot tell them apart.

Module row `taste_notifications`: `default_on: false` (it writes to people who did not ask), `Beta`, operator toggle, spec
cited as migration 0055 rather than the store's own `§9.9`, which points at Legal posture.

Ratchet 99 -> 95.

### Demand nobody recorded, and a guard that broke PostgreSQL (moderation.rs, 2026-11-08)

Three §35.5 signals had no producer and no reader.

* **A zero-result search is demand, not an error.** The forum search route now records a free-text query that finds nothing,
  keyed by the sha256 of the lowercased text so "Ten Ways" and "ten ways" are one miss with a count. A narrowing-only search
  (`category:` with no `q`) records nothing: that is a taxonomy question. `GET /supply/unmet-searches` (operator) is the read
  — a drought is a topic nobody writes about, an unmet search is a reader who went looking and came back empty.
* **`record_topic_activity`** runs on every reply, one row per `(topic, date)`: "last post at" cannot express forty replies
  today versus forty across forty days.
* **The first vote is the one an author remembers**, and it is notified once. Three gates, and only the last one is the
  enforcement: `existing.is_none()` (a *change* is not a new vote — the count alone cannot tell them apart), the count being
  exactly one, and the stored row. The store now **returns whether it inserted** (`rows_affected == 1` on both engines) and
  the caller notifies only on a genuine insert: a count that returns to one through a different reader after a retraction
  would otherwise send a second notice, and two casters racing on the same first vote would both send one.

**The defect this pass found was mine.** `category_exists`, the guard added in the community pass, used `SELECT 1 …` decoded
into an `i64` — `1` is **INT4** on PostgreSQL and sqlx will not decode it as INT8, so **every topic creation was a 500 on
PostgreSQL** while SQLite's untyped integers decoded happily. Reproduced with the change stashed (clean master fails the same
way) so it is not mistaken for this pass's edit, then fixed as `SELECT EXISTS(...)` decoded as `bool`.

The lesson is in the shape of the miss: that pass ran its own targeted test on both engines and the file it touched — but the
guard it added runs when a *topic* is created, and no test in that pass created one. A guard is only tested by the suites that
exercise the rows it guards, on both engines.

Ratchet 95 -> 92.

### A lease nobody renewed (jobs.rs, 2026-11-08)

The queue had producers and a worker. What it did not have was a caller for three of its own functions, and the first was
the one that mattered.

**`heartbeat`.** A claim is a *lease* — `options.lease`, 120 seconds by default — and `maintenance_pass` returns expired
leases to the queue. Nothing renewed it, so a job that outlived its claim was requeued by the sweep and handed to a
**second worker while the first was still writing it**. The same import, twice, over the same rows; the only trace was
"the lease was lost before the job could complete" in a log nobody reads on a success path. `run_once` now spawns a keeper
that renews at a third of the lease for exactly as long as the handler runs, and stops when the handler returns or when
the renewal says the lease is somebody else's.

Two facts about the test are worth keeping, because the first version of it proved nothing:

* **Only a queued job is claimable.** A second worker calling `claim_next` alone can never take a leased job; the sweep is
  what turns `leased` into `queued`. A thief that skips the sweep is a test of nothing — and it passed with renewal
  disabled, which is how it was caught.
* **The maintenance pass enqueues work of its own** (the syndication job). "The queue is empty afterwards" was the wrong
  assertion; the second worker legitimately leased that job. The assertion is scoped to the probe's id.

And a limit: lease stamps are written at **second resolution**, so a sub-second lease is expired the moment it is written
and no renewal interval can save it. The test uses a two-second lease against nearly five seconds of work.

**`counts_by_state`** is now `GET /admin/jobs/summary`, built from a new `ALL_STATES` in the domain rather than from
`GROUP BY state`: a dashboard assembled from the rows can only show states something happens to be in, which makes "no
failures today" and "nobody asked" the same line item. Every state is listed, empty or not.

**`attempts_for`** is `GET /jobs/{id}/attempts`. The owner sees their job's history — four attempts and the error from
each, not just the last one — and the operator additionally sees which worker ran it. A stranger gets the `404` an absent
job gets: a job id is not a capability.

Ratchet 92 -> 89.

### Three pieces of a cap, and none of them in the path (media_resilience.rs, 2026-11-08)

Four functions in this module had no caller, and the first was the whole anti-gaming rule for curator credits.

* **`sum_curator_rewards_today`.** The config declared `media_resilience.daily_credits_cap` (500), `caps::within_daily_cap`
  was a tested predicate, and this was the query that answers it — and the award path read **none of the three**. It
  hard-coded `15` and paid on every call, so the same mirror added a hundred times paid a hundred times. The link is still
  kept once the cap is reached (the correction is what the feature is for); the credits are paid up to the cap, partially
  when the remainder is smaller than the bounty, and the response says `credits_awarded` and `credits_capped` so a curator
  who is not being paid can see that instead of guessing.
* **`find_references_below_threshold`** is the under-mirrored queue the bounties are paid against. Without a caller, a
  curator could only find such a reference by stumbling into it. Now `GET /admin/media/references/at-risk?threshold=`,
  defaulting to the spec's own three.
* **`find_match_proposal`** is what happened to one proposal, which is the only record of a **refused** merge: the list
  route shows what is pending, and a rejected proposal leaves no other trace. Now `GET /media/match-proposals/{id}`.
* **`find_work_media_references`** is deleted, not wired: a narrower twin of `list_work_media_references`, which the
  `GET /works/{id}/media` route already calls and which answers the same rows plus each link's health and best mirror.

Both new PostgreSQL arms had never executed, and both were wrong — two of the three defects this pass fixed were in code
nobody had run:

| symptom | cause | fix |
|---|---|---|
| 500 on the queue | `m.id` is UUID, three timestamps are TIMESTAMPTZ, `content_notes` is JSONB; the row type is all `String` | `m.id::text`, `to_char(... AT TIME ZONE 'UTC', ...)`, `content_notes::text` |
| 500 on the queue, after the casts | a `to_char(...)` column is named `to_char`; `FromRow` maps by name | alias every expression (`AS first_seen_at`) |
| 500 on the proposal detail | `resolved_by` is UUID | `resolved_by::text` |
| `function pg_catalog.timezone(unknown, text) does not exist` | I copied the `to_char` treatment from `media_references` onto `media_match_proposals`, whose timestamps are **TEXT** | read the column, not the neighbour: plain `created_at` |

The last one is the sharpest: two tables in one file, four hundred lines apart, disagree about the type of `created_at`.

Ratchet 89 -> 85.

### A class nobody could read, from a table nothing filled (canon_agnostic.rs, 2026-11-08)

Three functions and a producer, and between them the whole of §50.2's class was invisible.

`stored_canon` had no reader, so a work's canon class — already computed, already stored, already tested — never reached a
page. `is_canon_agnostic` and `is_canon_dependent` are §50.2's eligibility question and its mirror; the module's own docs
insist a caller must not pattern-match `CanonVerdict` for this, because eligibility is a rule with one implementation.

And `measure_work_and_store` had no caller outside its tests, so on a fresh instance the table stayed empty, the listing
stayed empty, and "no canon-agnostic works" read as a fact about the corpus rather than a missing scheduler. It now runs
when a work is published — the moment its text stops changing — best-effort, like the taste queue beside it in `publish`.

What a reader gets:

* `GET /works/{id}/canon` — four states, not two: `canon_agnostic`, `canon_dependent`, `unclassified` (with the reason),
  `unmeasured`. A boolean would collapse three of them, and "we did not look" must not read as "we looked and it was fine".
  `eligible` and `assumes_canon` are separate because a badge and a filter are separate client decisions, and both are
  `false` when nothing was measured — the conservative direction §50.2 asks for.
* `GET /discover/canon-blind` — the labelled, bounded shelf: twelve rows, newest class first, every row carrying
  "you may not know this fandom", and **nothing ranked**, because a shelf with a score is a feed and §50.2 forbids these
  works outranking a recognised match. `state` distinguishes `unmeasured` from `ok`, so an empty shelf is not mistaken for
  an empty corpus.

**The missing half, named rather than approximated:** §50.2's rule is that a work is eligible when its fandom is unknown *to
the reader*. This pass ships the half that is a property of the work (is it readable without its canon) and the shelf; the
reader half needs each work's fandom tags intersected with that reader's taste weights, and it belongs in the taste module
rather than here — pooling per-reader weights into a per-work listing is the cross-reader blending §49.9 refuses. Until it
lands, the shelf is honest about what it is: canon-agnostic works, labelled, not "works you have never weighed".

Ratchet 85 -> 82.

### Standing could only be given with psql (governance.rs, 2026-11-08)

Three store functions and no door, and between them they are how standing is *given* on this instance:

* `set_trust` — the ladder is configured, `trust_for` reads it, `account_facts` shows it, the modlog has an action for it, and
  the only way to move an account up or down was `UPDATE trust_levels`. A moderation power with no door is a power only the
  person with the database password has.
* `apply_spam_demotion` — §14.4's rule (three upheld spam reports in ninety days drops one level, never below 2). The domain
  predicate was tested, the applier was written, nothing ran it: a spam wave left every level exactly where it was.
* `grant_operator_role` — the estate reads `role:operator` claims; granting one was SQL.

The doors are `POST /admin/accounts/{id}/trust`, `…/spam-demotion`, `…/operator-role` — operator only (ADR 0003), each with a
required reason and a modlog row, and each checked against the vocabulary that will read it: a level the configured ladder
does not name is a typo (storing it makes `name_of` answer `null` for an account whose level *is* set), and a role no reader
of the claim recognises is worse than a refusal because it looks like standing and grants nothing.

Two decisions inside the demotion door:

* **`Ok(None)` is two different answers.** Not enough confirmed reports is a state an operator can act on; already at the
  floor is finished. The door asks both questions itself and names which one it answered (`not_enough_confirmed_reports` /
  `at_floor` / `demoted`) instead of reporting "no change".
* **The basis column is JSONB on PostgreSQL and TEXT on SQLite.** `set_trust(…, "spam")` is valid on SQLite and
  `invalid input syntax for type json` on PostgreSQL, so the door absorbs that difference once — the reason travels as
  `{"reason": …, "operator": …}` — rather than each caller rediscovering it.

Ratchet 82 -> 79.

### A history the table recorded and no page showed (content.rs, 2026-11-08)

`publication_events` is written on every publish, update and revision restore. It was read by one listing's sort key and
nowhere else — so the two facts a reader wants about a work, *when was this published* and *when did it last change*, were
recorded and never shown: a work finished in 2019 and one finished yesterday rendered identically on their own records.

`GET /works/{id}/history` answers both maxima (`work_freshness`) and the evidence behind them (`publication_events`), so
"updated 3 days ago" can be clicked into and a restored revision is distinguishable from a fresh edit. Visibility is
`read_work`'s decision, re-used: the same `reading_decision_publishing_metadata` call, so a stranger asking about a draft
gets what reading it gives them, and the door is not an oracle for the existence of drafts.

Two defects found on the way:

* **`ORDER BY occurred_at DESC` with no second key.** `occurred_at` is second-resolution, so a publish and an edit in the same
  second tie — and a tie with no second key makes the list shuffle between two identical requests. The store now orders by
  `occurred_at DESC, id DESC`: `id` is not chronological, but it is stable, and "which of these two came first" stays a
  question the data cannot answer rather than one the query answers wrongly. (The first version of the test failed exactly
  there, on PostgreSQL, with publish-first in the same second.)
* **`wipe_content` is deleted.** It `DELETE`s every work, unit, revision and publication event in the database, its doc comment
  said "development seed reset only", nothing called it — not a route, not a test, not a script, not the spec — and tests get
  fresh databases, so it bought nothing while remaining one import away from being called on a live instance. Deleting a
  destructive primitive that no specification asks for is cheaper than guarding it.

**A stale test in another crate**: `crates/db/tests/note_links.rs` still called `count_dead_for_work`, deleted three passes
ago in the media/note-links pass. The `darkhold-app` suite never compiles `darkhold-db`'s tests, so nothing said so. The
assertion is now derived from `list_for_live_revisions` — the same rows the monitor reads — and the whole `darkhold-db` test
crate compiles again (`cargo test -p darkhold-db --no-run`).

Ratchet 79 -> 76.

### A score nobody shows, and a fixture the binary carried (directory.rs, 2026-11-08)

`entry_score` read `directory_entries.score` — the **stored, undecayed** column. Every surface that shows a score shows the
decayed one: the list ranks by it, and the entry page computes `decayed_score` explicitly so that "a detail page that showed
a different number from the list it was reached from" cannot happen. So `entry_score` was not merely unused, it was a
narrower twin of a rule the product deliberately does not follow; it is deleted, and the comment in `cast_vote` that named it
now describes the column instead of the function.

The other two orphans were `set_account_trust_for_tests` and `votes_for_tests`: `#[doc(hidden)]` but compiled into the binary,
carried by the product so that app tests could arrange a trust level and read vote rows. A writer that sets a trust level
with `basis = 'tests'` belongs with the other test helpers, not on a surface anything can call; both moved to
`crates/test-support` (`set_account_trust`, `directory_votes`), and the five call sites moved with them.

**A hypothesis the mutants killed, recorded so the next reader does not repeat it.** The old `set_account_trust_for_tests`
had identical SQL strings on both arms — `?` placeholders on the PostgreSQL arm too — which reads as a copy-paste bug that
would panic the moment a milestone ran on PostgreSQL. It is not: `Database::sql` rewrites `?` to `$n` on the PostgreSQL arm,
and a mutant that restored the `?` string passed on PostgreSQL. The duplicated strings were redundant, not wrong. Only the
vote query genuinely needs two strings (`vote_value` needs a cast).

**And a note on what these gates assert:** seeding level 7 instead of 6 leaves `milestone_39` green — its trust checks are
threshold-shaped ("this level may do X"), so over-granting is invisible while under-granting is not (a mutant that lowered
the level reddened with `ACCESS_DENIED`). That is a property of the gate's intent, not a defect, but it means the fixture is
proved by *denial*, not by *exactness*.

Ratchet 76 -> 73.

### Every transaction recorded its fee, to a table nothing wrote (monetization.rs, 2026-11-08)

§20.9.5 is an obligation, not a feature description: *every* monetary transaction records the processor's fee, and the range
actually paid over ninety days is published on `/api/v1/meta` so §20.10's solidarity arithmetic is transparent about what
processing costs. `record_payment_event` wrote the table that disclosure is made from and had no caller; `payment_events` had
no reader at all. The table was therefore empty forever, and the only way to publish a fee range would have been to invent
one.

Three things had to be fixed before the write could honestly be made:

* **The table could not express "already recorded".** `payment_events` had no idempotency key — which is *why* no money path
  wrote to it: a retried tip posts one earnings row (that ledger has a key) and would have posted two payment events, inflating
  the published range. Migration 0200 adds `event_key` with a unique index; `record_payment_event` takes it, inserts
  `ON CONFLICT(event_key) DO NOTHING`, and answers `(id, recorded)` — when the row already existed it returns *that* row's id,
  because a caller holding the UUID we minted and did not use holds a reference to nothing.
* **Only money paths write here.** A purchase, a money tip and a payout move money. A credit tip moves credits between
  buckets and records nothing: a credit is not a payment, and a fee booked against one is a fee nobody paid.
* **Zero is stated, not implied.** No processor adapter exists in this binary, so every recorded fee is zero. `/meta` publishes
  `processor_wired: false` beside the range so `0` cannot be read as "processing is free": it says nothing has been charged by
  a processor and none is wired to charge one. `processor_fee_min_/max_minor` are `null` until a payment exists — "we have
  never paid a processor" is not the same claim as "processing cost us nothing", and only the second is a zero.

**What a client sees:** `payments: { window_days: 90, processor_fee_min_minor, processor_fee_max_minor, processor_wired }`.
The window is the specification's and is deliberately not configuration — an operator shortening it would be shortening a
transparency commitment.

**A mutant survived the first version of the test**, and the fixture was wrong, not the code: it asserted the published fee
range only, so recording the *wrong gross* (0 instead of the price) changed nothing it looked at. The test now reads the
event row itself — kind, gross, net — and the mutant reddens.

Live surface changed (`/meta` gained a field; migration 0200 is pending) — this ships in the batched thinkcentre deploy with
the rest of Batch 1, and the backup must be taken with the migration applied, not before it.

Ratchet 73 -> 72.

### A settings page with no setting, and a rule with no reader (monetization.rs, 2026-11-08)

§20.9.1's eligibility setting — `original`, `any-with-assertion`, `disabled` — was three constants, a parser, and a tested
predicate (`assertion_required`) with no caller. There was no configuration key for it either, so ADR 0017's "the eligibility
setting is an instance setting" was not true of the binary: an instance could price and sell *any* work, and
`any-with-assertion` promised a rights assertion nothing ever collected or checked. `record_assertion` and `get_assertion`
were the missing halves — a table with a writer nobody called and a reader nobody called.

What exists now:

* `[monetization] eligibility` in the configuration, defaulting to the ADR's `original`, and **validated at boot**: an operator
  who writes `origional` asked for something this binary does not implement, and that failure belongs to them at startup, not
  to an author at pricing time.
* `POST /works/{id}/assertion` — the author records what they claim (`original` or `rights-held`). Owner only: a rights claim
  by somebody else is a statement about the author, not an assertion by them. The **policy version is the server's**
  (`{VERSION}+monetization`, the same shape as the age assessment's), because a version the client chooses is a version nobody
  can audit and "re-assert on a policy change" needs something the instance controls.
* `GET /works/{id}/assertion` — public, `asserted: false` rather than 404 when there is none — the question is about a work
  that exists, and the person who most needs the answer is the would-be buyer.
* The gates in `set_pricing` **and** `purchase`, sharing one helper so "what counts as an assertion here" cannot drift between
  the price and the sale.

**The modes differ in which assertion suffices**, and that is the load-bearing part: on `original`, a `rights-held` claim is
stored, readable, and *not enough*; on `any-with-assertion` it is. A single "has an assertion" check would have let a fanwork
be priced on the strictest instance, which is the one thing that mode exists to prevent. `disabled` is refused on the field
that selects what is sold (`model`), not on `assertion`: it is not "no assertion", it is no money, and credit tips keep working
because a credit is not a payment.

**A price set under one setting does not inherit permission from it.** The sale asks the same two questions again — that is why
the purchase gate exists at all, and the test proves it by pricing a fanwork on an `any-with-assertion` instance and buying it
under `original`.

**Behaviour change, deliberately:** pricing a work now requires an assertion. `milestone_21`'s five pricing fixtures and the
payment-events fixture were updated to assert first, which is the workflow the spec describes and which nothing enforced until
now.

Ratchet 72 -> 70.

### A setting with no door and a feedback list with no viewer (dnf.rs, 2026-11-08)

`works.allow_dnf_feedback` has been `BOOLEAN NOT NULL DEFAULT FALSE` since migration 0073 and it was read by exactly one
function — `work_allows_dnf_feedback` — which nothing called. Two consequences, both invisible from the product:

* **No author could turn it on.** Nothing in the tree wrote the column: not the work patch, not a settings route, not the
  operator console. The setting existed in the schema and could only be changed with `psql`.
* **The rule it names was never applied.** A reader could publish "I stopped reading this" about any work, including one
  whose author had somehow said no. A setting no writer sets and no reader reads is a comment.

`list_dnf_for_work` was the other half: the store could list a work's feedback and no surface showed it, so an author whose
work was being dropped had the numbers (`GET /works/{id}/dnf/reasons`, wired) and none of the sentences.

What exists now:

* `allow_dnf_feedback` on the work patch (`PATCH /works/{id}`), and therefore on the authoring surface that already edits
  visibility, rating and body audience.
* The gate lives **in the producer**: a public DNF on a work that has not opted in is `422` naming `is_public`. A private
  DNF is always allowed, because it is the reader's own reading history — refusing it would make their library lie about
  what they read on every work whose author never touched a setting.
* `GET /works/{id}/dnf/feedback` — the author's view: the published sentences, plus `accepts_feedback` so "can more arrive"
  and "what arrived" cannot disagree. Non-contributors get `404`, exactly as they would for a work they cannot read.

**Decisions recorded because they were not defaults:**

* **A private DNF stays private, including from the author.** `is_public` defaults to false ("private by default", M45-21)
  and the reader who leaves it false has said "this is for me". `list_dnf_for_work` therefore lost its `include_private`
  parameter instead of gaining a caller with `false`: a parameter that must always be false is one somebody eventually
  passes true. The author learns the *shape* of what is not published from `aggregate_dnf_counts`, which publishes counts
  and never text.
* **The opt-in gates publication, not recording** — see above.
* **Turning the setting off does not retract what was published.** A statement made while the door was open is evidence,
  not a permission that can be revoked retroactively; step 7 of the test proves the row survives the setting that allowed
  it.

**One defect found by the second engine, again:** the patch bound `i64` to `allow_dnf_feedback` on both arms, copying
`show_public_ratings`' shape — but 0073 spells the column `BOOLEAN` on PostgreSQL and `INTEGER` on SQLite (it says so in
the migration's own comment). SQLite accepted the integer; PostgreSQL answered `COALESCE types boolean and bigint cannot be
matched`, a 500 on the first PATCH. Each arm now binds what its column is.

**A mutant that proved nothing, caught:** the first version of the test let step 2's private row and step 3's public row be
the *same pseud and work*, so the upsert replaced one with the other — and the mutant that deletes `is_public = TRUE` from
the list survived, because a list with no filter still returned exactly one row. A second reader holding a private record
alongside the first reader's public one is what makes the exclusion testable.

Ratchet 70 -> 68.

### The shelf behind the banner, and the save that never worked on PostgreSQL (continue_reading.rs, 2026-11-08)

`continue_reading` answers "where did I stop, in the one thing I am reading" and it is wired to the banner. Beside it live two
queries no page asked for:

* `unfinished_works` — everything the reader started and has not finished. Its own doc says "this is for the library and for
  tests"; the library never grew it.
* `dnf_works_for` — the works the reader dropped, with the reason, "so the caller can decide". With no caller, no decision
  was ever made.

A DNF row alone is not a shelf: a reader who dropped ten works wants ten titles and their progress, and a query returning
`(work_id, reason)` pairs leaves the client to fetch every work. `GET /library/unfinished` is one list of **started** works
annotated with the reason the reader stopped — the judgement the store deliberately left out, made once, in the caller.

**`is_public` is about other readers, not about this one.** `dnf_works_for` filtered `is_public = 1`, reasoning that a private
mark "expresses a preference about it that is not 'stop recommending it'". But `GET /me/dnf` — wired, and older — lists
**every** DNF row the reader owns, private ones included. The two surfaces disagreed about what the reader had done. The
column's own name settles it: `is_public` says who *else* may see the record. The filter is gone, the row carries `public`
and the reader's own note, and a private mark appears in the reader's own shelf marked `dnf_public: false`.

**A DNF with no progress row is not on the shelf.** It is at `/me/dnf`, which is the list of marks; the shelf is the list of
started works. Step 5 asserts that boundary so neither door drifts into the other.

**The save that never worked on the deployed backend.** The shelf test writes progress through the API, and on PostgreSQL the
very first save returned 500: `current_revision_of_unit` read `WHERE id = $1` against a `units.id` that is UUID, which is
`operator does not exist: uuid = text`. The reader's browser sends a position with no revision on arrival — that is the
default path — so saving progress was broken on the live engine for every client that did not name a revision, and the
SQLite-only suites could not see it. `$1::uuid` now.

Ratchet 68 -> 66.

### A question the module refused, citing a mechanism nobody built (marked_for_later.rs, 2026-11-08)

`marked_for_later` had three wired doors and one orphan: `is_marked(db, account, work) -> bool`. The module comment explained
the orphan by naming the mechanism that answered the question instead:

> Asking "is this work in *my* queue" is answered by `WORK_STATE` on the work page, and a per-work read would invite a caller
> to ask about somebody else's queue with the same URL shape.

`WORK_STATE` does not exist — not in the Rust tree, not in the frontend, not anywhere but that sentence. So the question had
no answer on any surface, and the function written to answer it had no caller. A client could only fetch the whole queue and
search it: a reader with two hundred saved works pays that on every work page.

`GET /me/marked-for-later/{work}` now answers in one call. It lives under `/me/` rather than at `/works/{id}/...` because the
answer is a fact about the caller, not the work — the same work is in one reader's queue and not another's. The old objection
("a per-work path invites a caller to ask about somebody else's queue") was about the path's meaning, not its safety: the
account comes from the session either way.

**A refusal is only a decision when the thing it points at exists.** This one pointed at a constant that was never written;
the next pass over a comment like this should grep for the name before trusting it, which is what found this.

Ratchet 66 -> 65.

### Three functions whose only caller was a test, or nobody (search, reading, 2026-11-08)

The ratchet's rule is "wire it or delete it", and deletion is the right answer when the function is a **narrower or less safe
twin** of the path the product actually takes. This pass deleted three:

* **`search_works_ast`** — `search_works_ast_impl` with no content filters and the default scope. `/search` calls
  `search_works_ast_scoped`, because a blocked tag has to be excluded *in SQL, before the LIMIT* (§46.4). The wrapper's
  signature has **no `filters` parameter at all**, so any future caller reaching for the shorter name would silently serve
  works a reader has asked never to see. Its only caller was `search_comparisons.rs`, a suite that then tested a path no
  reader takes; the suite now calls the real entry point through a three-line local helper.
* **`search_works_ast_filtered`** — the same wrapper with filters and the default scope, superseded by `…_scoped`, which takes
  both. Nothing called it, not even a test.
* **`reading::public_reviews`** — the twin of `positivity::visible_reviews`, which is what `GET /works/{id}/reviews` serves.
  The difference is the point: `public_reviews` filtered `is_public`, `published_at` and `deleted_at`, and `visible_reviews`
  additionally excludes **held** reviews — the ones a classifier has not delivered yet. A function that answers "the public
  reviews" more generously than the route does is a §20 obligation waiting to be broken by whoever picks the shorter name.
  This is `entry_score`'s shape again: a narrower twin of a rule the product deliberately does not follow.

**The lesson this pass keeps re-teaching:** a function with no caller is not automatically harmless. Two of these three were
*plausible names with the wrong safety properties*, and the ratchet counts them as debt precisely because the next person to
need "search works" or "the reviews of a work" will find them first.

Ratchet 65 -> 62.

### `not_implemented` for a capability whose query existed (analytics.rs, 2026-11-08)

`GET /me/analytics/{capability}` derives `implemented` from whether a query ran, and answers `{"status":"not_implemented"}`
for every scope without one. `own.work.basic` — reader count, word count, unit count for a work you wrote — was in that list
while `analytics::work_reader_count`, the query written for exactly that scope, had **no caller**. The dashboard told an
author the instance could not answer a question it had already implemented.

The number now appears on the authoring view of a work. That is where the question is asked — the dashboard's totals are
across all of an author's works, and "did anyone read *this* one" has no other surface.

* Gated by `Scope::OwnWorkBasic` through the same `allowed_under` call `/me/analytics/{capability}` makes, so a preset that
  narrows the capability narrows this too: one decision, not two. `own.work.basic` is trust level 0, so every author passes.
* **Banded at ten distinct pseudonyms**, by `Reported::of` inside the store. Below ten the answer is `fewer_than: 10` with no
  `count` — a band, because a zero would be a claim and a suppressed number is not "nobody read it".
* The method travels with the number (`definition`, `approximation`, `freshness`), so a number nobody can check is not served.
* A reader's or a stranger's view carries no `analytics` key at all — not a zero, not an empty object: the author's numbers
  are not a fact about the work.
* If the count fails, the work still renders and `analytics` says `unavailable` with a reason. An author came for the work;
  blanking the page over a statistic would be the worse bug.

Ratchet 62 -> 61.

### The batched Batch-1 deploy, executed (2026-11-08)

The batching decision said one deploy, one `pg_dump`, one verification pass for every live-surface change since M45-14. It
ran tonight, and it earned its keep by finding three live defects nothing else could see.

| step | what happened |
|---|---|
| backup | `darkhold-20261109-040927-pre-batch1.dump` — `pg_dump -Fc -Z6`, 3.5 GB, 392 `TABLE DATA` entries, verified with `pg_restore -l`. Written to `/var/tmp` because `postgres` cannot traverse `~`; moved to `~/backups` (`/mnt/disk-important`) afterwards. The first attempt wrote to `/tmp`, which is a 4 GB tmpfs and was 100 % full. |
| migrations | `0200_payment_event_key` applied — 197 applied, 0 pending. Before the new binary existed the old one reported "0 pending": `migrate --status` reads the migration list **compiled into the binary**, so a stale binary cannot see a pending migration. |
| build | `cargo build --release` on the host, `CARGO_TARGET_DIR=~/.cargo-target/darkhold`; `strings` proof that the new route literals are in the artefact. |
| result | three restarts, each binary matching the tree it was built from: `d15293f2` (the first live surface), then `c82e8bab`, then **`c90177bf`** — the final one, `strings` showing `marked-for-later` in the artefact and `/meta` reporting `build 0.1.0+c90177bf`, the `payments` block as above, `/discover/canon-blind` 200 and a nonexistent work's `/history` 404. Two of those rebuilds happened because a pass landed while a build was running; the rule that held was *restart only onto a binary whose commit equals the tree's*, not "build once and hope". |

Verified live after the restart:

* `/api/v1/meta` carries `payments {window_days: 90, processor_fee_min_minor: null, processor_fee_max_minor: null,
  processor_wired: false}` — nulls and not zeros, which is the contract: "no processor has charged us anything" is not
  "processing is free".
* `/works/{id}/canon` answers `state: "unmeasured"` for a real id; `/discover/canon-blind` answers `{items: [], state:
  "unmeasured"}` rather than an empty page pretending to be a result.
* `/works/{id}/assertion` answers `asserted: false` for a work nobody has asserted about — not a 404.
* `/works/{id}/history` refuses exactly where `GET /works/{id}` refuses: the same id gives 200/200 or 404/404, never a
  disagreement, which is the property that makes the history door safe to publish.
* `/library/unfinished` and `/me/marked-for-later/{id}` answer `401` to an anonymous caller — present, and session-gated.
* the worker's "the experiment snapshot could not be written" WARN, which fired every thirty seconds on the old binary, is
  replaced by `INFO ... the experiment snapshot for a completed day was written written=0` — see the snapshot commit.

**Three live defects the deploy surfaced, each fixed in its own commit:**

1. Every experiment snapshot failed on PostgreSQL (`CASE WHEN ac.is_guest` over a BIGINT) and the suite that covers the
   module had never run its PostgreSQL arm, because its fixture used `datetime('now')`. Exposure measurement was reading a
   table that had never been written on the deployed engine.
2. Saving reading progress returned 500 on PostgreSQL for any client that did not name a revision — the default path —
   because `current_revision_of_unit` compared a UUID column to a text parameter.
3. Half the imported catalogue (27,464 of 55,913 published works) carries `body_audience = 'trust_at_least:2'`, the mirror's
   reading of AO3's "registered users only". Twelve of this instance's twenty accounts are trust 1, so they — and every
   anonymous reader — get `404` for those works, by §7.7.3's design (a gated body answers as an absent one).

   **Decided and executed the same night, once the operator delegated the decision.** The in-tree spelling for AO3's
   "registered users only" is `accounts_only`, which `retention.rs` documents as "about having an account, not about trust
   level" — a signed-in reader at trust 0 satisfies it. `trust_at_least:2` is therefore narrower than the flag it mirrors,
   and the live distribution settled the question: only two values existed across 58,042 works, `NULL` (30,578) and
   `trust_at_least:2` (27,464). A deliberate access policy does not have exactly two states with half the catalogue on the
   strict one.

   ```sql
   -- 27,464 rows, one transaction, after pg_dump -Fc -t works (13 MB, pg_restore -l verified)
   UPDATE works SET body_audience = 'accounts_only', updated_at = now() WHERE body_audience = 'trust_at_least:2';
   ```

   Verified after: the column holds `NULL` 30,578 / `accounts_only` 27,464; two of the converted works still answer `404` to
   an anonymous caller — which is what "registered users only" means — while a public work still answers `200` on the same
   loop. The positive half, a signed-in reader at trust 0 seeing an `accounts_only` body, is the assertion in
   `body_audience::an_accounts_only_body_is_withheld_from_an_anonymous_reader`, green on both engines; I did not mint an
   account on the live instance to prove one read, because a test account left behind is pollution the proof does not need.

   **The durable half is the mapping for the next import**: an AO3 `restricted` work imports as `accounts_only`, never as a
   trust floor.

### A documented duplicate of a write (reading_goal.rs, 2026-11-08)

`reading_goal::record_unit_read` inserted into `reading_unit_read` — the record that makes the `chapters` and `words` goals
countable — and its own doc said where the row really comes from: "`reading::save_progress` writes it inside the same
transaction as the progress upsert". So the module exported a second writer of the same fact, taking a `&Database` and
opening its own statement, and nothing called it.

Deletion rather than wiring, because the function **cannot be used correctly**: the one caller that needs it needs the insert
inside `save_progress`'s transaction, and a standalone writer invites a second, non-atomic one — a progress row that lands
while its chapter record does not is a goal that silently under-counts, which is the exact failure the inline comment
already guards against. The knowledge moved into the module doc, and `save_progress` now says it is the only writer.

The general shape, worth naming because it recurs: **a function that duplicates a write is not debt that wiring can clear.**
Its existence is the defect.

Ratchet 61 -> 60.

### A sweep nobody swept (finder.rs, 2026-11-08)

`finder::sweep_expired` is the store half of M45-34: a bounty past `closes_at` becomes `expired`, and the `credit_holds` row
its `budget` reserved is released. The store tests prove both halves ("wind the close date back and sweep: the credits come
free again"). **No process called it.** On the deployed instance, a bounty nobody answered stayed `open` and kept its asker's
credits reserved — permanently. That is the first failure the module's own doc names: "A cancelled bounty that kept its hold
would quietly freeze someone's credits forever." Cancel was wired; expiry was not, so the doc's "or expiry" was a promise the
running system did not keep.

The fix is one call in the worker's maintenance pass, beside the other sweeps nobody presses a button for (import-resume
reaping, credit grants, export removal), with a `warn` on failure rather than a stop — the pass is a series of independent
sweeps and one failing should not strand the rest.

**A store function with no scheduler is the same shape as a table with no writer.** The store's tests pass, the doc is
correct, the credits are still frozen: nothing in the test suite could see the difference between "implemented and scheduled"
and "implemented".

`proposal_count` (same module) is deleted. Its doc said "whether `account` has a proposal row on `bounty`, used for the 'your
answers' view"; its signature took no account and returned a count, and the detail route already returns the proposal list.
The one caller was a store test, which now counts `proposals().len()` — the answer the route actually serves.

That doc points at a **real gap, recorded and not built**: `GET /finder/me` answers "bounties I asked" and the caller's
available credits, but not "answers I gave" — a finder cannot see their own proposals. No store query exists for it either;
`proposal_count` only looked like one.

Ratchet 60 -> 58.

### Two conveniences for one exclusion (content_filter_sql.rs, 2026-11-08)

`content_filter_sql` writes the §46.4 exclusion — "a blocked tag never reaches the client" — once, deliberately: the module
doc records that the implementation it replaced ran *after* `LIMIT` and did nothing at all on PostgreSQL. So the module's
whole value is that there is one predicate and one place to splice it.

It also exposed `exclusion_for_viewer`, which was `for_pseud` plus `build`, correlated on `works.id`. No surface called it, and
none could have correctly: every paged statement that carries the exclusion aliases its work table (`w`, `wr`, …) so the
predicate can join, and a caller using the convenience would get a correlation pointing at a table outside its FROM clause.
The callers that need one round trip want `for_pseud` then `exclusion_for(&rules, "w.id")`; the ones that do not want plain
`for_pseud`.

**Two conveniences for one job is how a surface ends up using the one whose alias does not match** — and in this module the
failure mode is silent: a broken correlation either errors loudly or filters nothing, and the previous version of this code
proved the second is possible.

Ratchet 58 -> 57.

### A suite that cannot run on the deployed engine (m16_07_review.rs, 2026-11-08)

Running `m16_07_review` against PostgreSQL — which the SQLite-only default run never does — failed all four of its tests with
`panicked at m16_07_review.rs:67: sqlite`. The harness's `operator()` helper granted the operator role with

```rust
sqlx::query("UPDATE sessions SET roles = '[\"role:operator\"]' WHERE account_id = ?")
    .bind(&account)
    .execute(self.tdb.db().sqlite_pool().expect("sqlite"))
```

so the file was SQLite-only by construction: every test in it goes through `operator()`, and on PostgreSQL the helper panics
before a single request is made. The review package workflow — versions, grants, revocation, the operator queue — had **never
run on the engine the instance is deployed on**. Once the helper was per-dialect (one `?`/`$1` difference, one `$1::uuid` cast:
`sessions.account_id` is UUID on PostgreSQL and TEXT on SQLite) all four tests passed unchanged, so this one found no product
defect — it found four guards that were not guarding what the deploy runs.

`test_support::exec(db, sqlite, postgres, bind)` now exists for exactly this: both spellings, the failing statement in the
panic. A rewriter cannot infer a column's type, which is why the helper takes both strings rather than rewriting one.

**The measurement, recorded rather than chased:** 187 files under `crates/app/tests` reach `sqlite_pool()` directly, at 517
call sites. Many are legitimate per-dialect fixtures — a fixture that seeds a raw row genuinely differs on the two engines.
The dangerous ones are harnesses: a helper every test in the file goes through that cannot run on PostgreSQL turns the whole
file into SQLite-only, silently, because the default run is SQLite. Each of those files is a guard that does not cover the
deployed engine. They should be converted as the ratchet reaches them, not in one sweep — a 187-file sweep would be a change
nobody could review.

### The fourth spelling (erasure.rs, 2026-11-09)

M45-64's erasure coverage check parses `migrations/sqlite` for account-keyed columns and fails when one is not classified. It
looked for three spellings — `account_id`, `*_account`, `*_account_id` — and the schema uses a fourth, bare `account`, on
more tables than any of the three. `trust_levels.account` is a plain `TEXT PRIMARY KEY` with no `REFERENCES` clause, so
`DELETE FROM accounts` never touched it: **the live instance held 189 trust rows for accounts that no longer existed** (plus
one `operator_role` for a deleted account, a stale privilege), while the check stayed green because it believed its own
naming convention.

Teachable because the check was `green` about the wrong set: a scanner that enumerates a convention is only as complete as
the schema's agreement with it, and nothing made the schema agree.

- **The fix is the matcher first.** `column == "account"` was added to `unlinked_account_columns()`, which immediately named
  **22 unclassified columns** across 22 tables — `payments`, `privacy_requests`, `sanctions`, `credit_entries`,
  `credit_holds`, `subscriptions`, `reader_work_progress`, `push_subscriptions`, `taste_profiles`, `trust_levels`,
  `wishlists`, `usage_counters`, and ten more.
- **`Treatment::Rekey` became `Rekey(&'static str)`.** It used to emit `'erased:' || id`, and `credit_entries` — the credit
  ledger — has **no primary key at all**; its identity is `(transaction_id, bucket)`. A bare `id` there is a runtime error on
  the first erasure, and a shared `'erased'` would re-link every severed row of one person to the others. The key expression
  is now per entry: `Rekey("id")` on `payments`, `privacy_requests`, `sanctions`, `supply_routing`; `Rekey("transaction_id ||
  ':' || bucket")` on `credit_entries`.
- **Four survive and eighteen go.** A payment, a deletion request, a sanction and a ledger line are records whose existence
  someone else relies on — they keep their row and lose the link. Standing, wishlists, layouts, presence, push endpoints,
  reading positions, taste profiles, group and event membership, extension grants, throttle logs, usage counters and
  subscriptions are about the person and end with them.
- **`reader_work_progress.account` is UUID on PostgreSQL and TEXT on SQLite.** Declared `Text`, the first PostgreSQL erasure
  died with `operator does not exist: uuid = text` before touching a row — the `ColumnKind` field exists for exactly this and
  still has to be set by reading the *other* dialect's migration.

`crates/db/tests/bare_account_erasure.rs` proves the treatments run, not merely that they are written down: it seeds
`trust_levels`, `wishlists`, `privacy_requests`, `payments` and `credit_entries`, erases, then asserts the deletes happened,
the rekeys carry the row's own key (`erased:<id>`, `erased:<transaction_id>:<bucket>`), and — over the **whole** list — that
no table still answers for the erased account. Green on both engines. Mutants red: the checker forgetting the bare spelling
(22 columns unclassified) and the rekey using one shared placeholder.

Live cleanup, after `pg_dump -t trust_levels -t operator_role` (`~/backups/darkhold-stale-20261109-0745.dump`, 8.9 KB):
`DELETE 189` from `trust_levels` and `DELETE 1` from `operator_role`, both keyed on "no account row". Verified zero remaining
in both, and zero in the other twenty tables.

Ratchet unchanged (this row is M45-64's, not M70-13's).

### A registry nobody could list (succession.rs, 2026-11-09)

`PUT /succession/redirects` publishes a redirect; `GET /succession/redirects?host=&path=` resolves one path a reader
guessed. `succession::redirects_for` — the query that lists what has been published — had no caller, so an operator could
write a registry and never read it back. A duplicate, a typo'd host, or a target that has moved on stayed invisible until a
reader hit the 404. **A registry nobody can list is a registry nobody can audit.**

`GET /admin/succession/redirects?host=` (Operator) is the door. Three decisions inside it:

* **Operator-only, though the resolve door beside it is public.** A single path with a known host is a reader's question;
  every host and path this instance has ever answered for is instance internals, and enumerating it is not something a
  stranger should be able to do one harmless query at a time.
* **It is served in resolution order** — longest prefix first, which is the store's own sort — not alphabetically. That is the
  question an operator has ("which rule wins for this URL"), and an alphabetical list silently implies `/works` catches
  `/works/1234`.
* **`?host=` is named in the response even when the registry is empty**, so "nothing is published for this host" is
  distinguishable from "you asked about a host I did not understand" without a second request.

Test `m70_13_succession_redirects.rs` asserts the listing, its order, the same rows through the public resolver, per-host
isolation, and both refusals (a signed-in reader gets `404`, ADR 0030's no-standing answer; anonymous gets `401`). Mutants red:
the door not mounted, the `require_admin_of` gate removed (a reader then reads the registry), and the sort made alphabetical.

Ratchet 57 -> 56.

### A privacy default standing in for a missing button (rec_blurbs.rs, 2026-11-09)

§49.4 clause 2 says only notes the work's author has allowed may be quoted. The module's own header corrects the spec: the note
is the *reader's* words about the author's work, so the consent is the reader's, and `review.allow_quote` defaults to `false` —
"otherwise this migration would retroactively publish every rec note ever written, which is the exact failure the clause exists
to prevent".

`top_rec_blurb` filters on it and is wired into the public work page. `rec_blurbs::quote_consent` and
`rec_blurbs::set_quote_consent` — the reader's read and write — had **no caller**. So the only reachable state of clause 2 was
"quoted nobody": every second summary on the instance was suppressed, permanently, by a privacy default standing in for a
missing button.

Two doors, both pseud-scoped and both keyed on the **work** rather than on a review id the client would have to guess:

* `GET /works/{id}/reviews/quote-consent` → `{review_id, work_id, allow_quote}`
* `PUT` the same with `{allow_quote}`

A caller can therefore only ever change the consent on their own note, and a stranger gets `404` — not `403`, which would
confirm that somebody else's note exists and is quotable.

`m70_13_quote_consent.rs` walks the whole clause: consent is off by default and nothing is shown; granting it makes the note
the work's second summary, attributed, excerpted from the reader's own sentence, naming the note it quotes; withdrawing it
removes the summary while leaving the review itself in the list; **consent is not publication** — a private note whose writer
has consented is still not quotable, because those are two decisions; and a stranger cannot consent on another person's note.
Mutants red: the consent filter dropped, the publication filter dropped, and the write inverted (withdrawal granting).

One test bug worth recording: the first version of the test read `rec_blurb` off the **author's** view of the work, where the
field does not exist. Every assertion about "nothing is shown" passed vacuously and the granting step failed for the wrong
reason. The second summary is published on the public view; a test for a public surface has to read the public surface.

Ratchet 56 -> 54.

### Deploy record, second batch (2026-11-09)

Two more restarts, each onto a binary whose commit equalled the tree's:

* `76b77f51` — the erasure fix (the bare `account` spelling, 22 columns). Built in 14m37s.
* `a7215260` — the redirect listing and the quote-consent doors. Built in 16m50s.

Verified live on the last one: `/meta` reports `build 0.1.0+a7215260`; `GET /admin/succession/redirects` answers `401` to an
anonymous caller (present, session-gated); `GET /works/{id}/reviews/quote-consent` answers `401` likewise; the public
`GET /succession` still answers `200`, so adding the admin door did not disturb the resolve door; no panic in the journal.

**Two builds were killed mid-flight and restarted, both on purpose.** The host has three other projects' release builds running
(suki, kindred-rs, plus a `timeout`-wrapped one) — killing by the generic pattern `cargo build` would have stopped work that
belongs to another session, so every kill named `-p darkhold-app`. Each rebuild landed because a pass finished while a build was
running: the rule is *restart only onto a binary whose commit equals the tree's*, and the cost of holding it is a build.

### A loan you could not ask about (lending.rs, 2026-11-09)

`lending::find_loan(loan_id)` had no caller while every surface around it was complete: `GET /me/loans` lists the caller's
windows, `POST /media/{id}/lend` takes one, `PUT` revokes. So a client could read a loan id out of the list and then ask
nothing about that loan; the only way to see one window was to re-fetch the whole list and pick the row out.

`GET /me/loans/{id}` (Authenticated) closes it, owner-scoped:

* **A loan id is not a capability.** Somebody else's loan answers `404`, not `403` — a `403` would confirm that the id names a
  loan and that it belongs to somebody.
* **State is computed, not stored.** `state_at(now)` answers `active` / `expired` / `revoked` from the timestamps, so a window
  reads `expired` the moment the clock passes `expires_at`, not when the sweep next runs. The row outlives the window on
  purpose (§32.4): the reader is entitled to see how it ended.

**The test found a live PostgreSQL defect before the route ever shipped.** `find_loan` compared `id = $1::uuid`; a path
parameter is whatever the caller typed, so `GET /me/loans/not-a-loan` raised `invalid input syntax for type uuid` and the
route would have answered `500` — a malformed URL turning into an internal error, on the deployed engine only, because SQLite
compares text to text and simply finds nothing. Fixed by casting the **column** (`id::text = $1`), which is the rule the repo
already records for every id that arrives from outside: cast the column, not the parameter.

Test `m70_13_loan_detail.rs`: the borrower reads their own window; a stranger gets `404`; a revoked loan still reads, as
`revoked`; a random uuid and a non-uuid both answer `404`. Mutants red: the owner filter removed (a stranger reads the loan),
and `state` hard-coded (`revoked` reads `active`). `milestone_25` stays green on both engines (11 each), which is what proves
the cast change did not disturb the borrow/revoke path.

Ratchet 54 -> 53.

### The queue that carries every side effect had no number (health.rs, 2026-11-09)

`outbox::undelivered_count` had no caller. `/health/ready` watched three dependencies — database, migrations, storage — and
not the outbox, which is where every promised side effect waits: notifications, webhooks, index updates. So "my comment
notification never arrived" had no number behind it anywhere in the process that could answer.

`/health/ready` now reports a fourth check, `outbox`, whose `detail` is `"N undelivered event(s); a backlog is reported here,
never a readiness failure"`.

**It is a check, not a gate.** Readiness answers "should this process receive traffic"; a backlog is not a reason to be drained
out of a load balancer or restarted, and failing here would turn a slow webhook into an outage. `ok` therefore reports only
whether the count could be *read* — a missing `outbox_events` table is a real fault — while the number is an observation a
monitor alerts on, published in `detail` so there is one health contract and one query rather than a second implementation
beside it.

Not answered, and named: **the age of the oldest undelivered event**. A count cannot tell "five just enqueued" from "five stuck
since yesterday", and only the second is the failure an operator cares about.

Test `m70_13_outbox_backlog.rs`: a quiet instance reports zero and stays ready; three enqueued events move the count to three;
marking one delivered takes it to two (and the assertion explicitly rejects a cumulative "3"). Mutants red: the check removed,
the count hard-coded to zero, and readiness gated on the backlog.

Two fixture traps, both real: health routes are mounted at the **root** (`/health/ready`), not under `/api/v1`; and on
PostgreSQL the harness's tables persist between runs, so a fixed `dedupe_key` made the second run's inserts collide and the
count read `0` for a reason unrelated to the code — the keys now carry a uuid.

Ratchet 53 -> 52.

### A counter that drifts and a repair only `psql` could run (work_metrics.rs, 2026-11-09)

`work_metrics::recompute_and_store` documented itself as "used by the seed script and as a repair tool" and had no caller in the
binary. Work-card counters are maintained **incrementally** — one bump per event, in the code path that writes the event, and
never by a trigger (the module header's reason: dialect parity). Incremental maintenance has one failure mode a materialized
aggregate cannot escape: **drift**. A request that dies between the event write and the counter bump, an import that writes
events directly, a producer that was wrong until it was fixed — each leaves the stored number permanently wrong on a page
readers are looking at, and the repair was `psql`.

`POST /admin/works/{id}/metrics/recompute` (operator) is that repair. It answers with the row it stored, so an operator sees
what readers see rather than a description of what happened.

**The test found a second defect, in the same function, that no reader would ever report.** `updated_at` is TEXT on both
engines, and the PostgreSQL `ON CONFLICT` arm stamped it with a bare `now()` — a timestamptz assignment cast into text, which
renders `2026-11-09 10:39:05.963891+00`: fractional seconds and an offset that no other row in the schema has, and that sorts
*after* every correctly shaped timestamp at the same second because `+` beats end-of-string. Nothing errors; the column simply
lies about its order. Fixed to the same `to_char(now() AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS')` the INSERT arm uses, and
the test asserts the shape (`len == 19`, separators at 4/7/10/13/16) rather than trusting the eye.

Test `m70_13_work_metrics_recompute.rs`: a drifted row (999 views against two view events, which `get_metrics` serves as
readers' truth), operator recompute answers 2, `get_metrics` afterwards reads 2 — stored, not previewed — and the stamp is in
schema shape. Mutants red: the operator check removed (an ordinary account repairs), the store call replaced with
`compute_live` (the repair is a preview), and the route answering at a path no client has.

Ratchet 52 -> 51.

### What the new check said the first time it ran (2026-11-09, live)

`/health/ready` on thinkcentre reported **40,648 undelivered events** within a minute of the deploy. The number is real and it is
not a fault: 40,641 of them are `work.published` rows created in one import burst on 2026-09-20, and they are **held on
purpose** — the ATProto bridge is off, so its handler is not registered, and the worker claims only topics it has handlers for
(`server.rs` records the reasoning: registering it with the bridge off would either mark the events delivered and lose the
backfill, or churn forty thousand rows through the backoff with a warning each).

That is exactly the case a bare count cannot describe, and it is why the check reports **age beside count**:
`"40648 undelivered event(s), oldest 50d 15h; a backlog is reported here, never a readiness failure"`. A monitor rules on the
age — a queue that stopped moving yesterday is a fault, a held backfill from September is a decision — and the count alone would
have alerted forever on a healthy instance.

`outbox::oldest_undelivered_at` is the companion store query (`MIN(created_at) WHERE delivered_at IS NULL`), wired in the same
pass so it is not a new orphan. The age renders from RFC 3339 (what producers write) with a fallback parse for the schema's
`YYYY-MM-DD HH:MM:SS` shape, and an unparseable stamp reports `unreadable (…)` rather than guessing zero — a guess would hide the
drift that makes a queue look healthy. Mutant: `backlogs_age` always answering `none` reddens the age assertion.

### Deploy record, third batch (2026-11-09)

Two restarts, each onto a binary whose commit equalled the tree's:

* `be2d2031` — the loan detail door, the outbox check, the metrics recompute door. Built in 14m21s.
* `c7045593` — the age beside the count. Built in 14m41s.

Verified live on the last: `build 0.1.0+c7045593`; `/health/ready` → `ready`, outbox `"40648 undelivered event(s), oldest 50d
2h"`; `GET /me/loans/{id}` answers `401` anonymous (and `405` to a `POST`, so the method is not silently accepted); `POST
/admin/works/{id}/metrics/recompute` answers `401` anonymous; no panic in the journal.

Host load sat between 12 and 16 throughout — three other projects' release builds share the machine, so a darkhold build now
costs 14–27 minutes. Every kill named `-p darkhold-app`; none touched another project.

### A shelf no reader could open (library.rs, 2026-11-09)

`series_recs::next_entries` answers the question §16.1 exists for — *what is next in every series I am in the middle of, plus the
series I own and have read nothing in* — and it had **six store tests and no route**. Every rule the module encodes (the
furthest finished entry is the anchor; a finished work is never suggested; a one-entry series yields nothing; cold start is its
own `Origin`) was proven against a query no reader could run.

`GET /library/series/next` (Authenticated) is the door: `{window: {since, until}, items: [...]}`, beside `/library/unfinished`
because it is the same kind of answer — a shelf of the caller's own state.

Two decisions the route owns rather than the store:

* **The window defaults to one year, and is reported back.** A series a reader finished three years ago is not a continuation,
  and a feed that keeps offering it never moves on. `?since=0` asks for all time, which is the honest answer for somebody who
  really did pause for years. `since`/`until` are unix seconds because that is `hit_rate`'s convention, and one caller passing
  one kind of window everywhere beats a prettier query string.
* **An inverted window is refused `422` naming the field.** Answering `since > until` with an empty shelf would read as "nothing
  to read" — a false statement about the caller's library.

The response speaks the reader's noun, not the table's: `series_id`, not `collection_id` (M72's noun list), and `origin` is the
store's enum rather than a word the route invents.

Test (in `series_recs.rs`, where the fixture lives): a recent finish yields entry 2 of the recent series with its title and
origin; a finish from three years ago is excluded by default and returned under `?since=0`; another reader's shelf is empty; an
inverted window is `422`; anonymous is `401` — an empty shelf and "you are not signed in" are different statements. Mutants red:
the lookback widened to a century, the inverted-window refusal removed, and the door mounted at a path no client has. 18 tests in
the file, green both engines.

Ratchet 51 -> 50.

### A gate nobody could arm (exclusions, 2026-11-09)

`permission::is_excluded` was read in exactly one place — Guard 2 of `fork_work`, *"this work is excluded from forking"* — and
`permission::insert_exclusion_entry` had **no caller, no route, and no test fixture anywhere**. The registry was therefore empty
on every instance, Guard 2 could never fire, and §33's exclusion was a comment with a SQL query behind it: protection in the code
and in the spec, incapable of refusing anything. **A gate nobody can arm is worse than no gate** — it is read as coverage.

`POST /admin/exclusions` arms it and `GET /admin/exclusions?target_type=&target_id=` reports what is armed; both operator-only
through `require_admin_of(None)`, both `404` to a caller with no standing.

Three decisions:

* **A reason is required** — the table has a `reason` column that would otherwise hold empty strings forever, and an exclusion
  nobody can explain is a decision nobody can review.
* **A second registration of the same target is refused with a field**, `422` — the only `409` this error vocabulary carries is
  `REVISION_CONFLICT`, and telling a client a version raced when the request never mentioned one is a worse error than a generic
  status. A registry exists to answer *when* and *why*; two rows for one target make that answer ambiguous.
* **`creator` is stored and named as unenforced.** `enforced_by` lists the gates that actually consult the target — `["fork_work"]`
  for a work, `[]` for a creator — so an operator is never told an exclusion is in force when nothing reads it.

The test proves the point the row exists for: a fork succeeds, the operator registers the exclusion, `is_excluded` answers true,
and **the same fork now answers `422` naming the exclusion** — Guard 2 firing for the first time in the codebase. A refused
registration leaves the registry alone; a stranger gets `404` on both doors.

One assertion had to be rebuilt: step 6 reused the already-excluded parent, so the duplicate check answered `422` whether or not
the reason was validated, and the mutant that deleted the reason check **survived**. Re-targeted at a fresh work — an assertion
that cannot tell two refusals apart tests neither. Mutants red: the entry not written, the duplicate check removed, the reason
check removed, `creator` claiming a gate, and the operator check removed.

Ratchet 50 -> 49.

### Deploy record, fourth batch (2026-11-09)

`71d0cae9` — the series shelf and the exclusion registry — live on thinkcentre; verified `/meta` `build 0.1.0+71d0cae9`,
`GET /library/series/next` `401` anonymous, `GET`/`POST /admin/exclusions` `401` anonymous, no panic in the journal.

The build cost **32m42s** because the host load peaked at 41.7 (three other projects building at once), and a first attempt died
with SIGTERM mid-compile — nothing in this repo's tooling or the tree, but worth recording: at this load a darkhold release build
takes half an hour and a killed build is expected, not exceptional, so the binary commit is verified against the tree before every
restart (`/meta` `build` string) rather than assumed from a successful `cargo build` earlier in the session.

### An invitation the sender could not see, and a grant that skipped consent (collaboration, 2026-11-09)

Two orphans in one module, and they are the same question asked from both ends.

**`invites_for_work` had no caller.** `POST /works/{id}/contributors/invitations` mints an invitation, `revoke_invite` kills one,
and the invitee has `GET /invitations` — but the owner could not list what their own work had sent. That is the same shape as a
registry nobody can audit: a decision recorded and unreadable. `GET` on the same path (same collection, same permission rule)
returns **every** invitation, not only pending ones: the owner's question a week later is "did Ink ever answer", and an answered
invitation is a record of a decision. The invitee's side stays pending-only, which is the opposite filter on purpose — a person's
inbox is what still needs them.

An editor reaches the work but may not manage it, so the list answers them `403`, not `404`: they can disprove "the work
vanished" by loading it, and the list names *other people the owner approached* — a collaborator is not entitled to the owner's
recruitment.

**`add_contributor` was deleted and moved to `test-support`.** It wrote a contributor row directly, with no invitation. Nothing
in the product called it — only eleven fixture call sites — and it was the non-transactional twin of `respond_to_invite`'s grant
with two differences that made it the wrong function to keep: it could *change* an existing contributor's role and attribution
(`DO UPDATE`, bypassing the owner-protection rule in `update_contributor`), and it could grant rights to a pseud that never
consented. §4.3's model is that an accepted invitation grants rights to exactly that pseud, so a second path that skips acceptance
is a consent bypass with a plausible signature. It is now `test_support::grant_contributor`, where a fixture can reach it and the
binary cannot.

The one honest gap in this pass's evidence: **the route was written before its test**, so the new test was never seen red. The
remedy is recorded rather than glossed — the door-unmounted mutant stands in for that RED (it answers `405`, the test reddens),
and the same run re-derived the RED for the management check and for a pending-only filter. Mutants red: the door at a path no
client has, the management check removed (an editor audits the owner's recruitment), and only-pending listing (an answered
invitation disappears from the record). One mutant had to be re-targeted because `manage_decision` appears in four handlers and
the first `replace` hit `create_invitation` — the wrong edit, which proves nothing.

Ratchet 49 -> 47 (one wired, one deleted).

### Deploy record, fifth batch (2026-11-09)

`0ad9748d` — the invitation list, the deleted direct grant, the series shelf and the exclusion registry riding along — live;
`/meta` `build 0.1.0+0ad9748d`, `GET /works/{id}/contributors/invitations` `401` anonymous, `GET /library/series/next` `401`
anonymous, `/health/ready` `200`, no panic. Build cost 25m21s at load 30.

### A second doorway to one curated state (taxonomy.rs, 2026-11-09)

`taxonomy::curate_node` set `canonical_entities.canonical` and `review_status = 'curated'` and touched nothing else. §15.17 keeps
two tables describing one curated state, and `curate_name` writes both in one transaction — so a caller that reached `curate_node`
curated a name in the vocabulary `search_nodes` reads while leaving the exchange's row unverified: the disagreement migration
0082 says a join silently drops, and `curation_reaches_taxonomy.rs` names this function as *the hazard `curate_name` exists to
remove*. It also recorded no curator, leaving `curated_by`/`curated_at` NULL on the row it did touch — the columns that exist so
"who decided this" is answerable.

Deleted, with no replacement: `curate_name` is the only doorway, and a second `pub` writer is how the vocabularies drifted in the
first place. The tests that pin the end state (`curation_reaches_taxonomy.rs`, 3 green) never used it — they hold whichever writer
is used, which is why deleting a `pub` function broke nothing.

**Not deleted, and the reason matters:** `taxonomy::recompute_confidence` stays on the list because it is spec work with both
halves missing, not a redundant twin. It maintains `work_tags.confidence` from `work_tag_votes`, and today **nothing in the binary
writes a vote and nothing reads `work_tags.confidence`** — no `wt.confidence` appears in a single query. Wiring the sweep would
make a number fresher that no surface shows and no product path feeds: ceremony around a dead column. The honest row is the
reader-vote feature (a vote endpoint, a per-row recompute on cast, and a surface that shows confidence), and when that exists this
sweep returns with its reader. Its two assertions in `work_tag_provenance.rs` are the only executable record of §6.4's smoothing
formula, which is the second reason not to delete it.

Ratchet 47 -> 46.

### The reader layer behind a tag's confidence (taxonomy, 2026-11-09)

`recompute_confidence` was not a function nobody had wired. It was the *only* half of a feature: nothing in the binary wrote a
vote (`work_tag_votes` had no writer at all) and nothing read `work_tags.confidence` (no `wt.confidence` in a single query). Wiring
the sweep would have made a number fresher that no surface showed and no product path fed — ceremony around a dead column. So this
pass built the missing half and demoted the sweep to what it actually is.

* `PUT /works/{id}/tags/{node}/vote` — `{vote: 1 | -1 | 0}`, pseudonymous. The vote's identity is the caller's active pseud; a
  request may say *what* it thinks and never *who* it is.
* `GET` on the same path — **your** vote beside the confidence everybody shares. Nothing here returns another reader's vote: a
  pseud-scoped opinion published with its owner is a durable public judgement about somebody else's work.
* `POST /admin/taxonomy/confidence/recompute` — the sweep, as the operator repair pass it always was.

Four decisions:

* **`0` withdraws the row.** "I take it back" is not evidence against the tag; storing it in the denominator would let four readers
  who all withdrew pull a tag's confidence down — a disagreement count with no disagreement in it.
* **A tag the work does not carry answers `404`**, on both doors. There is no claim to judge; `422` would blame the caller for a
  request that was well formed, and `1.0` would claim nobody had disputed a tag that is not on the work.
* **The vote path computes its own row.** A reader who answers should see the number their answer produced, not one that arrives
  after the next sweep. The sweep covers the window the vote path leaves — a crash between the vote row and the aggregate — and a
  restore that replayed votes without their confidence.
* **One expression, two callers.** `confidence_expression` is the sweep's formula scoped to one row, so the repair cannot disagree
  with what a reader just saw; the mutant that drops the smoothing from the sweep reddens the test that reads the number back.

Two PostgreSQL defects found by running the file on the second engine, both invisible on SQLite:

1. **`CASE` types.** `THEN 1.0` is a NUMERIC literal on PostgreSQL, so the whole `CASE` decodes as NUMERIC and sqlx cannot turn it
   into `f64` — every vote answered `500`. The cast goes on the `CASE`, not on one branch (a cast inside `THEN` leaves the `ELSE`
   to decide, which is the same mistake spelled differently).
2. **INT4 binds.** `work_tag_votes.vote` is `INTEGER` on PostgreSQL, and sqlx binds `INT4` only from `i32`; SQLite's native `i64`
   hid it. Bind `i32` on the POSTGRES arm and `CAST(vote AS BIGINT)` on read, so one decode target serves both engines.

And one in the test itself: the row-count assertion reached `sqlite_pool()` directly, which is how a file becomes SQLite-only —
counted per engine instead.

Mutants red: a withdrawal stored as an abstention; the read asking by tag alone instead of tag-and-voter (A sees B's `-1`); an
untagged tag answered `200` as if undisputed; the repair pass dropping the smoothing the vote path keeps. One mutant was discarded
for not compiling (unused binding under `build.warnings` deny) — a mutant that does not compile is not evidence.

Ratchet 46 -> 45.

### Deploy record, sixth batch (2026-11-09)

The tag-vote doors live. `/meta` `build 0.1.0+aaebc98a`, `GET`/`PUT /works/{id}/tags/{node}/vote` `401` anonymous, `POST
/admin/taxonomy/confidence/recompute` `401` anonymous, `/health/ready` `200`, no panic. Backup `darkhold-20261109-152456-pre-tagvotes.dump`
(3.5 GB, 392 `TABLE DATA`, `pg_restore -l` verified).

Two operational facts worth keeping:

* **the database is `darkhold` on the host's PostgreSQL, not `lorehaven`** — `pg_dump lorehaven` fails with "database does not
  exist", which is how this was found again; the product is darkhold and the DB name matches it;
* **`/tmp` on the host is a 4 GB tmpfs and it was full** (100%, 4.0G/4.0G), so a dump written there died with "No space left on
  device". Dumps go to `/personal/backups/`, which has 1.5 TB free.

**The deploy tree is one commit ahead of this repo**: `aaebc98a` ("logging: the modlog's rules are `polaris-ledger`'s now, and
`into_entry` only resolves"), which exists on no remote and is not this session's work. It is an ancestor relationship — my
`7f42f0f6` is below it, so the deployed binary contains this pass — but the tree and `master` are no longer the same commit, and
that divergence is recorded rather than reset away.

### A privacy request a person could file and nobody could finish (admin.rs, 2026-11-09)

`POST /me/privacy-requests` has existed since migration 0021, with `GET` beside it so a person can watch their own list. But
`admin::complete_privacy_request` had **no caller**, and no door anywhere listed requests for an operator — so a `delete` request
sat in `pending` forever, the person asking could see it never move, and finishing one required a `psql` statement nobody had
written down. A queue with no action, where the queue is a legal obligation: an erasure request that never leaves `pending` is a
promise the instance made and cannot keep.

Two operator doors, both through `require_admin_of`:

* `GET /admin/privacy-requests?state=&limit=` — every account's requests, newest first. `?state=pending` is the working queue; no
  filter is the audit, because "what did we ever promise" is a question a filter would hide the answer to. `limit` is clamped to
  200: a queue route is a paging device, not a dump.
* `POST /admin/privacy-requests/{id}/complete` — `{result_ref?}`, answers with the record it wrote.

Four decisions:

* **Only `pending` completes.** `completed_at` and `result_ref` are what make the row evidence; a second completion would rewrite
  them to say whichever operator came last. It answers `422` naming the state it is in.
* **An unknown id is `404`.** A request id is not a capability, and the queue is not an existence oracle for anyone.
* **The completion is dated in the answer as well as stored**, so an operator who closes a case sees the timestamp the person will
  see.
* **`result_ref` stays optional**, because a `delete` has nothing to point at while an `export` does — but an empty string is
  stored as `NULL`, the same rule as "never checked is not clean": a reference of `""` would read as a reference.

The per-account list stays per account: the operator's queue is a second function, not a flag on the first. A person's own list
must not be able to answer "what have other people asked for", and a `bool` parameter is how that check gets lost in a later
refactor.

Mutants red: the completion dropping its `pending` guard (a second call rewrites the evidence); the route reporting success
whenever it was called (an unknown id answers `200`); the queue ignoring the state filter (a finished request stays in the queue);
the completion door refusing nobody (the person who asked closes their own case). Two of them had to be re-aimed off a
non-compiling first form — `if true` leaves the rest unreachable under `warnings = deny`.

**Named, not wired:** `admin::increment_abuse_counter` is the other orphan in this file, and it is a different shape. The counter
has a reader (`GET /admin/abuse-status/{key}`, which reports `blocked` and `count`) and no writer, so `blocked` can never become
true — but the missing half is a *policy* decision (which failures count, over what window, at what threshold), not a door. Wiring
it into the wrong place would arm a brute-force block with an unowned threshold. It stays on the list until that policy is named.

Ratchet 45 -> 44.

### Deploy record, seventh batch (2026-11-09)

`0.1.0+3f116482` — the privacy queue and the completion door live. `GET /admin/privacy-requests` `401` anonymous, `POST
/admin/privacy-requests/{id}/complete` `401` anonymous, `/health/ready` `200`, no panic. Backup
`darkhold-20261109-161950-pre-privacy.dump` (3.5 GB, 392 `TABLE DATA`, `pg_restore -l` verified) in `/personal/backups/`. Build
took ~22 minutes at load 44.

### An alias that renamed a filter it did not apply (recommendation_slots.rs, 2026-11-09)

`list_pending_wrangling(db, limit)` was `list_wrangling(db, None, limit)` under a second name, with no caller. The naming was
worse than redundant: `None` already *means* pending (`WHERE status = COALESCE(?, 'pending')`), so the alias claimed a filter
that `list_wrangling` applies anyway, and anyone reading the two signatures would reasonably expect `None` to mean "all states" —
the one thing it does not do. The queue's one door is `GET /admin/tag-wrangling/proposals`, which passes the status through so a
wrangler can ask for `approved` or `reverted` too. Deleted; `m29_transparency.rs` 48 green.

**Left, with the missing half named:** `list_response_slots(pseud_id, request_id)` would let a reader replay the hand a request
served. Nothing calls it and nothing has asked for it: the hand is shown once at the draw, and per-slot transparency already
exists (`GET /discovery/slots/{slot_id}/explanation`), which is the question a reader actually asks — "why did this one appear"
rather than "show me the four again". A replay door is a product decision about how long a served hand stays addressable, not a
wiring omission; it stays on the list until that decision exists.

Ratchet 44 -> 43.

### The ledger's notes cell became a document, and broke its own reader (scripts/ledger.py, 2026-11-09)

`gate.sh ledger` went red on the file, not on a row: `csv`'s default field limit is 131,072 bytes, M70-13's `notes` cell had
reached **131,739 characters** — every pass of the ratchet appended in place — and `scripts/ledger.py` could no longer read the
ledger it had written. The pass before this one was committed **through** that red gate: the gate prints its log tail and the
commit went ahead, which is its own finding (a gate whose failure can be scrolled past is not a gate).

Three repairs, in the order they matter:

1. **The journal moved out of the cell.** The 131,739 characters are now `docs/plans/m70-13-pass-log.md`, verbatim, and the cell
   carries a 448-character summary and a pointer. A CSV cell is a field, not a container: `psql \copy`, spreadsheets and
   `pandas` all have opinions about a 131 KB field, and the ledger is meant to be read by all three.
2. **The reader accepts what the writer accepts.** `csv.field_size_limit` is raised to 16 MiB with the reason written beside it —
   the default is a guard against a malformed file, not a bound on what a real row may say.
3. **A write that would create the problem is refused.** `NOTES_LIMIT` is 32 KiB and `write_rows` raises, naming the row, before
   anything is written: "the pass journal belongs in `docs/plans/roadmap-remaining.md`; keep a summary and a pointer here". The
   failure now lands on the pass that grows the cell, where the author still remembers what they were doing, instead of on the
   file months later.

Two tests pin it, and the second one caught a mistake in the first: the reader test initially wrote its 140 KiB fixture through
`write_rows`, which the new cap refuses — a fixture that has to pass the writer cannot prove anything about a file the writer
would not produce, so it writes the CSV directly. Ledger: 952 rows, 0 lone LF, biggest remaining `notes` cell 5,004 characters.

Ratchet unchanged at 43 (this pass repaired the ledger, not the ledger's subject).

### The ratchet was counting by name, so its number was wrong (scripts/orphan-queries.py, 2026-11-09)

Tracing `decision_audit::for_subject` turned up something bigger than the orphan: **`decision_audit::record` has no product
caller at all**, so nothing in the binary ever writes a decision, and the operator's `GET /decisions/audit` therefore always
answers an empty trail. The audit page is honest-looking and empty — the shape this loop keeps finding.

But `record` was not on the orphan list, and the reason is the ratchet's own measurement. Reachability was decided by **bare
name**: `calls(blob, name)` — any `record(` in any app module counted as a call to `decision_audit::record`. Half a dozen db
modules have a `record`, so each covered for the others, and `decision_audit::record` inherited reachability from its namesakes.
The same bug ran in the other direction too: a function reached through `use darkhold_db::lexicon::normalise;` and then called
bare looked dead.

Three changes:

* **The graph is keyed `module::name`** and edges come from each function's *own* body, so `a::record(` no longer marks
  `b::record` reachable.
* **Imports are evidence**: `use darkhold_db::module;` (or `crate::module::`) makes a bare call in that file count for that
  module. Without this the correction overshot — a first module-qualified version said **80**, because it ignored imports
  entirely.
* **The Rust test no longer measures.** `crates/app/tests/orphan_queries.rs` recomputed the same closure with its own name-based
  algorithm; two implementations of one measurement is how a count drifts, and these two had already drifted. The test now runs
  `python3 scripts/orphan-queries.py --check`, so there is one answer.

Measured correction: **43 → 57 orphans**. The debt is not new; the ruler was. Read every future number against that.

**Left, with the missing half named:** `decision_audit::record` needs a decision path worth auditing. The only one in the binary
is `POST /decisions/evaluate`, which returns fixed labels per task (`positivity_class → "positive"`, …) — recording those would
populate the trail with decisions no policy made, which is worse than an empty table: it would make the audit a claim about
nothing. The row stays until a real decision exists, and `GET /decisions/audit` stays empty rather than pretending.

A note on that commit (`161c187b`): its **body** is damaged. The message was passed inline in a double-quoted shell string, so the
backticked identifiers — `record`, `` `record(` ``, `use darkhold_db::m::f;` — were command-substituted away and the sentences read
"`record` has no product caller… was not on the orphan list". The history is not rewritten for a message: the commit is on both
forges and merged into the deploy tree. The full text is the section above, and every later commit in this loop uses
`git commit -F <file>` for exactly this reason.

### The audit trail has a writer: the positivity gate records the decision it makes (2026-11-09)

The measurement correction left one row open with its missing half named: `decision_audit::record` had no caller, so
`GET /decisions/audit` answered `{ "items": [] }` to the operator question it exists for. This pass supplies the caller.

**Where.** `positivity::classify_review` and `positivity::classify_comment` — the gate that decides whether a reader's words reach
an author. Both now call one private `record_decision`, so the decision is recorded where it is made rather than in the route that
calls it: a second caller (backfill, import, retry) records too. A decision surface that has to remember to audit itself is a
surface that will stop auditing.

**What a row says.** `task = "positivity"`, `subject` = the review or comment id, `deterministic` = the classifier's class,
`outcome` = what the policy applied (`delivered` / `held`), `provider = deterministic`. Three fields are decisions, written beside
the code:

* `posterior` is **`None`, not `0.0`** — no model was consulted, and a zero reads as "a model said no with certainty", which is
  the fiction the module's own docs warn an operator off tuning against.
* `threshold` is **`None`** — a class is not compared against a threshold; the author's preferences decide. Inventing a number
  would be a number an operator later tunes a real threshold against.
* a failed write **propagates**. The route's compensating action (withdraw the review) is only correct if it learns now, rather
  than months later from an empty table.

The graded text is not in the row — §12.1, and the test asserts the response does not contain the sentence it graded.

**Proof.** `decision_audit_routes::a_held_review_records_the_decision_that_held_it`: the author turns comments off, a reader posts
a public review, the receipt says held, and the operator's `?subject=<review id>` read returns exactly one row. Seen red first
(`items: []` while the receipt already said "held"), green after. Both engines (10/10 SQLite, 10/10 PostgreSQL). Two mutants, each
reverted from a `/tmp` backup: the recording call removed (red at the row-count assertion), and `posterior: Some(0.0)` (red at
`is_null`).

Deleting `decision_audit::for_subject` in the same pass, because it was a second spelling of `?subject=` with **no caller outside
a test** — the two store tests now call `list` with `AuditQuery { subject, .. }`, which is what the route serves.

**The ratchet's ruler was wrong a second time, in the other direction.** The module-qualified graph tested uniqueness against the
*public async* names only, so a private helper (`async fn record_decision`) was never "unique", got no edge from its caller, and
everything it called stayed on the list — which kept `record` listed even after it had a caller. Uniqueness is now computed over
every function in the graph. Baseline correction: **43 → 57 → 55** (57 with the public-only uniqueness, minus this pass's two
removals, with the uniqueness fix worth zero on its own). Stated because the next number is read against it.

**Deploy:** this changes a live write path (every held review or comment now inserts a row), so it ships to thinkcentre in this
pass rather than waiting — if the insert fails on the deployed engine, posting a review fails, which is not a failure to discover
later.

**Deployed (eighth deploy, 2026-11-09 18:0x).** Backup `darkhold-20261109-174211-pre-audit-writer.dump` (3.7 GB, 392
`TABLE DATA`, `pg_restore -l` verified) in `/personal/backups/`. Build 18m54s on the host, tree at merge `811d4294`, no migrations
in the merge and `migrate --status` confirming `197 applied, 0 pending`. Restart kept the replaced binary
(`~/backups/darkhold-replaced-*.bin`). Verified: `/api/v1/meta` build `0.1.0+811d4294` (matches the tree), `/health/live` 200,
`/api/v1/decisions/audit` **401** anonymously (unchanged contract), journal clean, no panic. `decision_audit` holds 0 live rows at
restart — correct, and the one thing this deploy cannot show by itself: the table fills on the next review or comment the gate
classifies, which is user content and not something a probe should manufacture on real works. The write path is proven on the
deployed engine by the PostgreSQL store test against the same migration set.

### The ruler was wrong a third time: a bare call in the function's own module (2026-11-09)

Working the list turned up five entries that are demonstrably reachable — `metrics::candidates` and `metrics::counts` are called by
`ranked_with_weights`, on the catalogue route, on every page a reader loads. They were listed because `specialist::candidates` and
`trend::counts` exist: the name was not unique, the caller did not `use` its own module, and so no edge was drawn. The third rule
was missing, and it is the strongest of the three — **a bare call inside the module that defines the function is that function**,
no import and no uniqueness needed.

That correction alone took the list **55 → 50** (`exchanges::is_participant`, `live_badges::recompute`, `metrics::candidates`,
`metrics::counts`, `work_mirrors::find`).

Then one real deletion, from the rule that has held all through this row: **a function whose only callers are tests, and which is a
narrower twin of the product path, is deleted and the tests follow it.**

`metrics::ranked(db, query, metric, limit, offset)` was `ranked_with_weights(…, &QualityWeights::default())`. Both catalogue routes
call the weighted form, so the short one shipped nowhere — and it had the worse property for a twin: the shorter signature is the
one a new caller reaches for, and it ranks without the operator's configured weights. It compiles, it answers, and it disagrees
with the page the reader is looking at. The two store test files (`quality_metrics.rs`, `quality_rate.rs`, nine call sites) now call
`ranked_with_weights` with the same defaults, which is the path that ships.

**Proof.** The weights are not untested, which is the thing a deletion like this has to check: mutating `ranked_with_weights` to
ignore its `weights` argument reddens
`m63_03_metrics::the_catalogue_names_and_obeys_the_quality_profile_it_ranks_with` (line 564). First attempt did not compile — the
unused parameter trips the workspace's deny-warnings — and a mutant that does not compile is discarded, not counted; the second used
`let _ = weights;` to keep it compiling, then reddened the intended assertion. Reverted from a `/tmp` backup.

Ratchet: **50 → 49**. Ruler history for the next number: 43 (bare name) → 57 (module-qualified) → 55 (uniqueness over every
function) → 50 (same-module calls) → 49 (this deletion).

### `notifications::list` was considered and kept, with the measurement written beside it (2026-11-09)

The next candidate was the textbook twin: the unfiltered inbox, whose own doc says a caller who can see a work should use
`list_filtered`, because a notification carrying a `work_id` puts that work's title in `title` and the unfiltered read hands back
exactly what the reader's filter exists to withhold.

Measuring the pair settled it the other way. `list_filtered` with an empty rule set **is** `list` — same SQL, same rows — so the
leak lives in the caller that passes no rules, not in the function that takes none. Deleting `list` would have pushed ten test call
sites through a filter API with an empty list, making the primitives (`notify`, `mark_read`, `unread_count`) harder to read in
exchange for no safety at all. The route that serves readers passes rules; that is the place to check.

So: reverted before committing, and the reasoning now sits in the function's doc comment where the next reader will hit it. A
deletion that survives its own measurement is the only kind worth making — this one did not.

### A claim that never expired, and the sentence "Called by the job queue" (2026-11-09)

`events::expire_claims` documents itself as *"Called by the job queue."* No queue called it, no route called it, no test called it.
So a reader who claimed a request and then stopped writing left the claim `outstanding` for good — and a claim is the row that tells
the next reader *somebody is already writing this*. The request became one nobody could pick up, and nothing looked broken: the row
said exactly what it meant.

This is the third instance of one shape in this row (`finder::sweep_expired`, the entity closure, the experiment snapshots): **an
idempotent store function with no scheduler is a schedule, not a door.** The fix is one call in `Worker::maintenance_pass`, beside
the other sweeps nobody presses a button for.

**The grace period is a decision, written where it is used.** There is no config field for it, and inventing a knob nobody tunes is
worse than a constant with a reason: thirty days is long enough for a short fic and short enough that a claim abandoned in week one
does not hold the request for a season. The store keys on `status = 'outstanding'` (migration 0080), so a second pass matches
nothing and its count means "what this run did".

Test `m70_13_claim_expiry.rs` seeds two claims — one stamped 2026-01-01, one stamped with the worker's own clock — runs the pass,
and asserts the old one is `expired` and the fresh one is still `outstanding`, then runs the pass again to prove idempotence. Run
ids carry a run-unique suffix because the PostgreSQL harness keeps tables between runs and `claims`' primary key is
`(request_id, claimant)`.
**Proof.** Red first: the pass was wired, ran, and the claim stayed `outstanding` (`left: "outstanding"`, `right: "expired"`).
Green on both engines (1/1 SQLite, 1/1 PostgreSQL). Two mutants, each reverted from a `/tmp` backup:

* **ten-year grace** → red at the stale-claim assertion. (The obvious first mutant — deleting the sweep call — does not compile,
  because the grace constant then goes unused and this workspace denies warnings; discarded rather than counted.)
* **zero grace** → red at the fresh-claim assertion.

That second mutant found a defect in my own fixture, which is the part worth keeping: the fresh claim was stamped with the worker's
own clock, so it sat exactly on the cutoff, and the store compares strictly (`claimed_at < cutoff`) — a zero grace would have left
it unexpired and passed. The claim is now an hour old, so "recently claimed" has slack and the zero-grace mutant reddens. A fixture
that lands on the boundary of the comparison it tests proves nothing about that boundary.

Ratchet: **49 → 48**.
**Deployed (ninth deploy, 2026-11-09 19:0x).** Backup `darkhold-20261109-183719-pre-claims.dump` (3.57 GB, 392 `TABLE DATA`,
`pg_restore -l` verified) in `/personal/backups/`. Build 20m08s on the host from `b7a91c52`, no migrations (`197 applied, 0
pending`), replaced binary kept for rollback. Verified: `strings` on the binary contains the sweep's log line, `/api/v1/meta` build
`0.1.0+b7a91c52` matching the tree, `/health/live` 200, journal clean over three minutes, no panic.

`claims` is **empty on the live instance**, so the sweep's log line cannot appear — the `Ok(0)` arm is silent on purpose. That is
worth stating rather than dressing up: the live deploy proves the code is running, and the behaviour is proven by the both-engine
test. The first claim anybody makes will be expired by a pass thirty days later, and the journal will say so.

### §16.18's third selection method gets a door, and it is a dry run by default (2026-11-09)

The spec lists three ways an instance picks Taste Vanguards. `admin_appointment` has had its door since M18 (one account at a
time). `resonance_threshold` needs an admin taste profile nothing on this instance computes. `contribution_volume` — "top N
curators by bookmark and review count in the last 90 days" — is a query, and both it and the dispatcher written to apply it had no
caller, so an operator who wanted the top curators had to find them by hand.

`POST /vanguards/select` (Operator) runs the selection. Three decisions inside it:

* **The dry run is the default.** A Vanguard can pin works to the front page, so a selection that appointed people as a side
  effect of asking "who would qualify?" would turn a question into an appointment. `apply: true` is the operator saying yes with
  the list in front of them; `apply` absent grants nothing.
* **The method is validated by name.** Anything but `contribution_volume` answers `422` naming `method`, because an empty list
  would answer "nobody qualifies" to a question this door never asked.
* **`limit` is clamped to 1..=100**, default 10 — a bound and a default, which are different things.

Two smaller decisions recorded beside the code: the operator check is this file's own `require_operator` (trust ≥ 5) rather than
the estate predicate, because five spellings of "is this the operator" exist and converging them is its own row — a single door
answering differently from its four siblings would be worse than the duplicate. And `/vanguards/select` sits beside
`DELETE /vanguards/{account_id}`; axum prefers the literal segment, so the new route does not shadow the param route (asserted by
`route_inventory` 5/5, and by the 405 the test produced before the route existed).

The test found two things while being written, both kept as notes in the file: `bookmarks.updated_at` is `NOT NULL` (the insert
needs it on both engines), and the module gate answers `404 MODULE_UNAVAILABLE` unless `vanguard` is enabled — `vanguard` is
opt-in by design, so the harness enables it rather than bypassing the gate.

**Proof.** Red first (405 Method Not Allowed — the route did not exist), green after: 1/1 SQLite, 1/1 PostgreSQL, `route_inventory`
5/5 on both. Two mutants reverted from `/tmp`: `apply` defaulting to true reddens the dry-run assertion ("a dry run must not grant
the role"); removing the method validation reddens the 422 assertion with a 200 carrying an empty list.

Ratchet: **48 → 46** (both `select_vanguards` and `select_by_contribution_volume` now have a caller).

## Batch 2 (added 2026-11-09) — M73, the affinity kernel

Owner direction: *"search resonance, affinity, attuned, etc. figure out if there's some common functionality that could be
extracted as a shared component as generally useful and flexible as possible."* Analysis and rejected alternatives are in
`~/secondbrain/10-Projects/polaris-estate-affinity-kernel.md`; the queue row is 13 in `polaris-estate-programme.md`.

**What the sweep found.** The estate has five separate answers to "how much does this match", and the ordinary-ranked queue does
not own the one that matters:

* darkhold `domain/affinity.rs` — weighted mean over the components that carry evidence (0.5 favourite / 0.3 tag / 0.2 arena),
  absent signals removed from both halves of the division, `None` when nothing is measurable; plus `jaccard`, `cosine`,
  `arena_agreement`.
* ascend `themes.rs` — `confidence(n) = n/(n+5)`, `combine(prior, evidence, confidence)`, `multiplier` bounded to 1.0–3.0,
  `percentile_ranks`.
* orbital `ranking.rs` — `cosine` over tag sets, `tag_relevance` = `Σ(w·hit)/Σw`, `trust_mult`, and `0.0` for both "not
  applicable" and "not relevant" while its own comment says they must not look the same.
* kindred — `assoc_stats` (already `polaris-assoc`), `mmr` (already `polaris-rank`), `glicko` (already `polaris-rating`).
* `polaris-taste` — ballots, ratings, weights, budgets: the *input* platform, not the arithmetic.
* `polaris-gravity` — already extracted, and the precedent for the shape: the arithmetic is the component, the weights and bounds
  are declared data at the call site, refusals are named types.

The disagreement is the finding: `0.0` versus `None` for "no evidence" is reader-visible — it is the difference between "we do not
know you yet" and "you are not relevant here". Duplication here is not wasted lines, it is a wrong answer.

**Decision.** `polaris-affinity` 0.1: pure functions, no state, no DB, no HTTP, no `product` parameter.

```
similarity::{jaccard, cosine_counts, agreement_share}  -> Option<f64>
evidence::{Component, weighted_mean}                   -> Option<f64>
confidence::{confidence(n, k), blend(prior, evidence, n, k)}
bounds::multiplier(affinity, Bounds)                   -> f64   // bias, never collapse
cohort::percentile_within(cohort, value)               -> f64
decay::{half_life, by_age}
```

Three rules the crate enforces: absence returns `None` and the call site decides what absence means; weights and bounds are the
caller's data; `multiplier` is total and bounded so affinity 0.0 still ranks at the declared minimum.

**Rows.** M73-01 crate; M73-02 darkhold adoption by deletion; M73-03 ascend; M73-04 orbital.

**Order — why Batch 2 and not now.** M73-02 deletes darkhold's affinity arithmetic, which is on the M70-13 ratchet's list
(`select_by_contribution_volume` was the pass that produced `affinity`, and the module's helpers are reachable). Landing M73-02
first would regenerate `docs/plans/orphan-queries.txt` twice in three passes and make two baselines stale in the same day. The
ratchet passes in flight cost nothing to finish, so M73 starts after the current ratchet slice — and M73-01 is independent of
darkhold, so it can run in parallel with ratchet passes if a second session is free.

**Non-goals.** No taste service, no vector store, no schema, no replacement for `polaris-taste`'s ballots, `polaris-rank`'s
profiles, `polaris-assoc`'s co-occurrence statistics or `polaris-gravity`'s reader dial. Kindred is not an adopter: it has no
profile→score surface, and linking a crate it does not call would spend its dependency budget for symmetry.

#### Deployed 2026-11-09 — and one flag worth reading

`POST /vanguards/select` is live on thinkcentre. Build 16m43s, `pg_dump -Fc` 12m06s to
`/personal/backups/darkhold-20261109-1948?-pre-vanguard-select.dump` (3,738,288,128 bytes, 392 `TABLE DATA` entries
enumerated by `pg_restore -l` before the restart — a dump that restores a manifest is a dump, not a file that exists), unit
restarted, `/health/live` 200, `/api/v1/meta` answering, and `POST /api/v1/vanguards/select` answering **401** to an anonymous
caller — the door is reachable and gated. `vanguard` is in the live `[modules] enabled` list, so this route is not shipping dark.

**The flag: the binary stamps `0.1.0+e0678c3b.dirty`.** `git status` in the deploy tree is clean *now*, and the tree's HEAD has
moved to `beb93534` ("healthz: report the estate version, keep the build id") — a commit this session did not make, 20 ahead of
the propagation checkout. So another session was working in the deploy tree while the release build ran, and the build recorded
the tree it saw: clean at `e0678c3b` plus whatever was uncommitted at that instant, which has since been committed as `beb93534`.
`strings` on the deployed binary contains both `vanguards/select` and this pass's refusal text, so **this pass is in the
running binary**; what cannot be claimed is that the binary is exactly one commit. Nothing was reset, reverted or force-merged:
the foreign commit stays, and the next deploy from a quiet tree will produce a clean stamp. Recorded rather than smoothed over
because "0.1.0+<sha>.dirty" on a production binary is a fact the next operator needs, not a cosmetic suffix.

#### Deployed 2026-11-09 — the pull that had never once run on PostgreSQL

Build 15m03s from a clean tree (`git status` empty, another session's work absent this time), so the stamp is
**`0.1.0+99aeaa32`** with no `.dirty` suffix. `pg_dump -Fc` verified by `pg_restore -l` (392 `TABLE DATA` entries) before the
restart; unit active; `/health/live` 200; `/api/v1/meta` naming the build.

**The finding is bigger than the pass.** `taste_signal::for_account` binds `&str` against `shelves.account_id`, which is UUID on
PostgreSQL. On the live database the extended-protocol form — what the driver actually sends — refuses to prepare:

```
PREPARE old_signal(text) AS SELECT count(*) FROM shelves s WHERE s.account_id = $1;
ERROR:  operator does not exist: uuid = text
PREPARE new_signal(text) AS SELECT count(*) FROM shelves s WHERE s.account_id = $1::uuid;
PREPARE
```

Both call sites caught the error with `_ => ranked`, so **every** discovery and concierge page on the deployed engine ran
unpulled, silently, since M64-09 landed. The layer was not merely inert on PostgreSQL — it had never executed there once, and
the SQLite suite could not see it. Note for the next reader: comparing in `psql` with a bare quoted literal proves nothing
(`account_id = 'deec0d42-…'` is parsed as a uuid literal and succeeds); only the typed-parameter form reproduces it, which is
why the PG test and `PREPARE` are the evidence and a literal comparison is not.

**Not verified live, and why.** The pull's effect needs a signed-in reader *with shelf items* on a gravity-tilted surface, and
the live instance holds `shelves` 1 / `shelf_items` 0 (the shelf feature has writers but no user has used it). So the honest
statement is: the query now prepares and runs on the deployed schema, and `for_account` returns `Ok(None)` for a reader with no
shelf — which is the documented "no shelf, no signal" arm — rather than an `Err`. `journalctl | grep "taste signal unavailable"`
returns 0 for the eight minutes after the restart, which is consistent with the fix and is not by itself proof.

### The leakage review queue had no producer (2026-11-09)

`GET /admin/discovery/leakage` reads `taste_leakage::review_rows`; `record_review`, the only writer of that table, had no caller
in `crates/app`. The queue was permanently empty, so §52.1's view read as "nothing leaks here" when it meant "nobody has ever
looked" — and `leakage.rs`'s module doc listed only `record_payout`/`get_payout` as doorless, which is how the omission survived.

`POST /admin/discovery/leakage` (Operator) is the producer. Three decisions: the audience stays `Operator` and the pseud comes
from the session (`route_inventory` refused `RequirePseud` — the audience table models who may call, and a session with no pseud
gets a 422 naming `pseud`); both vocabularies are validated by name through the functions the view already uses, so a typo cannot
land as `keep`; and the write's answer carries no count, because §52.1's rule is about not publishing the queue's size.

Proof: red first (405), then writer 1/1 and `taste_leakage_view` 8/8 on both engines, `route_inventory` 5/5; mutants removing the
operator check and forcing `Disposition::Keep` both redden their own assertion. Ratchet 44 → 43. `record_payout`/`get_payout`
stay named against the payouts row: a payout is written by a run, and there is no run yet.

#### The leakage-writer deploy was attempted and NOT completed — the deploy tree is another session's workspace (2026-11-09)

`POST /admin/discovery/leakage` is merged, tested on both engines and pushed, but it is **not live**, and the reason is worth
recording in full because it cost the instance twenty minutes of downtime.

The build (16m42s) finished cleanly and the dump verified (392 `TABLE DATA`). The restart then failed: the binary refused the
host's `darkhold.toml` with

```
unknown field `half_life_days`, expected one of `mode`, `gravity_strength`, `signal_weight_mode`, `admin_weight`, …
```

`git status` in the deploy tree explains it — and this time it is not a merge artifact:

```
M  Cargo.lock, crates/app/Cargo.toml, crates/app/src/config.rs, crates/app/src/lib.rs,
M  crates/app/src/routes/mod.rs, crates/app/src/server.rs, crates/db/src/lib.rs
?? crates/app/src/routes/taste.rs, crates/app/src/taste_source.rs, crates/db/src/taste_source.rs
```

Another session is mid-way through a `[taste]` rework (`ADR 0016` taste sources) in the shared checkout: new modules, modified
config struct, and a `darkhold.toml` edited at 21:16 — inside my build window and after the code the build compiled. That config
declares `half_life_days` and `min_evidence`, which only their (unbuilt) struct accepts. My binary was compiled from the tree as
it stood at build start, so it cannot read it; and the previously running binary (19:48) cannot either, because the config moved
under it while it was running.

**Recovery, in order, so the next reader does not improvise:** the previously running binary was restored from
`~/backups/darkhold-replaced-194857.bin` (still refused the config — the config is the problem, not the binary), then the `[taste]`
block was disabled in `darkhold.toml` (backup at `~/backups/darkhold.toml-*.bak`, with a dated comment saying why and what
re-enables it). The unit is `active`, `/health/live` 200, `/api/v1/search` 200, build `0.1.0+7134d2be.dirty`.

**State of the feature:** merged and green (`6d64ae65`), not deployed. `POST /api/v1/admin/discovery/leakage` answers 405 on the
live instance — the path exists for GET only, which is exactly what an undeployed route looks like.

**Lesson, written down because it has now happened three times:** a shared deploy tree means the build is a snapshot of somebody
else's working directory. Before a deploy, `git status` must be empty *and* `darkhold.toml` (untracked, so invisible to that
check) must be older than the last good binary; and after the build, `git status` must still be empty — a file that changed during
the build is a file the binary did not compile. The `.dirty` stamp on the 19:48 build was the first warning of the same thing and
was read as cosmetic.

### A conversation had no lock: the DM doors served any thread to any account (2026-11-09)

`community::is_participant` answers "is this account in this conversation" and had no caller. Both message doors —
`GET` and `POST /conversations/{id}/messages` — took the id from the path and never asked. `list_messages` filters
`conversation_id`, `deleted_at` and blocks but not membership, so any signed-in account that knew or guessed an id could read the
thread; `post_message` read the participant list, but only to check blocks, so anyone could write into one.

The red run printed the evidence in the other account's words. Both doors now ask `is_participant` and answer
`NotFound { resource: "conversation" }` — 404, not 403, because a denial confirms the thread exists and belongs to somebody. The
test asserts the refusal body for a real thread and for an id that was never a conversation is **identical**, not merely the same
status, and that a refused write leaves the thread's message count unchanged.

`list_messages` now documents that membership is the caller's check: a store function cannot decide the refusal's shape, and one
that 403s would leak the same fact to the next caller that forgets.

**Live exposure, measured:** the deployed database holds `conversations 0`, `messages 0`, `conversation_participants 0`. The hole
is real and unexploited, which is why this ships with the next build rather than forcing one past another session's checkout —
but it is the **first item in the queued deploy**, ahead of the leakage writer, precisely because a privacy hole outranks a
missing producer.

Proof: red (200 to a stranger, carrying the other account's message), green on both engines (privacy 1/1, `m70_13_community_doors`
1/1, `route_inventory` 5/5); mutants removing either guard redden their own assertion. Ratchet 43 → 42.

### The tag confirmation state had no door, so no tag counted (2026-11-09)

`tag_confirmation::confirm` writes `work_tags.confirmation`; `gravity_contributing_tags` — read by discovery, tasting and the
concierge when they build a gravity vector — keeps only `reader`/`wrangler` rows. `confirm` had no caller, and the deployed
database measures the consequence: **30,982 of 30,982 `work_tags` rows are `unconfirmed`**. §49.2's "only reader- or
wrangler-confirmed tags move a ranking" had reduced to "no tag moves any ranking", silently, on every surface that reads gravity.

This is the third pass to find the gravity layer starved of input (the taste signal, then the tags themselves). It is the first
where the product's own rule names the missing act, which is why the repair is a door and not a default.

`PUT /works/{id}/tags/{node}/confirmation`, Pseudonymous, `{"state": "reader" | "inaccurate" | "wrangler"}`:

* a reader confirms or flags; **only a curator may claim `wrangler`** — a state is a claim about authority, and `is_active_curator`
  is the same predicate the tag door beside it uses, so the two agree about who a curator is;
* **`unconfirmed` is refused by name** — it is the absence of a judgement, and accepting it would erase somebody's confirmation;
* **the flag records the pseud**, the identity that judged, not the account behind it;
* **a tag the work does not carry is `404`**, the decision the vote door beside it made.

Proof: red first (404, no route), then green on both engines — author-applied counts toward nothing, `reader`-confirmed is
returned by the filter, `inaccurate` stops counting at once and names its flagger. `route_inventory` 5/5. Mutants: wrangler
without the curator check (403 → 200) and ignoring `confirm`'s "pair not found" (404 → 200), both retargeted after their first
versions failed to compile. Ratchet 42 → 41.

Not deployed: queued behind the conversation privacy fix and the leakage writer, in that order.

### §47.4's discount existed in the schema and in no row (2026-11-09)

Migration 0098 added `kind`, `source` and a partial index `WHERE kind = 'earned'` to `work_view_log` for §47.4 ("ranking counts
only `Earned`"), plus `obscurity_at_read` for §47.7. `ranking::record_interaction` was written to fill them, and its **only caller
was a test**. `work_metrics::record_view` — the single production writer — inserted four columns and left the other three at their
defaults, so every row on every instance read `kind = 'earned'`, `source = 'unknown'`, `obscurity = 1.0` no matter how the reader
arrived. A bounty read was stored as an earned one, and the discount could only have been applied to rows that were never written.

Repair: `record_view` takes the arrival `source` and writes `kind` + `source` in the **same INSERT** (kind from
`InteractionKind::for_source`); `record_interaction` and `InteractionRow` deleted, and the round-trip test now drives the real
writer. One write, not two: a second UPDATE of a fact the INSERT already knows is a window in which a crash leaves a bounty read
recorded as earned forever.

`obscurity_at_read` is deliberately **not** written — nothing measures a work's obscurity at read time, and writing `1.0` from the
writer would look like a measurement and mean "not obscure". The note in `ranking.rs` names the consumer half: no query filters
`kind = 'earned'` (the partial index has no user), no query reads `obscurity_at_read`, and `work_kudos` still has two writers that
fill neither column. §47.4 is **decided, recorded per row, and not yet applied** — the first line of any future consumer is the
filter, and until then the columns are evidence rather than policy.

A PG defect fell out of the change: `work_view_log.work_id` is **TEXT** on PostgreSQL (0068 spells it TEXT in both dialects) while
`works.id` is uuid. The PG INSERT carried `$1::uuid` — inert in an INSERT by assignment cast, fatal in a comparison
(`operator does not exist: text = uuid`), which is what the new read-back did. Cast removed; comment corrected, because the old one
taught the wrong thing.

Proof: two mutants from /tmp (writer falls back to defaults; `for_source` never discounts) redden exactly their assertion; both
engines green (m45_ranking 10/10, milestone_68 22/22, work_metrics lib 5/5). Ratchet 41 → 40.

### The entity tree had no write, so the closure was always empty (2026-11-09)

`entity_closure::rebuild` walks `canonical_entities.parent_id`; `descendants_of` — "the `+children` expansion's answer" (§11.4) —
reads the closure and had no caller. The root cause was one link upstream: **nothing in the product ever wrote `parent_id`**.
Measured live: `parent_id IS NOT NULL` on **0 of 58,042** entities, `entity_closure` **0** rows, `entity_edges` **0** rows. The tree
did not exist, the maintenance pass faithfully materialised nothing every ~30s, and `+children` could not expand on any deployment —
not because expansion was unwritten, but because there was no way to say "this tag is a child of that one".

**Door:** `PUT /taxonomy/{id}/parent` (Pseudonymous, curators only, `is_active_curator`), body `{"parent_id": id | null}`.

* writes `parent_id`, **not** `entity_edges(rel='parent')` — both vocabularies exist, but the closure walks the column, and the edge
  would be a second place to say the same thing that nothing reads;
* **cycles refused at the door** — the walk's visited set cannot hang, but a tree cycle is a curator mistake and `422` names the
  field;
* **below the floor is not a node** (§6.4/R-23) — an edge to or from an assertion describes a tree that does not exist;
* `null` detaches; `""` is a malformed id and is refused rather than read as an absence.

**Consumer:** `GET /tags/{tag}?children=true`, off by default. Membership is an `IN` subquery over `[tag] ∪ [descendants]`, not a
join: a work carrying both a parent and a child tag is one work, and a join would duplicate it and inflate `COUNT(*) OVER ()` so the
page total would lie. Paging and total are one result's properties, which is why the multi-tag listing is its own function that the
single-tag one delegates to. An unknown tag with `children=true` answers an empty page, not a 404.

**Harness defect found on the way:** `tag_page.rs`'s `exec` reached `sqlite_pool()`, making the whole file SQLite-only and giving
the `/tags` surface zero PG coverage. Now per-backend; its four tests pass on both engines for the first time.

Proof: red first (404), then both engines — parentage 2/2, tag_page 4/4, route_inventory 5/5. Mutants: no curator check (403 →
200), cycles accepted (422 → 200), `children` ignored (3 items → 2). Ratchet 40 → 39.


## Deploy 2026-11-09 23:11 — the queued ratchet passes reached production

The deploy tree (`/personal/documents/code/rust/darkhold`) went clean and the other session's taste rework was committed, so the
queue could finally ship. Merged `lc/master` into it (`4b81ec4c`, 20 files, no conflicts, **no new migrations**), backed up
`/personal/backups/darkhold-20261109-224441-pre-m70-13-passes-73-76.dump` (1.89 GB, verified `pg_restore -l | grep -c 'TABLE DATA'`
= **392**), built release in 15m16s, `migrate` reported `197 applied, 0 pending`, restarted, `/api/v1/meta` now says
**`0.1.0+4b81ec4c`**.

Pre-flight worth recording, because it was the trap that took the site down on 2026-11-09 earlier: `darkhold.toml` mtime 21:45:27
was **newer** than the installed binary (21:43:56). The discriminator was the unit's `ActiveEnterTimestamp` — 21:45:33, six seconds
*after* the config edit — so the running binary had already accepted that config, and `migrate --status` from the tree confirmed
the file parses (`197 applied, 0 pending`). Config newer than the binary is only dangerous when the service started before the
edit.

**What is now live, probed rather than assumed:**

| surface | before | after |
|---|---|---|
| `PUT /api/v1/taxonomy/{id}/parent` | 404 (no route) | **401** anonymous — exists, curator-gated |
| `PUT /api/v1/works/{id}/tags/{node}/confirmation` | 404 | **401** |
| `POST /api/v1/admin/discovery/leakage` | 405 | **401** |
| `GET /api/v1/tags/{tag}?children=true` | unknown param | **200** |
| `work_view_log.source` on a real read | `unknown` (1,682 of 1,682) | **`direct` / `kind=earned`** on the first post-deploy row |

That last row is the one that matters most: the ranking producer wrote its first truthful row in production. Every historical row
says `unknown` because the writer never recorded an arrival path; from this build on, a row says how the reader got there, which is
the input §47.4's discount needs.

`strings` on the artifact was checked **before** the restart (`taxonomy/{id}/parent`, `admin/discovery/leakage` each present once),
so the binary that shipped is the code that was built — the failure mode that put a three-commit-old binary in front of the unit on
2026-11-08.

**One loose end for whoever re-enables it:** the `[taste]` block in the host's `darkhold.toml` is still commented out with a dated
note (it was commented out on 2026-11-09 when an older binary rejected `half_life_days`). The binary deployed now is built from a
tree that **contains** that struct, so the block would parse again — but re-enabling it changes live taste behaviour and that is the
other session's feature, so it stays off until they say otherwise. The trees are not identical by design: the deploy tree carries
their 25 commits, my line carries the ratchet passes, and only the deploy tree has both.

### The audience panel read a table nothing wrote (2026-11-09)

§41.2's warmth rule exists in full — `warmth_delta`, `tier_for`, `record_warmth`, `GET /me/audience` — and the writer had no caller.
Measured live: `interaction_warmth` **0 rows**, so every author's panel answered five zeroes, indistinguishable from "nobody reads
me" on an instance with 58,042 works.

Wired **React** (kudos, both directions) and **Comment**. `warmth::thresholds` now reads `CommunityConfig::warmth_thresholds`, which
was a JSON blob nothing parsed, with a per-key fallback that logs which key it could not use. A reader cannot warm themselves. A
relationship that lands at ≤ 0 bp is **deleted** rather than left as `lurk`, so the un-kudos is symmetric and `total` means "currently
warm" rather than "ever interacted".

**Defect caught by probing rather than by reasoning:** the comment route passed `&id` after `let id = insert_comment(…)` had
shadowed the path parameter with the *comment's* id. Both are uuids, so `WorkId::parse` accepted it, the author lookup answered
`None`, and the panel stayed empty with no error anywhere. Fixed to `&work_id`, reason recorded at the call.

**Named halves, not shipped:** `ReadUnit` (needs `save_progress`) and `FinishWork` (needs `reading_status`) have no producer yet; both
need the same reversal discipline as the kudos toggle. Until they land the panel shows `react` and `comment` tiers only.

Proof: red first (all zeros after a kudos), then both engines — kudos → `react:1`, un-kudos → `total:0`, three comments (1200 bp) →
`comment:1`, author's own comment ignored. Mutants: comments recording nothing; self-warming allowed (`total:2`). Ratchet 39 → 38.

### Deployed 2026-11-10 00:03 — `0.1.0+981a8e64` (warmth producer)

Merged `lc/master` (no new migrations, 197 applied / 0 pending), backup
`/personal/backups/darkhold-20261109-233903-pre-warmth.dump` verified at **392 TABLE DATA**, release build 17m13s.

**Live proof, not a probe of the route's existence:** a throwaway reader kudos'd a public work and the database then held its first
ever warmth row — `warmth_bp = 200, tier = react`, author resolved through the pseud. Before this build, `interaction_warmth` had
**0 rows** on an instance with 58,042 works. The throwaway account, its kudos and its warmth row were then deleted: a synthetic
relationship must not become product signal.

`strings` on the artifact was checked before the restart, and `migrate` ran from the tree (the binary finds `darkhold.toml` by
working directory).

### `FinishWork` records the act, not the state (2026-11-10)

`PUT /works/{id}/reading-status` now reads the prior status and records 500 bp only on entry into `finished`; leaving it subtracts.
The tier is the proof point: one finish is `react` (500 ≥ 200), two would be exactly `comment` (1000), so a state reader is visible
from outside without exposing basis points.

Still named: **`ReadUnit`** needs a per-unit "already counted" fact (`save_progress` stores a position, not a set of read units) — a
migration plus a writer, not a route edit. And the **library-item status door** records nothing: it addresses an item whose work is a
`source_work_key` string, not a `works.id`.

### Deployed 2026-11-10 00:59 — `0.1.0+99e11533` (FinishWork), and a foreign break found by building

Merged `lc/master`, backup `/personal/backups/darkhold-20261110-002223-pre-finishwork.dump` verified at **392 TABLE DATA**,
release build 13m47s.

**The build failed first, and the failure was not mine.** `polaris-oidc-client` moved to 0.2.0 at 23:54:47 — "the id_token is
verified, not read" — removing `verify_claims`; `crates/app/src/routes/oidc.rs` still imported it, so every fresh build of the tree
could not compile. Nobody was mid-build, so this was a real estate break, not a race: the crate's commit message says "three sites",
and darkhold was the site that had not moved. Adopted `Provider::verify_id_token(raw, nonce)`, which checks the **signature** against
the provider's published JWKS as well as `iss`/`aud`/`exp`/nonce — the old code read the payload and compared three fields, which a
token written by anything on the wire would satisfy. The same `Provider` value that exchanged the code holds the key set, so the
JWKS is fetched once.

`Cargo.lock` was then refreshed and committed (`a39da609`): without it a fresh build rewrote the lock and the binary stamped itself
`+99e11533.dirty`, a version string that says the artifact matches no commit.

**Live proof:** a throwaway reader finished a public work and `interaction_warmth` held `warmth_bp = 500, tier = react`; re-sending
`finished` left it at 500 rather than 1000, which is the transition rule visible from the database. Account, status row and warmth
row deleted afterwards.

### Deployed 2026-11-10 01:48 — `0.1.0+ed82e4f0` (clean stamp)

Merged `lc/master` (passes 78–79 plus the oidc adoption and the lockfile), release build 14m19s. The stamp carries no `.dirty`
suffix now that `Cargo.lock` is committed — the version string matches the tree, which is the only reason to read it.

Live: `/health/live` 200, `/api/v1/notifications` and `/api/v1/me/audience` 401 (present, session-gated), `migrate` 197/0. Earlier in
this deploy the same tree failed to compile at all because `polaris-oidc-client` 0.2.0 had removed `verify_claims`; that adoption is
in this build, so the deployed artifact verifies the `id_token` signature instead of reading its claims.

`gate.sh repo`, `gate.sh ledger` and `gate.sh design` are all `ok` on the dev tree at `78d00579`.

### Quality signals: the filter dimension nothing could write (2026-11-10)

`min_quality` and `quality:<kind>:<min>` compiled, the "Hidden gems" surface declared `quality_signals` a data need, and the table
held **0 rows** on a 58,042-work instance: the only writer in the tree was a test fixture. Every quality filter answered "nothing".

Now: `POST /admin/works/{id}/quality-signals` (operator judgement, `source = operator:<account>`, replacing that operator's previous
row for the same kind) and `GET /works/{id}/quality` (public components + the composite computed with the **filter's own formula**).

Named next half: `completeness` and `reader_positivity` are derivable from stored facts, but their mappings are policy — a migration
row each with a documented mapping, not a route that invents a curve. `editorial_review`, `quorum_distinction` and `maturity` stay
operator/quorum acts by definition.

### Deployed 2026-11-10 02:38 — `0.1.0+fb451f68` (quality-signal door)

Backup `/personal/backups/darkhold-20261110-023836-pre-quality-signals.dump` verified at **392 TABLE DATA**; release build 15m12s;
`migrate` 197/0.

Live, and stated exactly: `GET /api/v1/works/{id}/quality` answers `{"composite": null, "signals": []}` — the honest empty shape, not
a fabricated average — the door answers `401` to an anonymous caller, and `min_quality:750` still returns `total: 0` **because no
operator has scored anything yet**. The door is what shipped; the catalogue stays unscored until an operator uses it or the derived
kinds (`completeness`, `reader_positivity`) land with their documented mappings. Claiming the filter is "fixed" would be claiming a
door is a population.

### `reader_positivity` is real; `completeness` is not a signal (2026-11-10)

`reader_positivity` = `round(1000 * positive / visible)` over `positivity::visible_reviews`, recomputed on every review write and
deletion. No visible reviews deletes the row: a `0` would claim a verdict nobody reached.

`completeness` is dropped as a derived kind: `completion:complete` is already a filter, so the signal would be the same fact twice
with an invented curve attached.

**Open decision (not taken in passing):** `"This is awful and boring. I hate it."` classifies **Positive** — it matches no
`HOSTILE_MARKERS` phrase and falls through to `non-critical-engagement`, so the §12 gate delivers generic hostility to authors.
Extending the marker list is policy (adding `awful`/`boring` would hold "the villain was awful", which is praise), so it needs a
stated rule and its own tests.

### Deployed 2026-11-10 03:39 — `0.1.0+e56152f8` (reader_positivity)

Backup `/personal/backups/darkhold-20261110-0320xx-pre-reader-positivity.dump` → resolved: the file is
`darkhold-20261110-0319xx-pre-reader-positivity.dump`; verified **392 TABLE DATA** (3.74 GB). Release build 15m25s; `migrate` 197/0.

**Live, through real HTTP calls on the deployed instance:**

1. a throwaway reader published a review on a public work →
   `quality_signals` held `reader_positivity | 1000 | 1 | computed`, and `GET /works/{id}/quality` answered
   `{"composite": 1.0, "signals": [{"kind": "reader_positivity", "value": 1000, "source": "computed"}]}`;
2. `GET /media?q=min_quality:750` returned **total 1** — the first time that filter has ever returned anything on this instance;
3. deleting the review through the API removed the signal row (`0` rows) and the composite returned to `null`.

Cleanup: the throwaway account, its pseud, session, the review, its classification and the author's notification were deleted.

### The gate that delivered hostility as praise (2026-11-10)

The classifier's `non-critical-engagement` default was `Positive`, so text with no marker — `"This is awful and boring. I hate it."` —
was delivered to authors as a review and counted as praise by `reader_positivity`. Now: a verdict about the **work** with **two
distinct dislike markers**, no craft-specificity and no negation is `Negative` (held by the gate, not counted). One harsh adjective stays
engagement; craft-specificity outranks the verdict; negation is filtered in the twelve characters before a marker.

Also fixed: `cargo test -p darkhold-domain` was red because `taste_notifications` said `default_on: false` in its registry row and was
missing from `OPT_IN` — prose and checker disagreed, and the list was the missing half.

Not fixed, and stated as a limit: **stored classifications keep their old verdicts.** Re-classifying history is a data migration with
its own reversible rule; the fix changes what happens next, not what happened before.

### Deployed 2026-11-10 04:12 — `0.1.0+a1bb450a` (the dislike class)

Backup `/personal/backups/darkhold-20261110-0357xx-pre-classifier-dislike.dump` → verified **392 TABLE DATA** (3.74 GB). Release
build 16m25s; `migrate` 197/0.

**Live proof, through the API on the deployed instance:** a throwaway reader posted `"This is awful and boring. I hate it."` and the
receipt was `Comment held for moderator review.` — with `GET /works/{id}/quality` still answering `{"composite": null, "signals": []}`.
Before this build that same text was `Positive`: delivered to the author, and counted as praise by `reader_positivity`. The same pseud
then edited it to a positive review, got `Comment posted.`, and the signal appeared as `reader_positivity | 1000 | computed` with
composite `1.0`.

Cleanup: review, its classification, the computed signal, the author's notification, the throwaway account and pseud were deleted (0/0/0
verified).

**Limit, stated rather than papered over:** reviews classified before this build keep their stored verdicts. Re-classifying history is a
data migration with its own reversible rule.

### `ReadUnit` — the last of §41.2's four acts (2026-11-10)

"Read" is the reader's own fact: their saved position reached the end of the unit. New table `reader_unit_completions` (account, unit)
distinct from `reading_unit_read` (pseud, first *open*, consumed by reading goals) — different scope, different act. `save_progress`
answers `bool`; both doors credit through that one answer.

**Next pass, from a red gate rather than a plan: `gate.sh dialects` fails on 14 pre-existing defects in 9 files** — `tag_scripts`,
`content_policy`, `experiment_decisions`, `trope_clusters`, `experiment_snapshots`, `experiments`, `gravity_prefs`, `shelf_follows`,
`source_engagement`: uncast placeholders that pass on SQLite and fail on PostgreSQL. Each needs a failing PG test before the cast, not
just the cast.

### Deployed 2026-11-10 10:54 — `0.1.0+e1fdb38f` (ReadUnit)

Backup `/personal/backups/darkhold-20261110-1037xx-pre-readunit.dump` → verified **392 TABLE DATA**. Build 13m55s. Migration `0201_reader_unit_completions` applied (`migrate` printed it), 198 applied / 0 pending.

**Live proof, through the API on the deployed instance:** a throwaway reader saved a position at 500 permille → `reader_unit_completions` 0 rows, warmth 0; saved at 1000 → 1 completion row and `interaction_warmth` 100 bp; saved the same unit at 1000 again → still 100 bp. Cleanup deleted the completion, warmth, progress, unit-read, session, pseud and account rows (0/0/0 verified).

### `gate.sh dialects` — 14 sites, 12 real defects and 2 gate bugs (2026-11-10)

The 0172–0178 family binds `now_rfc3339()` (a `String`) into `TIMESTAMPTZ` columns: a bind SQLite accepts and PostgreSQL refuses with
`column "created_at" is of type timestamp with time zone but expression is of type text`. Proven on the server before the fix and after
it. Twelve sites fixed by casting (`::timestamptz`, `::uuid`, `::bigint`, `to_char` for a timestamp read into a `String`).

The other two were the checker's: a qualified column attributed to every table sharing the column name, and a SELECT list cut at the
first literal `FROM` so a subquery's `ORDER BY` was read as a projection. Both fixed with three self-test cases, because a gate that
reports correct code is a gate the next pass ignores.

**Standing rule, now proven twice:** the suite count is not the coverage. 86 files claim dual-backend coverage; 385 make no claim, and
every defect in this pass lived in the 385.

### Deployed 2026-11-10 11:45 — `0.1.0+544e9503` (dialect casts)

Backup `/personal/backups/darkhold-20261110-111419-pre-dialect-casts.dump` → verified **393 TABLE DATA** (one more than last time: `reader_unit_completions`). Build 18m26s (concurrent with another session's build). `migrate` 198/0.

**Live proof:** a throwaway account created a tag script — `POST /api/v1/tag-scripts` → **201** with `updated_at: "2026-11-10T10:44:01Z"`, and `GET /api/v1/tag-scripts` → **200** decoding the same value. Both halves were PostgreSQL 500s before this build: a `String` bound into `TIMESTAMPTZ` on the write, a `TIMESTAMPTZ` read into a `String` on the read. Row and account deleted after (0/0 verified).

**Still red in `gate.sh dialects`, and it is the next pass:** `find-single-backend-suites.py` lists ~20 test helpers that reach `sqlite_pool()` — `m69_03_passport`, `m69_09_affinity`, `m69_14_live_badge`, `m69_18_trope_coverage`, `m69_19_npmi`, `m70_01_orphan_route`, `m70_02_whole_work`, `m70_04_restricted`, `m70_05_collection_tags`, `m70_07_marked_for_later`, `m71_04_provenance`, `m71_11_facet_cache`, `m71_16_machine_surfaces`, `work_page`, `m70_12_visibility_surfaces` — each makes its whole file SQLite-only, which is where the defects above lived.

### Fourteen suites now run on PostgreSQL (2026-11-10)

`check-pg-arm-uses-sqlite-pool.py` had 43 findings; 25 fixed here (14 files converted and green on both engines, plus the helpers that
made the rest mechanical). What remains is 18 sites in `landing_page`, `m16_*`, `m64_*` and `m69_03_passport`.

**Opening finding for the next pass:** with `m69_03_passport` ported, `GET /me/passport` answers **500 on PostgreSQL** — a product
defect the SQLite-only harness had hidden. The suspects are the three reads between the handler and the document: `taste_bp`,
`contribution_counts`, `reader_affinity::for_account`.

**Standing rule:** a fixture id must be a valid UUID, and `randomblob` has no PostgreSQL twin in a seed — both only fail on the second
row, which is why replacing them with literals "works" until the fixture has two rows.

### The passport route's three PostgreSQL 500s (2026-11-10)

Found by porting its suite, not by reading the code — and it took four attempts because each fix disclosed the next defect. The three
real ones: an uncast account bind into `bookmarks.account_id` (UUID); **`LIMIT ?` left in the PostgreSQL arm**; and a join of
`pseuds.id` (UUID) to `comments.author_pseud` (TEXT).

Rule worth carrying: against the live database a *literal* `'<uuid>'` is coerced, but a **bound** text parameter is not — so this class
of defect is invisible to `psql` inspection and to every SQLite suite, and appears only when a client binds. Deployed
`0.1.0+85786ea7`; the module is closed by default and answers 404, not 500.

**Remaining in `gate.sh dialects`:** 17 sites (the 18 from the last pass minus `m69_03_passport`, whose fixture is now dual-engine).

### The dialects gate is green (2026-11-10)

Seventeen sites, nine files, one shape: a fixture helper that reached the SQLite pool directly, which made the whole file a
SQLite-only suite because every test went through that helper. All converted and proved on both engines.

Three defects the static gate cannot see, found by running those suites on PostgreSQL: `SUM(...)` is NUMERIC (cast to BIGINT);
a fixture wrote `'a'` into `units.order_key`, which is BIGINT on the deployed engine; and **`/discovery`'s crosspost-union slot
answered 500 every time it ran**, because `min(uuid)` does not exist in PostgreSQL — the cast has to go inside the aggregate, not
after it.

Gate defect found in passing: `cargo fmt --all` also formats **path dependencies**, so an edition-2024 dependency made the fmt
check permanently red on a file this repository does not own. The step now walks workspace members one at a time.

**Next:** the road-map order falls through to M45, M66 GS-1..GS-5, M69, M63, M64 and M71 rows, with the M70-13 ratchet at 36
orphans as the fallback.

### M45-39 — mutual backup is a recorded assertion, and the document stops lying about it (2026-11-10)

The row had two halves built (the review-window switch, the redirect registry) and one that `GET /succession` itself
confessed was missing: "mutual backup with a peer instance". That confession was a hardcoded string, which meant the
document would keep saying the half was missing on the day an operator recorded one — a document that lies in the
direction of modesty is still lying.

`succession_mirror` (migration 0202) records the peers an operator asserts hold a copy; `PUT
/admin/succession/mirrors` is an upsert on the peer id (a second PUT is a correction, not a second copy);
`DELETE /admin/succession/mirrors/{peer}` withdraws it and answers 404 to a peer nobody recorded. The public
document carries `mirrors: [{peer_id, export_url, asserted_at, verified}]`, and `not_built` is computed from the
rows.

`verified` is always `false` and is emitted anyway: this instance records an assertion and cannot reach into a
peer's disk, so the field says what a reader would otherwise assume. An omitted field reads as a yes.

**The boundary, decided and recorded**: acting on a lapsed plan — a successor publishing the export and taking the
redirects live — belongs to the successor's instance. Darkhold federates inbound only, so that half is theirs;
`ActivityPub account Move` stays in `not_built` unconditionally rather than becoming a false claim.

Proof: `m45_39_succession` 7/7 on both engines, including the two assertions that pin the confession (absent with no
mirror, gone with one, back after withdrawal); mutant `has_mirror = false` reddens exactly that step.

### M45-40 — the lens is an order, and the operator has none (2026-11-10)

`curator_lens` (migration 0203) is one row per account: a person has one view, and publishing twice is that view
edited. `PUT /lens/{handle}` needs the rec-blogger role and the caller's own handle; `GET /lens/{handle}` is public
and 404s for a lens nobody published; `GET /lens` lists them.

Two decisions worth keeping. **The candidate set is search's, not the lens's** — `ranked_for` calls
`search_works_ast_scoped` and only re-orders what search already agreed to show, so visibility and blocked tags keep
one owner. **The `basis` field exists because an order without a reason is the thing §0.3 forbids** when the operator
does it: `taste` when the curator's kudos overlap the candidates, `catalogue` when they do not, ties keeping the
catalogue's position.

The operator's lens is refused on the write door (403), the read door (404) and the list (filtered). The read-door
refusal is proved by seeding the row through the store, because the write door never lets one exist — a guard tested
only through a door that refuses first is a guard nothing reaches.

Proof: `m45_40_curator_lens` 2/2 on both engines. Mutants, each reverted from `/tmp`: write-door rule removed
(204 instead of 403), read-door rule removed (200 instead of 404), ordering removed (catalogue order instead of
3/2/1). `route_inventory` 5/5.

### M45-41 — a share pays both sides once, and the second guard was not a guard (2026-11-10)

`POST /works/{id}/share` mints a code from `(account, work)` — derived, so two racing POSTs cannot mint two codes for
one work and pay the same reader twice. `GET /works/{id}?ref=CODE` pays one credit to each side into the `share`
bucket, gated by the transaction's idempotency key `share:<code>:<visitor>`.

The finding of this row: the first version had **two** guards — an early return on `share_visit`'s insert and the
transaction key — and the mutant that deleted the early return **passed**. The key was doing all the work. The
redundant guard is gone, its comment says why, and the surviving mutant is the one that proves the real gate
(random key → a refresh pays twice).

Refusals: a code for another work attributes nothing; a sharer following their own link earns nothing; an anonymous
visitor earns nothing for anybody. Attribution never fails the page.

Proof: `m45_41_share_attribution` 1/1 both engines; mutants (unique credit key, self-attribution allowed) reddened;
`route_inventory` 5/5.

### M45-42 — claim your works: plan (written before the code, per the standing rule)

**The problem.** The catalogue holds ~58k works imported as metadata; their authors are not accounts here. A claim
lets an author take ownership of metadata rows that are already about them, without an operator guessing.

**The proof of authorship.** A token the claimant places in their AO3 profile, because the profile is a page the
author already controls and AO3 already verifies. Nothing in this instance can be forged into it.

**Shape (decided):**

1. `POST /claims` (session, `{works: [ids], profile_url}`) → `201 {claim_id, token, instructions}`. Token is
   `dh-claim-<24 hex>`, minted once per (account, profile_url); a second claim reuses it, so an author who opens the
   page twice does not have two tokens to paste.
2. `POST /claims/{id}/verify` (session, owner-scoped) → fetches `profile_url`, requires the token verbatim, then
   attaches every work in the claim whose `owner_pseud_id` is the metadata placeholder and which is not in the
   exclusion registry. Answers `{claimed: n, refused: [{work, reason}]}` — a claim that attaches nothing must say why
   per work rather than "0".
3. `GET /claims/{id}` → the claim's state (`pending`/`verified`/`refused`) and what it would attach, so an author can
   see the outcome; owner-scoped, 404 to a stranger (a claim id is not a capability).

**Refusals, each a different sentence:**

* a work already owned by a real account → `refused: "already_claimed"`, never silently skipped;
* a work in the exclusion registry (`is_excluded`, the pass-42 door) → `refused: "excluded_by_author"`;
* the token absent from the profile → `422` naming `profile_url`, and the claim stays `pending` (fetching a page that
  does not carry the token is not a failure of the claim, it is a visit that proved nothing);
* a second account verifying a claim whose works are already claimed → `409` with the per-work list.

**The half that does not exist yet, named here:** there is no mailer and no background fetcher in this instance.
`verify` fetches the profile **at request time** and only the one URL in the claim; it does not crawl, and it does not
retry. If the fetch fails, the claim stays `pending` and the answer says `fetch_failed` — a claim is not refused
because AO3 rate-limited us.

**Test file**: `crates/app/tests/m45_42_claim_works.rs`, both engines, with the profile fetch behind a trait so the
test answers with a canned body instead of reaching the network. The trait is the seam: `ProfileFetcher` with one
method, a real implementation for the app and a canned one for the suite — the claim logic is what the row is about,
not the HTTP client.
### M45-42 as built (2026-11-10) — what the plan got right, and the two places it was wrong

Implemented in `f7136141`: migration `0205_work_claims.sql` (`work_claim` unique on `(account_id, profile_url)`,
`work_claim_work` with a NULL-able `outcome`), `crates/db/src/claims.rs`, `crates/app/src/routes/claims.rs`
mounted as the swappable module **`import_claims`** (default on: an imported catalogue whose authors cannot claim
their own works is a catalogue of orphans).

**Where the plan was wrong, and the code is right:**

* The plan promised a `409` when a *second account* verifies works somebody already claimed. There is no such door and
  there should not be: `verify` is owner-scoped, so the second account can only ever verify **its own** claim, and the
  works in it come back `already_claimed` in the per-work list. A `409` would have been a second way to say what the
  outcome list already says, and it would have had to name another account's claim — which is exactly the disclosure
  §0.3 forbids.
* The plan said a failed fetch would answer `fetch_failed`. It answers `422` naming `profile_url` with a *different
  message* from "the token was not found", and the claim stays `pending`. A `state: "refused"` would have made an
  AO3 timeout permanent in the author's eyes.

**One definition of "a metadata row":** `is_metadata_work` reads `works.owner_pseud_id → pseuds.account_id =
SYSTEM_ACCOUNT`, the same fact `crates/db/src/orphan.rs` reads. Two definitions of "nobody here wrote this" would
drift, and the drift would decide who may claim a work.

**The token is derived, not random** — `sha256(account \u{1f} profile_url)`, so a second visit to `POST /claims` returns
the same string. A random token per request would make every earlier paste look stale and give the author two strings
to keep with no way to know which one the instance is watching for.

**Proof:** `m45_42_claim_works` 2/2 SQLite, 2/2 PostgreSQL; mutants (exclusion lookup removed → the excluded work is
attached; the owner check removed → an owned work is taken) each reddened the intended assertion, reverted from /tmp;
`route_inventory` 5/5; `gate.sh repo` and `gate.sh ledger` ok.
### Deployed 2026-11-10 22:22 — M45-39/40/41/42 on thinkcentre (`0.1.0+f7136141`)

One batch, one dump, one verification pass, as the batching decision required.

**Backup:** `/personal/backups/darkhold-20261110-205002-pre-m45-39.dump` — 3,740,150,855 bytes, written 20:50→21:18, **393 `TABLE DATA` entries** per `pg_restore -l`, i.e. taken at schema 0201 and therefore before any of 0202–0205 was applied. The order matters and was checked: dump finished 21:18, build finished 22:22, migrate ran after that.

Measured after the restart:

* `_migrations` max version **0201 → 0205**; the six new tables exist (`succession_mirror`, `curator_lens`,
  `share_code`, `share_visit`, `work_claim`, `work_claim_work`).
* `/api/v1/meta` reports `0.1.0+f7136141` — the artifact that was built, not the previous binary.
* `strings` on the installed binary: **2** hits for `admin/succession/mirrors`, 1 each for `/lens/`, `works/{id}/share`,
  `claims/{id}/verify`. A merged-but-unbuilt route has 0 here whatever the build log said.
* `GET /succession` → `{"plan":null,"mirrors":[],"not_built":["mutual backup with a peer instance","ActivityPub
  account Move"]}` — the mirror half is still confessed because no mirror is recorded, which is the computed answer, not
  the old hardcoded one (a PUT would remove that line).
* `GET /discovery` 200 (public), `GET /lens` 200 (public list), `GET /lens/nobody` **404**, `PUT /lens/x` **401**,
  `POST /works/{id}/share` **401**, `POST /claims` **401** — the anonymous refusals are auth-before-validation by design.

**Two deploy traps cost the time this batch, both recorded in the deploy skill:** `pkill -f "/tmp/deploy45.sh"` inside an
ssh one-liner matches the ssh server's own command line and kills the session (exit 255, no output, nothing executed), and
a `setsid nohup` build died mid-`Compiling` with no OOM and no cargo process, leaving a log-waiter polling a log that would
never say `Finished`. The build now runs in the foreground of one tracked ssh that prints its exit code.
### M63-18 — config profiles: one user, two instances (2026-11-10)

`crates/cli/src/profiles.rs`, `--profile`/`DARKHOLD_PROFILE`, and `dh config path|get|set`. The file is
`~/.config/darkhold/config.toml` (or `$DARKHOLD_CONFIG`), `0600`, holding `default_profile` and
`[profiles.<name>] url/token/cookie`.

**One resolution order, in one function:** `--flag` > `DARKHOLD_*` (clap folds the environment into the flag) >
the selected profile > the built-in default. Two places deciding which instance is being talked to is how a token
reaches the wrong server, and the failure then looks like a server bug.

**Decisions worth keeping, each in a comment beside the code:**

* **`--base-url` beats the profile's URL but keeps the profile's token.** Pointing one command at a staging instance
  is about *where*, not *who*; dropping the token because the URL was overridden would answer `401` for a reason the
  reader never asked about.
* **An unknown profile is a named error, exit 2, with the file path and the list of profiles.** Falling back to the
  default would send the *other* instance's token to the *other* instance — the exact failure profiles exist to
  prevent — and it would do it silently.
* **A token read back is a description, never a value:** `set (hidden, N chars)`. A prefix is what a screenshot leaks,
  and the length plus the profile name is what tells two entries apart. `dh config set` on a secret echoes only
  `set (hidden)` so the value never lands in shell scrollback.
* **`config` is dispatched before resolution**, because it edits the file resolution reads. Otherwise fixing a broken
  profile would be refused by the broken profile.
* **`--base-url`'s clap default became the empty string** ("not given") so a profile can supply the URL. Two `rec`
  tests asserted the old parser default; they now resolve the way the binary does, which is the value that matters.

**Proven on the built binary, not only in unit tests:**

```text
$ dh config set profiles.work.url https://work.example
$ dh config set profiles.work.token supersecret-token-value
$ dh config get profiles.work.token          → set (hidden, 23 chars)
$ stat -c %a ~/.config/darkhold/config.toml  → 600
$ dh --profile work module off arena
would PATCH https://work.example/api/v1/modules/arena {"enabled":false}
$ dh --profile work --base-url https://staging.example module off arena
would PATCH https://staging.example/api/v1/modules/arena {"enabled":false}
$ dh --profile nope module off arena
no profile named `nope` in …/config.toml (profiles: work)     [rc=2]
```

`cargo test -p darkhold-cli` green (98 unit + seven integration suites); mutant making `resolve` load the file and then
ignore the profile reddened both precedence assertions, reverted from `/tmp`.

**The first full run went red for a reason worth keeping:** `crates/cli/tests/exit_codes.rs` spawns the real binary and
removed only `DARKHOLD_TOKEN`/`DARKHOLD_COOKIE`, so an exported `DARKHOLD_CONFIG` carrying a profile with a cookie changed
what the test measured — `a_local_refusal_that_needs_a_session_is_four` failed while the code was correct. A test that
inherits the developer's config file is not a test. The helper now removes `DARKHOLD_CONFIG`, `DARKHOLD_PROFILE` and
`DARKHOLD_BASE_URL` too, and the suite was re-run **with the profile environment still exported** to prove the isolation:
7/7 suites green.

**Not built, named:** no `dh config unset`, no per-profile `name`/`description`, and the token is not encrypted at rest —
the file's `0600` is the boundary, the same one `ssh` uses.
## Blocked rows, and the queue that results (2026-11-10)

A blocked row is demoted, never waited on: it goes to the back, the block is recorded here, and the next
unblocked row starts in the same turn. The loop pauses only when **every** remaining `planned` row is on
this table. The rule and the three legal kinds of block are in `~/secondbrain/10-Projects/darkhold/GOAL-2026-11-08.md` §2.1.

| row | kind | what would clear it | taken instead |
|---|---|---|---|
| M45-43 — Discord bot (rec commands, per-fandom feeds, account linking) | **credential** | a Discord application's bot token plus an account to link for the live proof. Its offline half is *not* blocked: the command surface and the rec formatting can ship behind a transport trait, exactly as `M45-42` did with `ProfileFetcher` — do that first, then leave the token-dependent half named in the ledger | M45-46 |
| M63-05b — `dh rec --link epub` | **cleared 2026-11-11** | the download route now exists (`GET /derivatives/{id}/download`, see the section below); the CLI wiring is the only work left, so the row is unblocked and back in the queue. `output.rs` already fixes the link shape; what is missing is the endpoint, so the flag refuses with its reason rather than printing a 404 with a file extension | M63-15 |
| M63-33 — reconcile the live database with the workspace migrations (11 tables absent, 12 legacy-named present) | **reserved decision** | the operator's per-table call: the live database was migrated by the other lineage, whose numbers collide with this tree's, so this is a DDL decision over live data with data-loss options. Not a session's call | M63-15 |

**De-facto queue after those demotions** (each row still read before it is implemented; this is an order, not
a shortcut):

1. **M45-46** — preview before apply. Measured 2026-11-10: the config history *and* rollback already exist
   (`instance_taste_settings::history`/`rollback`, both with routes), so the missing half is only the
   **view-as persona preview** of discover/search under a proposed config. Note the shape it must take: the
   preview may not be a second ranking path (`get_discovery` is the only feed builder, and a twin would drift
   from it); extract `feed_for(persona, overrides)` and have both doors call it.
2. **M45-45**, **M45-52**, **M45-54**, **M45-55** — taste as a versioned blendable object; then the three
   compliance rows, which are one product (notice-and-action, statement of reasons, the RPF/Spain knob).
3. **M63-15, M63-22, M63-23, M63-24, M63-25, M63-26, M63-29, M63-30, M63-32, M63-05b*, M63-33*, M63-11,
   M63-12** — the CLI block. `dh doctor`, `admin explain-rec`, `admin backup`, `bench`, `audit`, `stats`,
   `tui`, calibre, the fandom-tag backfill, then the two `*`-marked demoted ones, then `lib tag --from-csv` and
   `watch`. Cheapest rows in the ledger: one verb plus a golden-output test each.
4. **M66 GS-1..GS-5**, then **M69-23/15/24**, then **M64's eight**, then **M71's ten**, then **M73-01..04**,
   then M29-08 + M44-02 as one product, then **M70-13** (36 orphans at the ratchet).

### M45-46 — the knobs had no reader (2026-11-10, half built; the preview is the missing half)

The row asks for three things: a view-as persona preview of discover/search under a proposed config, config history,
and rollback. Measured before writing anything: **history and rollback already exist** (`instance_taste_settings::history`
/ `rollback`, both with routes since M45-45a), and the stored knobs were read by **nothing on a reading path** — the
feed took its strength and mode from `darkhold.toml`, so the console's numbers described a change that never happened.

That second measurement decided the order: a preview of an ineffectual change is a fiction with a UI. So the shipped half
is the reader, `crates/app/src/taste_knobs.rs`:

* **stored beats config** — the same precedence the admin read door already documents ("a stored dimension set wins over
  the config"); two rules for the same two sources would be a bug waiting to be reported;
* **the unit is basis points of `gravity_ceiling`** — the column is an `INTEGER` with no unit comment, so the unit is
  stated once, in that module, where both the write door and the read path can point at it: `10_000` = the ceiling,
  `750` = `0.075`;
* **out-of-range is unset, not clamped** — a `-250` or a `99999` was written by something that did not share the unit, and
  clamping turns that into "off" or "maximum" while hiding the mismatch; the config applies and the value is logged;
* **a failed read is not "no settings"** — it falls back to the config and logs, because a database that cannot answer is
  not an instance that has no taste.

Wired into `GET /discovery`: `gravity.instance` and `gravity.mode` are now the values actually in force, and the row's test
reads the public door rather than a private helper.

**Still missing, named in the ledger:** the preview door. It needs the feed builder extracted from `get_discovery` into a
function taking `(persona, overrides)` — a second ranking path would drift from the one readers get, which is the failure
the whole milestone is about. `taste_knobs::effective` is that seam's first half: the concierge (`home.curated`) and the
gravity dial still read `state.config().taste` directly and should adopt it in the same pass.

**Found while proving it, and recorded rather than fixed:** two M29 tests
(`a_promoted_surface_runs_the_algorithms_the_experiment_installed`,
`a_held_surface_is_skipped_and_a_hand_promotion_is_recorded_as_manual`) answer `500 INTERNAL` on PostgreSQL. They fail
identically with this change stashed, so they predate it; `docs/known-gaps.md` names them.

**Proof:** `the_stored_knobs_reach_the_feed_they_are_supposed_to_shape` — green on SQLite and PostgreSQL; the full
`m29_transparency` suite 49/49 on SQLite; mutant that reads the row and then discards it reddens the mode assertion
(`taste_weighted` vs `balanced`), reverted from `/tmp`. Live database holds **0** settings rows, so no behaviour changes
on the instance until an operator writes one — the deploy is batched with the next live-surface row.

### M45-46 — preview before apply, built on the extraction that makes it honest (2026-11-10)

All three bullets now hold. The shape the plan insisted on is the shape it took: the preview is not a second ranking
path. `get_discovery` became a thin wrapper over

```rust
async fn feed_for(state, session: Option<&SessionUser>, params, knobs: &taste_knobs::Knobs)
```

and `POST /admin/discovery/preview` (operator only) calls the same function with `session = None` and a proposed
`Knobs`. Two consequences worth the refactor:

* **A preview cannot write.** With no session there is no account, so the taste-ordering-and-log arm of the feed cannot
  fire — the guarantee is structural. An earlier `log: bool` parameter was written and then deleted: the preview is
  anonymous, so the flag was dead code, and a dead flag is one somebody flips later.
* **The preview refuses exactly what the write door refuses**, because the mode vocabulary (`SIGNAL_WEIGHT_MODES`) and
  the basis-point conversion (`from_basis_points`) are shared rather than copied. A preview that accepted `"vibes"` would
  show a page that could never be served.

It answers in the feed's own shape plus `preview: {applied: false, base: config|stored, knobs}` — `base` because a preview
of a change to a stored row is not the same preview as a change to the config, and an operator who cannot tell them apart
reads the wrong number as the starting point.

**Refusals, each a sentence:** a non-operator is refused (`403`/`404`, one account wide); a strength outside
`0..=10000` basis points is `422` naming `gravity_strength` with the range in the message; an unknown weighting policy is
`422` naming `signal_weight_mode` and listing the accepted four.

**Proof:** `the_preview_shows_a_strangers_feed_under_proposed_knobs_and_changes_nothing` — green on SQLite and
PostgreSQL; `m29_transparency` 50/50 on SQLite; two mutants each reddened the intended assertion and were reverted from
`/tmp`: (a) preview serves the knobs already in force → `0.0` where `0.2` was promised, (b) preview writes the settings
row it previews → the public feed reads `0.2` afterwards and the panel's `source` stops saying `config`.

**Not done, named:** `concierge` (`home.curated`) and `gravity_dial` still read `state.config().taste` directly and should
adopt `taste_knobs::effective` in the same pass; the preview is anonymous only, because showing a named reader's feed to
an operator would put that reader's history on the operator's screen (§0.3).

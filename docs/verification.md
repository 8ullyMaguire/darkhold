## 2026-11-02 — the two lineages meet, and the merge finds four things

**Not a requirement — a merge:** `~/code/rust/darkhold` (the thinkcentre checkout, upstream
`lorehaven`) and `~/code-local/rust/darkhold` (this tree, upstream `darkhold`) had a common ancestor at
`4b731acc` and then diverged: this tree 44 commits ahead, that one three, and its three were the
Polestar/Polaris sign-in work (W2: one account, verified-email linking, the password retiring once an
estate sign-in exists). Tags `pre-merge-20261102` and `pre-merge-code-20261102` mark both tips.

Git merged it with **no conflicts at all**, which is the finding: four defects survived a clean merge and
each was caught by a gate that already existed.

1. **Migration numbers collided.** Both lineages had reached 0138/0139 — this tree with
   `0138_comments_timestamp_canonical` + `0139_search_history`, that one with the two OIDC migrations.
   Different filenames, so git kept both, and a migration is keyed on `version_name`: a fresh database
   would apply two 0138s in filename order and `apply_to`'s "up to and including this version" would be a
   prefix over an ambiguous list. Renumbered 0147/0148 in both engines, after this tree's 0146.
2. **Two undecided identity columns**, caught by the §11.16 gate wired into CI **earlier today**:
   `oidc_links.account_id` (`rekey_account`) and `oidc_links.email` (`replace` — it is a cache the
   provider refreshes on every sign-in, and the one column here that names a person in the clear).
3. **An unclassified account column**, caught by the erasure invariant: `oidc_links.account_id` had no
   treatment, so `DELETE FROM accounts` would leave a link claiming a subject still signs in as the
   person who erased themselves. Treatment: `Delete`.
4. **Two design tokens that did not exist**: `--border-1` in `Account.svelte` and `--color-surface-2` in
   `SignIn.svelte`. The token gate is the one this tree has and that one does not — an undefined custom
   property falls back to its literal, which is a light-theme colour, so the sign-in page's panel and the
   account border would have been invisible in every dark theme with nothing failing. Fixed in the
   components to `var(--border)` and `var(--color-surface-raised)`, not by growing the alias table.

Also repaired: the merged `migrations/postgres/0147_oidc_links.sql` header arrived with a paragraph
spliced from the SQLite file (a pre-existing defect in the other tree, now that the file is about to be
published).

```
python3 scripts/check-snapshot-pii.py       -> OK: 2426 columns across 331 tables   (was 2414/329)
cargo test -p darkhold-db --test erasure_covers_fk_free_accounts -> 4 passed
cargo test -p darkhold-db --test migration_catalogue             -> 6 passed
cargo test -p darkhold-app --lib                                 -> 240 passed
cargo test -p darkhold-domain                                    -> 16 suites ok
node node_modules/vitest/vitest.mjs run (frontend)                -> 610 passed, 81 files
python3 scripts/check-sqlite-migration-syntax.py                  -> OK
```

**What is NOT verified:** the OIDC code arrived with no tests of its own — 345 lines of route and 229 of
db module, unmigrated migrations until now, and `cargo check` is the only thing that has read it here. It
compiles, its migrations apply, and its columns are now decided; nobody has signed in through Polaris
from this tree. That is the next thing to do with it, and it needs a real estate account to do.

## 2026-11-02 — M45-62: the gate that ran for nobody, and the 37 columns it cannot see

**Requirement:** `M45-62` (closed) → `M45-65` (opened, measured) · **Tests:** gate + 4 erasure invariants

Running `python3 scripts/check-snapshot-pii.py` in a clean tree:

```
FAIL: 1 column(s) with no snapshot policy decision.
  undecided: badge_award.account_id
```

`badge_award` shipped in M47-14 with no policy decision, so §11.16's gate had been **red in a clean
tree** ever since — and nothing noticed, because **the gate was not in CI**. A gate that lives in a
docstring is a gate nobody trips over. Both halves are fixed here: the decision exists
(`rekey_account`, re-keyed on the `accounts.id` mapping so a snapshot keeps "someone earned this"
attached to a pseudonymised account), and the gate now runs in `.github/workflows/ci.yml` with its
self-test.

Then the question the row actually asked — `events.created_by` and `event_participation.account`
hold **pseud ids** under account-sounding names — turned out to be bigger than the two columns. The
answer is *keep the names and record the cost*, because a rename rewrites an applied migration
(sqlx checksums it) and every matcher that reads those columns, to make a name truer only for readers
of the DDL. §18.11 already admitted the pseud ids; it now also says what the spelling costs:

> neither identity gate sees these columns

Measured, from the migrations rather than from memory: **68** identity-carrying columns outside the
foreign-key graph, of which **37 are invisible to both gates** — 25 bare `account` columns and 12
`created_by` columns. The snapshot gate matches `account_id` and not a bare `account`; the erasure
scan matches `account_id`, `*_account`, `*_account_id` and not a bare `account` or `created_by`.
Neither has ever asked about `taste_profiles.account`, `trust_levels.account`, `challenges.created_by`
or 34 others.

That inventory is `M45-65`, opened rather than half-done: both gates compare measured against
classified in **both** directions, so widening the pattern turns them red until all 68 columns carry
a treatment, and each treatment is a judgement that has to be read off the writer — `RequirePseud` or
`RequireSession` in the handler that inserts the row. Widening the regex and leaving 68 entries blank
would be a red gate in a tree where red is already background noise, which is precisely the failure
this pass just fixed.

```
python3 scripts/check-snapshot-pii.py            -> OK: 2414 columns across 329 tables
python3 scripts/check-snapshot-pii.py --self-test -> exit 0
python3 -c "import yaml; yaml.safe_load(open('.github/workflows/ci.yml'))"  -> ok
cargo test -p darkhold-db --test erasure_covers_fk_free_accounts -> 4 passed
cargo test -p darkhold-domain --test ideas_match_requirements    -> 3 passed
```

**Red observed, not simulated:** the gate failed on `badge_award.account_id` before the decision was
written and passed after. Closing M45-62 also turned `ideas_match_requirements` red in the same way
it did last time (an implemented row still listed as a planned idea), and `scripts/ideas_catalogue.py`
retired it.

## 2026-11-02 — M45-36 (partial): what the reader is told when the media is gone

**Requirement:** `M45-36` · **Tests:** 3 component · **Status:** partial, and the row says so

A media reference with no healthy link rendered "This media is currently unavailable." That sentence is
indistinguishable from a bug: it names no item, keeps no author note, and leaves the reader nothing to
search for. The fallback now says what kind of media it was, quotes the author, and names the host it
used to live at.

Two deliberate limits, both asserted: the host is **named, not linked** (a dead URL is a promise the
page cannot keep), and an unparseable stored URL degrades to the note alone rather than an error in the
reader's face — `display_url` is author-supplied, so it is parsed defensively.

**What this does not do:** the fallback uses the author's note, not a curator-authored description of
the media, and there is no bounty that pays a curator to write one. Both need a column and a curator
door. That is why the row is `implemented-locally-tested` rather than closed.

```
node node_modules/vitest/vitest.mjs run src/routes/WorkPage.test.ts -> 12 passed
node node_modules/vitest/vitest.mjs run src/routes                  -> 324 passed (46 files)
node node_modules/vite/bin/vite.js build                            -> built in 3.16s
```

**Red by removal (all three measured):** dropping the host line fails one test, dropping the note line
fails two, and returning the raw URL when `new URL` throws fails the degradation test.

**Test-pollution note, since it cost more than the feature:** the media tests passed alone and failed in
the file. The cause was the note-links block leaving `isAuthorWork` mocked `true`; the page then
renders the author header and the media block never mounts, so the failure looked like a missing
element instead of a leaking mock. Each block now resets the mocks it depends on.

## 2026-11-02 — M65-08: the catalogue caught its own author, one row later

**Requirement:** `M65-08` · **Tests:** 3, one of which failed against the tree before this pass

I closed `M45-38` in the previous commit. `cargo test -p darkhold-domain` then failed:

```
M45-38 ("Author-note links through snapshot-and-monitor pipeline") is
implemented-fully-tested in docs/requirements.csv but still listed as an idea
```

That is §59.8's second backlog happening to me, one row after I produced it: two lists, one saying a
feature is planned while the tree serves it. The join existed and had not been consulted, because I
edited the CSV by hand and never re-ran the generator. `python3 scripts/ideas_catalogue.py` retired
the row and the suite went green.

The retirement is three joins — the idea's `row_id`, the requirement title, and the requirement id
behind the idea — because a brainstorm-sourced idea carries only one of them. The same regeneration
retired two rows an earlier pass had left behind (`M57-T1-14` "New in your fandoms", `M57-T1-27` "Most
bookmarked this week"), both `implemented-locally-tested` and both already modules
(`home.new_this_week`, `home.popular_in_fandom`). Two more second backlogs, found by running the tool.

```
cargo test -p darkhold-domain --test ideas_match_requirements  -> 3 passed
cargo test -p darkhold-domain                                  -> 16 suites ok, 0 failed (876 in the lib)
python3 scripts/ideas_catalogue.py                             -> 95 ideas; idempotent on a second run
```

**Probed for vacuity:** renaming one idea's id to a built requirement id (`M45-38`) turns
`no_idea_row_describes_a_requirement_that_is_already_built` red; restoring it turns it green. Removing
the retirement branch *in the generator* is not a valid probe — the generator retires through three
joins and the other two still catch the row, which I measured (96 rows, test still green) before
discarding that mutation as inconclusive rather than reporting it as a pass.

## 2026-11-02 — M45-38b: the note-link report, and a journey that proves the chain

**Requirement:** `M45-38b` (closing the surface `M45-38` left) · **Tests:** 3 component, 2 browser

The work page now lists the links a work's live notes carry, says which are dead **before** the click,
and offers the author an inline re-check. The section is absent — not empty — when a note holds no
links: a monitor that always shows a box is a box nobody reads.

Component tests decide the render from a given report; they cannot prove the report exists. So the
end-to-end journey types the note into the editor's revision field (no API shortcut — that would test
the endpoint twice), saves, publishes, opens the work page, presses the author's button, and asserts
the warning appears. Its links are loopback, which the fetch guard refuses unconditionally: the
verdict is deterministic with no network, and it is the honest answer to an author pointing a note at
their own machine.

```
node node_modules/vitest/vitest.mjs run src/routes/WorkPage.test.ts  -> 9 passed
node node_modules/vitest/vitest.mjs run src/routes                   -> 321 passed (46 files)
DARKHOLD_BIN=target/debug/darkhold npx playwright test e2e/note-links.spec.ts
  -> 2 passed (chromium, real binary, 7.6s)
node node_modules/vite/bin/vite.js build                            -> built in 8.10s
```

**Red by removal (both measured):** replacing the `dead > 0` branch with `false` fails *"warns before
the click"*; dropping the `count > 0` guard fails *"renders nothing when the notes carry no links"*.

Two things this pass corrected in my own work, recorded because both would have shipped green:

1. `getByText('One link has stopped answering.')` fails even when the sentence is on screen — the
   paragraph is one text node split by markup, so the assertion had to read the element's
   `textContent`. The first version of the test was asserting the *node shape*, not the behaviour.
2. The author branch of the page reads `lifecycle` off the work, so the author test's fixture needed
   a published work rather than the anonymous one; without it the render threw inside the component
   and the failure pointed at the assertion instead of the fixture.

The binary embeds `frontend/dist` at compile time, so `vite build` **alone** changes nothing that is
served — the e2e run needed `cargo build -p darkhold-app --bin darkhold` after it. That is the same
trap `continue-reading.spec.ts` documents, hit again.

## 2026-11-02 — M45-38: author-note links go through the monitor (§32.7.4a, gaps review D3)

**Requirement:** `M45-38` · **Tests:** 8 domain, 6 store × 2 engines, 5 HTTP × 2 engines

A note is prose, and the links in it rot like any availability link. The pipeline is the existing one —
the same `LinkStatus` vocabulary, the same three-strike rule — and the part specific to notes is
*extraction* and *ownership*.

**Extraction is a write, not a read.** A link discovered while rendering a page could never have been
checked, and re-scanning every revision per request makes the reader pay for the monitor. So the
extraction runs when the revision is saved, and an `ON CONFLICT DO NOTHING` on `(revision_id, url)`
keeps a retried save from resetting a dead link's health — the failure that would make the whole
monitor decorative.

**The three decisions in extraction, each with a test that fails when it is removed:** only `http`/
`https` (a `mailto:` cannot rot this way, a `javascript:` URL is an attack, a bare host is a guess);
trailing prose punctuation is stripped, because `(see https://x.test/meta).` must register the URL and
not `…/meta).`; and a cap of twenty per note, because a note is not a sitemap.

**`media_fetch::classify_with_length` is deliberately not reused.** It classifies *media*, so an HTML
response is a refusal to it — and a note link to a Tumblr post is HTML and perfectly healthy. The note
check runs through the same SSRF guard (`plan_fetch_allowing`, every resolved address checked,
redirects disabled) but applies its own verdict: 2xx/3xx answers, 404/410 gone now, everything else may
recover over three strikes.

**A refused URL is recorded dead immediately**, not escalated over three sweeps. Retrying a decision
does not change it, and the honest report of a note pointing at `127.0.0.1` is "this link is dead",
not "degraded, check again tomorrow". That refusal is also what makes the HTTP test deterministic: it
needs no network.

```
cargo test -p darkhold-domain --lib note_links             -> 8 passed
cargo test -p darkhold-db  --test note_links                -> 6 passed, SQLite and PostgreSQL
cargo test -p darkhold-app --test m45_38_note_links         -> 5 passed, both engines
cargo test -p darkhold-app --test route_inventory           -> 5 passed
```

**Red by removal (both measured):** disabling extraction fails `saving_a_note_registers_its_links…`
with `{"count":0}`; reporting every stored row instead of only live revisions fails
`a_corrected_note_replaces_what_the_work_reports` with `left: Some(2), right: Some(1)`.

Two things the second engine caught, recorded rather than tidied: PostgreSQL refuses `TEXT` into
`TIMESTAMPTZ` (columns are RFC 3339 text on both engines, as 0144 set) and refuses to decode `INT4` into
`i64` (`BIGINT`, ADR 0004). A third: the SQLite text names the work twice in two queries while
PostgreSQL names it once, so the bind count differs by dialect — the comment in the store says so,
because one bind fails with "column index out of bounds".

**Not done, and tracked:** `M45-38b` — the reader UI does not show the dead count yet. The pipeline and
the API are live; the page is next.

## 2026-11-02 — M65-08: an idea row becomes a module when it is built (§59.7/§59.8)

**Requirement:** `M65-08` · **Tests:** 3 in `crates/domain/tests/ideas_match_requirements.rs`

Measured before deciding anything: **14 rows in `IDEAS` had a `requirements.csv` row marked
`implemented-…`.** The generator retired an idea by row id and by title; a document-sourced idea
carries the requirement's id with its own wording, so neither rule reached it. The join was available
and unused — the catalogue was a second backlog of ten to fourteen rows.

**Why it had never been regenerated:** the generator's coverage gate refused to run. Two reserved spec
sections (§29, §44) were ideas with no home, and the gate exits rather than write a catalogue that is
missing them. Registered as `M29-08` and `M44-02`; the gate now also homes a reserved section by its
**number**, because `docs/spec/29-community-roadmap-feature-consensus-and.md` slugifies with a
file name truncated mid-sentence (`…-consensus-and`), and demanding a slug match would have demanded a
requirement sentence ending in the word "and".

Result: **175 → 165 idea rows, zero stale ones**, `docs/ideas-catalogue.json` regenerated from the same
run, and the assertion is a test rather than a habit — `no_idea_row_describes_a_requirement_that_is_already_built`
reads the CSV with `include_str!`, so it cannot be defeated by a working directory.

```
cargo test -p darkhold-domain --test ideas_match_requirements -> 3 passed
cargo test -p darkhold-domain --lib modules                   -> 16 passed
cargo test -p darkhold-app    --test route_inventory          -> 5 passed
python3 scripts/ideas_catalogue.py                            -> 165 ideas, retired 2 by title
```

**Red by removal:** re-inserting `M63-04` as an idea row fails the first test with *"M63-04
("crosspost union") is implemented-verified-e2e in docs/requirements.csv but still listed as an
idea"*.

One assumption in the third test was wrong and is now recorded there: the registry also carries rows
for surfaces that are **specified and unbuilt** (`story.similar` and its M64 siblings are `Planned` on
purpose), so "every registry row must be built" is not an invariant this tree holds.

## 2026-11-02 — M45-58: erasure reaches the supply routing record (§58.3 × §51)

**Requirement:** `M45-58` · **Tests:** 3 store (both engines) + the erasure-coverage gate

Two clauses that pull opposite ways: §58.3 says a routing row is **never deleted** (an abandoned
routing is the evidence the reader was asked), and §51 says erasure reaches every column that
identifies a person. The compatible answer is the third one — the row stays, the link becomes
**`erased:<row id>`**.

**No migration.** The first attempt rebuilt `supply_routing.account_id` to be nullable (0146, SQLite
table rebuild + `DROP NOT NULL` on PostgreSQL). It was withdrawn: SQLite's rebuild leaves the helper
table's name in the migration text, which `erasure_covers_fk_free_accounts` reads as a brand-new
unclassified account column, and NULL is the weaker answer anyway. `Treatment::Rekey` updates a `NOT
NULL` column in place, so the schema did not have to move.

**Why per row and not a shared placeholder.** `'erased'` would be unique-per-nothing: every severed
routing of one person would share it, and a pseudonym that links rows is still personal data. The
mutation test proves the difference — switching to a shared placeholder turns all three tests red,
the collision one first.

**The gate found a real leak while this was being fixed.** `badge_award.account_id` — the table
M47-14 added three commits ago — was unclassified, so erasing an account left its badges behind. It
is now `Delete` (a badge is personal recognition, not an audit record), and removing that entry again
turns the gate red with the column named.

```
cargo test -p darkhold-db  --test supply_routing_erasure         -> 3 passed, SQLite and PostgreSQL
cargo test -p darkhold-db  --test erasure_covers_fk_free_accounts -> 4 passed
cargo test -p darkhold-app --test erasure                         -> 10 passed
cargo test -p darkhold-db  --test migration_catalogue             -> 6 passed
```

**Red by removal:** a shared `'erased'` placeholder fails all three; removing the `badge_award` entry
fails the coverage gate naming the column.

## 2026-11-02 — M45-39b: the account archive (§13, M45-39 phase 2)

**Requirement:** `M45-39b` · **Tests:** 3 store + 3 route (both engines)

Measured before building, as M45-53 taught: a per-work exporter already exists (EPUB, PDF, AZW3, as a
queued job). This is the other artefact — the **recovery copy**. One document per account: profile,
pseuds, works, shelf, reading state, ratings, reviews, credit balances, each section with its own
SHA-256 and row count.

Four decisions:

- **Not a job.** A rendered novel is expensive; this is a handful of indexed reads. A reader on a
  dying instance should not be queued behind a worker that may never run.
- **No work bodies.** The words travel through the exporter, where the licence, the CTA and the
  sanitising live. A second path to the same bytes would be a second, unaudited path.
- **Erased rows excluded.** An archive of erased data would be an erasure that did not happen, and
  the test soft-deletes a pseud, a rating and a review to check exactly that.
- **The checksums are real.** The tests recompute each section's SHA-256 from the returned rows
  instead of comparing the field to itself.

**PostgreSQL found five separate faults**, all of them the ADR 0004 family and none of them visible on
SQLite: `id`/`account_id`/`subject_id`/`work_id` are UUID there and need `::text` on both the cast and
the predicate; `is_public` and `contains_spoilers` are BOOLEAN there and INTEGER here, so the `::int`
cast differs by engine rather than by width; and `stars` is BIGINT there while SQLite's INTEGER decodes
as `i64` anyway.

```
cargo test -p darkhold-db  --test archive          -> 3 passed, SQLite and PostgreSQL
cargo test -p darkhold-app --test m45_39_archive   -> 3 passed, both engines
cargo test -p darkhold-app --test route_inventory  -> 5 passed
```

**Red by removal:** a constant section hash fails the checksum test; dropping the account predicate
from `bookmarks` fails both the store and the route isolation tests; ignoring `deleted_at` on works
fails the erasure test.

## 2026-11-02 — M45-39a: the succession document and the redirect registry (§11.11a)

**Requirement:** `M45-39a` · **Tests:** 8 domain + 6 route (both engines)

`M45-39` sat at the front of the queue for a month because it was filed as "needs external
dependencies", which is true of one of its three parts: ActivityPub `Move`. The document and the
registry need a table and two routes. They ship as `M45-39a`; the row keeps the halves that really do
need a peer protocol.

The refusal this section exists for is **an export URL is mandatory**: a destination without one is a
forwarding address where the destination holds the only copy, and a reader who follows it is trusting
it with their archive.

Two properties worth naming:

- **Staleness is computed at read time** (`confirmed_at + review_days`), because a stored `is_stale`
  flag needs something to flip it and the case it exists for is the case where nothing is running. An
  unreadable `confirmed_at` reads as **stale**, not as fresh — a parse failure is not evidence of
  liveness.
- **A stale plan is served, marked** rather than withheld. `stale: true` plus `confirmed_at`
  distinguishes "had a plan, stopped confirming" from "never had one", and the first is the fact
  worth knowing.

Resolution is by **longest matching path prefix**, applied in Rust rather than SQL because the two
engines sort prefixes differently and a tie in the database would be decided by row order — which is
not a rule. A prefix matches at a segment boundary, so `/works` does not claim `/workshop`.

**PostgreSQL rejected the first schema**, and for the reason ADR 0004 already names: `review_days`
must be `BIGINT` because the value travels as `i64`, and PostgreSQL will not store an INT8 bind in an
INTEGER column. SQLite was green throughout; the route returned 500 on the second engine only.

```
cargo test -p darkhold-domain --lib succession          -> 11 passed (8 new)
cargo test -p darkhold-app --test m45_39_succession     -> 6 passed, SQLite and PostgreSQL
cargo test -p darkhold-app --test route_inventory       -> 5 passed
cargo test -p darkhold-db  --test migration_catalogue   -> 6 passed
cargo test -p darkhold-domain --lib modules             -> 16 passed
```

**Red by removal:** shortest-prefix resolution fails the registry test; widening the review window by
100,000 days fails the staleness test. **Green when removed, and recorded:** the empty-`export_url`
branch, because `!is_https("")` already refuses it — that branch buys a better message, not the
guarantee.

## 2026-11-02 — M47-14: First Words, the debut badge (audit item 75)

**Requirement:** `M47-14` · **Tests:** 3 domain + 3 route (both engines) + 6 frontend

No badges table existed. §9.7.6's fourteen milestone rows were specification only, so the audit's
"one boolean + a badge" was a feature and the boolean was the part to reject: a stored
`is_new_author` flag goes stale the instant a second work is published.

```
table:   badge_award (0144), UNIQUE (account_id, badge)
rule:    First Words — publish your first work — 20 credits (§9.7.6's own row)
trigger: content::publish_work → award_debut_badge, AFTER the commit
surface: PublicPseudView.badges / .debut → PseudProfile.svelte
```

**Why after the commit.** `publish_work` is one transaction covering the state change, the
publication event and two outbox rows, and the author is told it succeeded when that commits. The
bonus is a second ledger write with its own idempotency key; inside the transaction, a ledger failure
would roll back a publication already reported as done.

**One-time-ness has three layers**, and it matters which one carries it: `UNIQUE (account_id, badge)`,
the early return when the insert conflicted, and `post_transaction`'s key
`badge:<slug>:<account>`. Mutating the publish hook's `if count != 1` guard away leaves every test
green — so that guard is a fast path (it skips two reads per later publication), not the invariant.
Recorded in `docs/plans/m47-14-debut-badge.md` so the next reader does not delete it expecting a
failure and conclude the tests are asleep.

**PostgreSQL, again, found what SQLite could not:** `owner_pseud_id::text = ?` (UUID vs TEXT bind),
and `account_id::text` on the *select* as well (PostgreSQL refuses to decode a UUID column into a
`String`, which surfaced as a 500 on the profile route rather than as a type error).

```
cargo test -p darkhold-domain --lib badges              -> 3 passed
cargo test -p darkhold-app --test m47_14_debut_badge    -> 3 passed, SQLite and PostgreSQL
cargo test -p darkhold-app --test route_inventory       -> 5 passed
cargo test -p darkhold-db  --test migration_catalogue   -> 6 passed
cd frontend && npx vitest run src/routes/PseudProfile.test.ts -> 6 passed
```

**Red by removal:** skipping the bonus (`if false`) fails the credit assertion; widening
`published == 1` to `>= 1` fails the second-publication test.

## 2026-11-02 — M47-13: §9.7.9 reading goals (audit item 41)

**Requirement:** `M47-13` · **Tests:** 7 domain + 5 db + 4 route (both engines) + 5 frontend

The audit line was "§9.6 has the goal; nothing renders it". Two things were missing, not one: there
was no `reading_goal` table, and **no per-chapter read record either**.

`reading_progress` is unique on `(account_id, pseud_id, subject_type, subject_id)` — one row per
work per device — so it holds the chapter the reader is *on*. Three chapters of one work are one
row. A `chapters` target, which §9.7.9 offers by name, could not be counted from it. So `0143` ships
two tables:

```
reading_goal      (pseud_id PK, period, metric, target)          — opt-in by existing
reading_unit_read (pseud_id, unit_id UNIQUE, last_read_at)       — written by the progress route
```

and `reading::save_progress` writes the chapter record **inside the same transaction** as the
position, because a progress row that landed while its chapter record did not is a goal that
silently under-counts. `reading_a_chapter_through_the_reader_route_moves_the_goal` is the test that
proves the hook: it drives the real `PUT /reading/progress` door and asserts the ring's `current`.

Decisions worth the ink:

- **The window is computed, never stored.** A stored period row needs a sweep to roll it over and
  still resets at the wrong hour for most readers; the boundaries go back in the payload.
- **`works` is account-scoped and says so.** `reading_status` is keyed on the account (§9.6 keeps one
  shelf), so the payload carries `scope: "account"` for that metric and `"pseud"` for the others. A
  per-pseud goal silently reading another pseud's shelf would be a privacy bug with a tidy interface.
- **No verdict, no streak, no red.** No `failed`, `missed`, `streak` or `behind` key exists in the
  payload, and the route test asserts their absence on the serialised JSON. §9.7.1's "never create
  anxiety" is cheaper to keep when there is no word for the alternative.

**Schema names, measured rather than assumed.** The tables are `units` and `revisions` (keyed by
`unit_id`), not `chapters`/`chapter_revisions`: the 0004 migration alone gives the pre-rename names,
and the first version of this store compiled, then failed at runtime with `no such table: chapters`.
`reading_unit_read.unit_id` is TEXT while `units.id` is UUID, so the PostgreSQL arm casts the
*column* and never the join to `revisions`.

**PostgreSQL found three engine-specific faults** that SQLite could not: `target` had to be BIGINT
(ADR 0004 — INT4 will not decode into `i64`), `reading_status.account_id::text = ?` for the TEXT
bind, and the hook's statement needs `$n` placeholders — a `?` on PostgreSQL surfaces as `INTERNAL`
rather than as a bind error, which is why this one had to be found by running the second engine
rather than by reading the code.

```
cargo test -p darkhold-domain --lib reading_goal          -> 7 passed
cargo test -p darkhold-db  --test reading_goal            -> 5 passed, SQLite and PostgreSQL
cargo test -p darkhold-app --test m47_13_reading_goals    -> 4 passed, SQLite and PostgreSQL
cargo test -p darkhold-db  --test reader_surface_t1       -> 22 passed (the hook touched progress)
cargo test -p darkhold-app --test route_inventory         -> 5 passed
cargo test -p darkhold-db  --test migration_catalogue     -> 6 passed
cd frontend && npx vitest run                             -> 602 passed (5 new)
npx svelte-check --threshold error                        -> 0 errors
```

**Red by removal:** accepting `target <= 0` fails the domain test *and* the route's 422 test;
un-flooring `remaining()` fails the "no -3 to go" test; disabling the chapter-record hook fails the
end-to-end goal test with `current: 0`.

## 2026-11-02 — M47-12: §9.4 quick reactions (audit item 43)

**Requirement:** `M47-12` · **Tests:** 6 db + 5 route (both engines) + 6 frontend

The audit's line for item 43 was "reaction rows exist; the label is free text", which reads as a
validation rule on a column that is already there. The probe found the wrong table: `work_reactions`
(0038) is §35.1's **typed vote** — one per reader, retractable, with a costed negative option — keyed
`(work, pseud)`. §9.4's labelled bar had never been built, and it is a different surface: six labels,
no typing, and **several at once per reader**. A reader may vote "insightful" and react "made me
cry" to the same work; a table keyed `(work, pseud)` cannot hold both.

```
table:   quick_reaction (0142), UNIQUE (work_id, pseud_id, label)   ← the "count once" rule
routes:  GET    /api/v1/works/{id}/quick-reactions            public, ships the vocabulary
         PUT    /api/v1/works/{id}/quick-reactions/{label}    pseudonymous, idempotent
         DELETE /api/v1/works/{id}/quick-reactions/{label}    pseudonymous, idempotent
gate:    work_feedback_preferences.comments_enabled → owner's account default → enabled
ui:      QuickReactions.svelte, mounted under ReactionBar on the work page
```

**Why the unique index is the feature.** §9.4 says "count once per reader per target"; making that
the key means every read is a plain `GROUP BY` and no query can forget a `COUNT(DISTINCT …)`.

**Why the refusal names the vocabulary.** An unknown slug is a `422` carrying all six, because
"unknown label" leaves the reader guessing at slugs they cannot see.

**Why `GET` answers `enabled: false` instead of `403`.** The work page stays readable when an author
turns feedback off; only the *write* is refused. A `403` on the read would tell the reader they had
done something wrong.

**PostgreSQL needed three things SQLite did not**, all found by the second engine rather than by
reading: `works.id = ?::uuid` (TEXT bind against a UUID column is "operator does not exist: uuid =
text"), `COALESCE(...)::bigint` (INT4 into `i64`), and dialect-aware boolean literals in the fixtures
(PG rejects `0` for a `BOOLEAN` column).

```
cargo test -p darkhold-db  --test quick_reactions        -> 6 passed, SQLite and PostgreSQL
cargo test -p darkhold-app --test m47_12_quick_reactions -> 5 passed, SQLite and PostgreSQL
cargo test -p darkhold-app --test route_inventory        -> 5 passed (the three new routes are tabled)
cargo test -p darkhold-app --test milestone_31_work_discussion -> 34 passed
cd frontend && npx vitest run                            -> 597 passed (6 new)
npx svelte-check --threshold error                       -> 0 errors
```

**Red by removal:** making `is_known` return `true` fails the domain vocabulary test *and* the
route's 422 test; dropping `{#if view?.enabled}` fails the "renders nothing when feedback is off"
test.


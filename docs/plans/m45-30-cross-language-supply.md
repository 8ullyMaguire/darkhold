# M45-30 — Cross-language supply (§61): fund a translation, and report the gap honestly

**Status:** planned 2026-11-02 against measurement. **No migration.**

## The idea, as the gaps review wrote it (C6)

> The fics the operator would love most may be in languages they don't read; pixiv, LOFTER, Ficbook,
> and the Spanish- and Portuguese-language communities are enormous. Taste-score foreign works (tags
> map across languages; stylometry partially transfers), then fund translation bounties with author
> permission. Priority 7 becomes a supply engine for Priority 2.

## Premises, measured 2026-11-02

| claim | measurement | verdict |
|---|---|---|
| works carry a language | `works.language TEXT NOT NULL DEFAULT 'en'` | holds |
| there is foreign supply to score or fund | live: **58,042 of 58,042 works are `en`** | **zero**: the mechanism has no input on this instance |
| author permission is required, and exists | `works.permission_statement` holds a `PermissionStatement` JSON with a `translation` field (`yes/ask/no/unstated`); `GET`/`PUT /api/v1/works/{id}/permissions` are **already mounted**, owner/contributor-gated | holds, and needs no new door |
| lineage edges exist for `translation` | `derivative_lineage.kind IN (… 'translation' …)`; `/api/v1/works/{id}/lineage` has GET **and** POST | holds, already built |
| bounties exist to be funded | `bounties` + `economy::create_bounty(db, job_kind, terms, account, amount)`; live **0** rows | holds |
| tags map across languages | `work_tags` exists but is **not backfilled** (M63-32: 0 joins of 1,247 liked works), and live `tags` is absent | **blocked** |
| stylometry partially transfers | `grep -rln stylometr crates/` → only a catalogue string; **no implementation** | **absent** |

So the milestone splits cleanly into the part that is buildable today and the part that would be
invented data:

- **Buildable now**: the supply report (a real count of a real column), the translation bounty (an
  existing table, an existing gate, an existing balance), and the refusal for the transfer half.
- **Refused by name**: "taste-score foreign works" — there is no stylometry in this tree and the tag
  vocabulary is not backfilled, so a cross-language taste score would be a number with no signal
  behind it. The door says so, with the two missing pieces named, instead of returning 0.5.

## Step 1 — domain (`crates/domain/src/translation.rs`, pure)

```rust
pub enum Language { Primary(String) }              // "pt-BR" -> "pt"
pub fn normalise_language(tag: &str) -> Option<String>;
pub enum Funding { Fundable, MustAsk, Forbidden { statement: &'static str } }
pub fn funding_gate(p: Permission) -> Funding;      // yes -> Fundable, ask -> MustAsk, no/unstated -> Forbidden
pub struct Supply { pub by_language: BTreeMap<String, i64>, pub total: i64 }
pub fn unrepresented(supply: &Supply, wanted: &[&str]) -> Vec<String>;
pub enum Transfer { Available, Unavailable { missing: Vec<&'static str> } }
```

- `yes` funds, `ask` does **not** fund (the author must answer first — §40.2's "ask routes to a
  request"), `no` and `unstated` refuse and name the statement. `unstated` refuses because §40's
  model treats it as the restrictive reading, and C6 says *author permission required*.
- `normalise_language` takes the primary subtag and lowercases it, so `pt-BR`, `pt-br` and `pt` are
  one language for counting purposes. Empty/blank is `None`.
- `Transfer::Unavailable { missing }` names `["stylometry", "tag backfill (M63-32)"]` — the two
  measured gaps, in the payload.

## Step 2 — store (`crates/db/src/translation.rs`)

| function | rule |
|---|---|
| `language_supply(db)` | `GROUP BY language` over `works` — the whole catalogue, not a sample |
| `fund_translation(db, work, target, account, amount)` | reads the work's `translation` statement; `Funding::Forbidden` refuses **naming the statement**; `MustAsk` refuses naming the request path; then the patron's balance must cover `amount`; then `create_bounty(… job_kind='translation', terms={work_id, source_language, target_language})` |
| `translation_bounties(db)` | open bounties with `job_kind='translation'`, newest first |
| `bounties_for_work(db, work)` | the same filtered by `terms->>work_id` |

Balance check: `economy::balances(account)` summed; a bounty from an account that cannot pay it is a
promise the archive would have to break. The bucket sums are basis points, and the comparison is
`sum >= amount`. Overdraft is not a thing here.

## Step 3 — routes (`crates/app/src/routes/translations.rs`), module `translations`

| method + path | who | answers |
|---|---|---|
| `GET /api/v1/translations/supply` | anyone | `{by_language, total, unrepresented: [...], transfer: {available: false, missing: [...]}}` |
| `GET /api/v1/translations/bounties` | anyone | open translation bounties |
| `POST /api/v1/works/{id}/translation-bounty` | session | `201` with the bounty, or `422` naming `translation` / `target_language` / `balance` |

Module row: id `translations`, relevance **4**, `default_on: true`, `Toggle::Operator`, spec §61, no
nav. Relevance 4 and not 10 because the reference instance has no foreign work at all: the doors are
correct and inert, and claiming translation is part of every instance's shape would be a lie the
supply report itself disproves.

## Step 4 — tests

`crates/db/tests/translations.rs` (both engines), 7 cases: supply counts by primary subtag and total;
an empty catalogue is a zeroed report, not an error; a `yes` statement funds and writes a bounty with
`job_kind='translation'`; `ask` refuses naming the statement; `no` refuses; `unstated` refuses
(because C6 requires permission); an account without the balance is refused and **writes nothing**.

`crates/app/tests/m45_30_translations.rs`, 6 cases: the supply door is public and reports the live
shape (`en` only, `transfer.available == false` with the two missing pieces); funding needs a session;
a work with `translation: yes` funds; `ask`/`no` refuse on the field `translation`; a body with no
language or an empty one refuses on `target_language`; the module is registered enabled with no nav.

Domain unit tests: subtag normalisation (`pt-BR`, `PT`, ` `), the four permission arms, supply
unrepresented against a wanted list, and that `Transfer::Unavailable` carries the two named gaps.

## Verification

| # | command | expected |
|---|---|---|
| 1 | `cargo test -p darkhold-domain --lib translation` | 5 passed |
| 2 | `cargo test -p darkhold-db --test translations` | 7 passed |
| 3 | same with `DARKHOLD_TEST_PG_URL` | 7 passed |
| 4 | `cargo test -p darkhold-app --test m45_30_translations` | 6 passed |
| 5 | mutations | every one caught |
| 6 | `python3 scripts/ideas_catalogue.py` | 175 ideas, 0 homeless |

Mutations: fund on `ask`; fund on `unstated`; skip the balance check; write the bounty before the gate;
count languages by full tag instead of primary subtag; report `transfer.available: true`.

## Deferred, by name

- **Taste-scoring foreign works.** No stylometry exists in this tree, and `work_tags` is not
  backfilled (M63-32), so a cross-language score would be invented. `GET /translations/supply`
  answers `transfer: {available: false, missing: ["stylometry", "tag backfill (M63-32)"]}`.
- **Claiming and paying a bounty.** `create_bounty` writes state `open` with `escrow_transaction
  'pending'`; the settlement path is §20.3's, and this row funds rather than pays.
- **Detecting that a work is foreign.** `language` is a declared column and the importer sets it; a
  language *detector* is a different feature with its own false-positive story.

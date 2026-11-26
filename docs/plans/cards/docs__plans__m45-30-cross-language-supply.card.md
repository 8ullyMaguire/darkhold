# docs/plans/m45-30-cross-language-supply.md — plan card

Source: `docs/plans/m45-30-cross-language-supply.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

| stylometry partially transfers | `grep -rln stylometr crates/` → only a catalogue string; **no implementation** | **absent** |
## Step 1 — domain (`crates/domain/src/translation.rs`, pure)
## Step 2 — store (`crates/db/src/translation.rs`)
## Step 3 — routes (`crates/app/src/routes/translations.rs`), module `translations`
`crates/db/tests/translations.rs` (both engines), 7 cases: supply counts by primary subtag and total;
`crates/app/tests/m45_30_translations.rs`, 6 cases: the supply door is public and reports the live
| 6 | `python3 scripts/ideas_catalogue.py` | 175 ideas, 0 homeless |

## Copy-pasteable code / commands

pub enum Language { Primary(String) }              // "pt-BR" -> "pt"
pub fn normalise_language(tag: &str) -> Option<String>;
pub enum Funding { Fundable, MustAsk, Forbidden { statement: &'static str } }
pub fn funding_gate(p: Permission) -> Funding;      // yes -> Fundable, ask -> MustAsk, no/unstated -> Forbidden
pub struct Supply { pub by_language: BTreeMap<String, i64>, pub total: i64 }
pub fn unrepresented(supply: &Supply, wanted: &[&str]) -> Vec<String>;
pub enum Transfer { Available, Unavailable { missing: Vec<&'static str> } }

## Verification command and expected output

  existing table, an existing gate, an existing balance), and the refusal for the transfer half.
| # | command | expected |
| 1 | `cargo test -p darkhold-domain --lib translation` | 5 passed |
| 2 | `cargo test -p darkhold-db --test translations` | 7 passed |
| 4 | `cargo test -p darkhold-app --test m45_30_translations` | 6 passed |
Mutations: fund on `ask`; fund on `unstated`; skip the balance check; write the bounty before the gate;

## RED/GREEN evidence

| 1 | `cargo test -p darkhold-domain --lib translation` | 5 passed |
| 2 | `cargo test -p darkhold-db --test translations` | 7 passed |
| 3 | same with `DARKHOLD_TEST_PG_URL` | 7 passed |
| 4 | `cargo test -p darkhold-app --test m45_30_translations` | 6 passed |

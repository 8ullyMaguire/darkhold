# docs/plans/m45-23-curator-adapters.md — plan card

Source: `docs/plans/m45-23-curator-adapters.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

Spec: `docs/spec.md` §55. Plan. Tracker row: `docs/requirements.csv` `M45-23`.
`wasmi` to a `Cargo.toml` before step 9 is green, stop — that is the failure this
| SSRF refusal | `crates/scrapers/src/safety.rs` `validate_url` (L1618), `is_forbidden_ip` (L1755) | Already rejects loopback, private, link-local incl. `169.254.169.254`, and `::ffff:` mapped forms. **Tested** at L2750. |
| `SafeFetcher` | `safety.rs` `SafeFetcher::new(allowed_hosts, policy)` | The domain lockdown already exists as a constructor parameter. |
| robots + pacing | `safety.rs` `robots_gate`, `robots.rs` | `Crawl-delay`, 1/s floor, `Disallow`, operator override. |
| `FixtureFetcher` | `safety.rs` L1532 | Offline parsing harness. |
| `SourceAdapter` trait | `crates/scrapers/src/registry.rs` | `route(url)`, `by_key`, `catalogue`, `disable`. |
| 11 adapters | `crates/scrapers/src/sites/` | ao3, chyoa, efiction, ffnet, ficbook, pawchive, royalroad, scribblehub, syosetu, wattpad, xenforo |
| 65 HTML fixtures | `crates/scrapers/tests/fixtures/` | §11.7's discipline is already paid for. |
| Extension domain | `crates/domain/src/extension.rs` | `ExtensionState`, `Capability`, `MemoryTier`, `grant_is_subset`. |
**File:** `crates/domain/src/extension.rs`
**File:** `crates/domain/src/source_adapter.rs` (new)

## Copy-pasteable code / commands

/// §21.2's category list, now a type. `SourceAdapters` is new in §55.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    ReaderWidget, DashboardWidget, ThemeLayout, RecommendationEngine,
    DeclarativeRecipe, SearchHelper, WritingTool, ChallengeVariant,
    PositivityFilterRule, MoodTag, FandomLanding, Integration,
    /// §55. Curator-submitted source adapters.
    SourceAdapters,
}

cargo test -p darkhold-domain --lib extension 2>&1 | grep 'test result'

/// A §55.3 declarative source adapter: URLs and selectors, no code.
///
/// Every selector is compiled at construction. A manifest that would throw on
/// first fetch is refused at submission, where a steward can read the reason,
/// rather than discovered by a reader's import.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceManifest {
    pub source_id: String,
    pub name: String,
    /// Scheme + host, no trailing path. The allowlist is exactly this host and
    /// its subdomains — §55.4.1.
    pub base_url: String,
    /// Minimum seconds between requests. The host may lower this and never
    /// raises it; §11.5's `Crawl-delay` and the 1/s floor win on conflict.
    pub rate_limit_per_second: f64,
    pub work_pattern: String,
    pub chapter_pattern: String,
    pub selectors: Selectors,
    pub pagination: Pagination,
    pub auth: Auth,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selectors {
    pub title: String,
    pub author: String,
    pub summary: String,
    pub body: String,
    pub tags: String,
    pub word_count: String,
    pub date_published: String,
}

## Verification command and expected output

gates it on a sandbox that does not exist. Steps 8–10 make that gate checkable
**Verify**
cargo test -p darkhold-domain --lib extension 2>&1 | grep 'test result'
**Verify**
cargo test -p darkhold-domain --lib source_adapter 2>&1 | grep 'test result'
**Note on the private-address test:** assert on the *rule*
test passes while `172.16.0.1` slips through. `safety.rs` L1760 already has the
**Verify**
cargo test -p darkhold-domain --lib source_adapter 2>&1 | grep 'test result'
  both literals in the test, so the two encodings cannot drift.
  `const` and parse it. **This test is what keeps the spec's example real**: if
**Verify**
cargo test -p darkhold-scrapers --lib declarative 2>&1 | grep 'test result'
The load-bearing test is parity, because §55.8's fourth acceptance line demands it:
#[tokio::test]
§11.1's "handed in, never held" rule, and they are why this test needs no fetcher.
**Also verify:** the adapter cannot fetch a URL outside its `base_url`. Break the
allowlist and confirm the test goes red; this is §55.4.1 and it is the one
**Verify**
cargo test -p darkhold-db --lib migration 2>&1 | grep -E '0113|test result'

## RED/GREEN evidence

`wasmi` to a `Cargo.toml` before step 9 is green, stop — that is the failure this
  Break it by removing the attribute and confirming red.
allowlist and confirm the test goes red; this is §55.4.1 and it is the one
- `a_curator_below_tl3_is_refused` — delete the trust check, confirm red.
# then: add `wasmi = "0.4"` to crates/app/Cargo.toml, re-run, confirm RED, revert.
An `assert!(!contains("wasmi"))` that has never been seen red is the same class
of non-evidence as a green test on unfixed code.
- [ ] §55.8's acceptance criteria have named tests, green on **both** engines.
- [ ] Every new test seen red.
- [ ] `wasm_gate` green **and** seen red.

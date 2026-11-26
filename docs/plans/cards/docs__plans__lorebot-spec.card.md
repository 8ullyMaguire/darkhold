# docs/plans/lorebot-spec.md — plan card

Source: `docs/plans/lorebot-spec.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

* `scripts/conformance.sh` — the suite run three times, once per adapter, and it

## Copy-pasteable code / commands

bot issues a short-lived challenge
→ user opens Darkhold, signs in there and only there
→ selects a pseud and scopes
→ confirms
→ bot receives revocable limited authorization

## Verification command and expected output

* No database driver in the dependency tree. This is **asserted by a test**, not
§23.2's list is not a wish list. Each item below is a test, not a review
| requirement | where it is enforced | the test |
| destination/channel context checked before every response | `Adapter::send` takes an explicit destination | conformance suite: a reply is never delivered to a destination the caller did not pass |
| revocation and unlinking | `revoke()` in the store; the token stops working on the next call | integration test against a real Darkhold |
| bots never receive the password | the link flow mints a token server-side | integration test: the exchange response contains a token and no password field |
## Exit condition
From the plan, verbatim: **a Discord, a Telegram and a Matrix adapter all pass
the same core test suite; revoking a token takes effect on the next call; no
conformance suite** — one test body, run three times, once per adapter — not
three copies that drift. Three hand-written suites pass on the day they are
`postgres` or `surrealdb` appears. It is a test because the property is
* `cargo test --workspace` — unit tests, and the conformance suite per adapter.
* The integration test needs a running Darkhold; it is `#[ignore]`d without

## RED/GREEN evidence

No RED/GREEN evidence was recorded in the source document.

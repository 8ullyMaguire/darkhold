# docs/plans/lorebot-plan.md — plan card

Source: `docs/plans/lorebot-plan.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

Implements `docs/plans/lorebot-spec.md`. Read the spec first; this file is the
mkdir -p ~/code-local/rust/lorebot/{crates/{lorebot-core,lorebot-adapter,lorebot-discord,lorebot-telegram,lorebot-matrix},scripts,tests}
`Cargo.toml` at the root:
members = ["crates/*"]
Deliberately `members = ["crates/*"]` so adding an adapter is a directory, not a
`rust-toolchain.toml`, pinned the same way Darkhold's is, so a toolchain bump
Ported from `~/code/rust/fanfic-archivist-bot/crates/archivist-core/src/core.rs`
cd ~/code-local/rust/lorebot && ./scripts/conformance.sh
cd ~/code-local/rust/lorebot && sed -i 's|^\[dependencies\]|[dependencies]\nrusqlite = "0.32"|' crates/lorebot-core/Cargo.toml && cargo test -p lorebot-core no_code_path 2>&1 | tail -3
* `lorebot/docs/ADAPTERS.md` — how to add a fourth adapter (the port source's
* Darkhold `docs/requirements.csv` — M54-01 to `implemented-verified-e2e` with
* Darkhold `docs/handoff.md` — the Part B entry, including the two omissions

## Copy-pasteable code / commands

mkdir -p ~/code-local/rust/lorebot/{crates/{lorebot-core,lorebot-adapter,lorebot-discord,lorebot-telegram,lorebot-matrix},scripts,tests}
cd ~/code-local/rust/lorebot
git init

[workspace]
resolver = "2"
members = ["crates/*"]

cd ~/code-local/rust/lorebot && cargo metadata --no-deps --format-version 1 >/dev/null && echo OK

## Verification command and expected output

**Verify**
**Verify**
cd ~/code-local/rust/lorebot && cargo test -p lorebot-core core:: -- --nocapture
**Verify**
cd ~/code-local/rust/lorebot && cargo test -p lorebot-core error::
that the two are distinguishable in their `Display` output. The test asserts on
**Verify**
cd ~/code-local/rust/lorebot && cargo test -p lorebot-core config::
**Verify**
cd ~/code-local/rust/lorebot && cargo test -p lorebot-api -- --nocapture
  with the same key is a catastrophic failure mode, so the test asserts
  conformance suite pairs this with an integration test that the server refuses
**Verify**
cd ~/code-local/rust/lorebot && cargo test -p lorebot-core store:: -- --nocapture
The wrong-key test is the one that catches a real bug: GCM authentication
**Verify**
cd ~/code-local/rust/lorebot && cargo test -p lorebot-core cache::
**Verify**
cd ~/code-local/rust/lorebot && cargo test -p lorebot-core dispatch:: -- --nocapture
**The conformance suite is the exit condition**, and it is written once:

## RED/GREEN evidence

5. The destination passed in is the destination used — no ambient channel.
* `cargo test --workspace` — green, both `lorebot-api` and `lorebot-core`.

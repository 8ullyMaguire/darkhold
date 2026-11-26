# Darkhold — architecture

Measured 2026-11-26 from this repository and from `estate.toml` (`[[site]] name = "darkhold"`). Where the two disagree, the declaration is quoted with its source rather than silently reconciled. Repo-specific docs stay authoritative; this is the uniform map.

## Runtime processes and ports

| fact | declared value | source |
|---|---|---|
| hostname | `darkhold.polarisocial.xyz` | `estate.toml` |
| app port | `8081` | `estate.toml` |
| nginx port | `8008` | `estate.toml` |
| unit | `darkhold.service` (scope `user`) | `estate.toml` |
| health | `/healthz` | `estate.toml` |
| vhost | `own`; probe `true`, assert_version `true` | `estate.toml` |

Site status in the manifest: `live`. Additional declarations: `identity = "polaris-id-linked"`, `inbox = "local"`, `search = "polaris-search"`, `recs = "off"`, `loops = "off"`, `media = "local"`, `comments = "off"`, `taste = "polaris-taste-source"`, `gravity_dial = "polaris-gravity"`, `preservation = "off"`, `federation = ["activitypub"]`, `feeds = false`, `export = "local"`.

## Data stores

SQLite (default) or PostgreSQL, with migrations under `migrations/sqlite/` and `migrations/postgres/` (`README.md`; `docs/requirements.csv` rows on storage).

Counted from the tree: 0 SQL migration files; sources are listed in `docs/FEATURES.csv`.

## Module boundaries

Workspace/declared modules from `Cargo.toml`: `crates/metadata`, `crates/decisions`, `crates/domain`, `crates/db`, `crates/app`, `crates/cli`, `crates/scrapers`, `crates/taste`, `crates/test-support`, `crates/sandbox`.

Top-level directories in this checkout: `.cargo`, `.darkhold`, `.github`, `.hermes`, `config`, `crates`, `data`, `deploy`, `docs`, `frontend`, `migrations`, `scripts`.

Routes declared in the estate manifest: `capabilities = "/api/v1/capabilities"`, `auth = "/api/v1/auth/login"`, `search = "/api/v1/search"`, `taste = "/api/v1/taste"`, `inbox = "/api/v1/notifications"`, `openapi = "/api/v1/openapi.json"`.

## Deploy chain

- `build`: `"CARGO_TARGET_DIR=~/.cargo-target/darkhold cargo build --release -p darkhold-app --bin darkhold"` — `estate.toml:[site.deploy]`
- `migrate`: `"DARKHOLD_CONFIG=/personal/documents/code/rust/darkhold/darkhold.toml ~/.cargo-target/darkhold/release/darkhold migrate"` — `estate.toml:[site.deploy]`
- `restart`: `"systemctl --user restart darkhold.service"` — `estate.toml:[site.deploy]`
- `verify`: `"darkhold.polarisocial.xyz"` — `estate.toml:[site.deploy]`

## Verification and docs map

- `README.md: cd frontend && npm ci && npm run build && cd ..`
- `README.md: cargo clippy --workspace --all-targets > /tmp/clippy.log 2>&1   # stderr matters`
- `README.md: cargo test  --workspace --no-fail-fast  > /tmp/test.log   2>&1`
- `README.md: Redirect with 2>&1 and read the file. rustc, clippy and cargo test`
- `AGENTS.md: Verification: run just check and paste the real output; then the estate gates`

- Feature ledger: `docs/FEATURE-LEDGER.md` (1055 rows, statuses measured 2026-11-26).
- Plan cards: `docs/plans/README.md`.
- Refusals: `docs/DECISIONS.md`.
- Current state: `docs/STATUS.md`.
- Document inventory: 387 docs files at depth ≤ 3 from this pass's count; 127 plan documents and 27 ADR records.

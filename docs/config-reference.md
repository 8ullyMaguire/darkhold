# Darkhold Configuration Reference

Every instance is configured through `darkhold.toml` (spec §38). This file
documents each section and its keys, with defaults, type, and purpose.

Sections are applied in the order shown here; `deny_unknown_fields` means a
typo in any key is a startup error rather than a silent misconfiguration.

## `[site]`

The identity of the instance.

| Key | Type | Default | Purpose |
|-----|------|---------|---------|
| `name` | `string` | `"Darkhold"` | Display name across the UI |
| `base_url` | `string` | auto | Canonical URL (auto = bind + port) |
| `contact_email` | `string?` | `None` | Admin contact shown on error pages |
| `topics` | `[string]` | `[]` | Public discovery topics |

## `[server]`

How the process listens.

| Key | Type | Default | Purpose |
|-----|------|---------|---------|
| `bind` | `string` | `"127.0.0.1"` | Listen address |
| `port` | `int` | `8080` | Listen port |
| `max_body_bytes` | `int` | `2097152` | Max request body (2 MiB) |
| `request_timeout_secs` | `int` | `30` | Per-request timeout |

## `[database]`

Connection string and pool settings.

| Key | Type | Default | Purpose |
|-----|------|---------|---------|
| `url` | `string` | `sqlite://./data/darkhold.sqlite` | DB connection string |
| `max_connections` | `int` | `5` / `10` | Pool size (dev / prod) |
| `acquire_timeout_secs` | `int` | `10` | Pool acquire timeout |

## `[storage]`

Filesystem layout for blobs.

| Key | Type | Default | Purpose |
|-----|------|---------|---------|
| `root` | `string` | `"./data"` / `"/var/lib/darkhold"` | Blob root (dev / prod) |

## `[security]`

Cookie, session, and CSRF settings.

| Key | Type | Default | Purpose |
|-----|------|---------|---------|
| `cookie_secure` | `bool` | `false` / `true` | Secure cookie flag (dev / prod) |
| `session_ttl_days` | `int` | `30` | Session lifetime |
| `csrf_required` | `bool` | `true` | CSRF protection |
| `trust_proxy` | `bool` | `false` | Trust `X-Forwarded-*` headers |

## `[logging]`

What is logged and how.

| Key | Type | Default | Purpose |
|-----|------|---------|---------|
| `filter` | `string` | `"info,darkhold_app=debug"` | Log filter |
| `format` | `string` | `"pretty"` / `"json"` | Log format (dev / prod) |

## `[assets]`

Where static assets live.

| Key | Type | Default | Purpose |
|-----|------|---------|---------|
| `root` | `string?` | unset | Custom asset directory |

## `[administration]`

Operator and webhook settings.

| Key | Type | Default | Purpose |
|-----|------|---------|---------|
| `operator_account_id` | `int?` | `None` | Account ID allowed on `/admin` routes |
| `webhook_timeout_secs` | `int` | `10` | Webhook request timeout |
| `webhook_max_attempts` | `int` | `5` | Webhook delivery attempts |
| `webhook_base_delay_ms` | `int` | `500` | Webhook backoff base |
| `webhook_allowed_hosts` | `[string]` | `[]` | Webhook host allowlist |

## `[dev]`

Development-only toggles.

| Key | Type | Default | Purpose |
|-----|------|---------|---------|
| `seed_enabled` | `bool` | `true` | Allow `darkhold seed` to write fixtures (prod = false) |

## `[imports]`

Source fetching and CAPTCHA settings.

| Key | Type | Default | Purpose |
|-----|------|---------|---------|
| `robots_posture` | `string?` | `None` | Robots policy: `strict`, `permissive`, or `off`. The real setting |
| `honour_robots` | `bool` | `true` | Compatibility key. `false` selects `permissive`; `true` selects `strict` unless `robots_posture` says otherwise |
| `solver_url` | `string?` | `None` | CAPTCHA solver URL |
| `archive_fallback` | `bool` | `false` | Fallback to archive.org |
| `robots_posture_overrides` | `table` | `{}` | Per-adapter overrides, keyed by the adapter's `key()` — e.g. `[imports.robots_posture_overrides.ao3]` |

`robots_posture` and `honour_robots` are read together in
`darkhold_scrapers::robots::resolve_posture`, which is the only thing that
should read either. `honour_robots` exists so an older config keeps working, and
it is a fallback rather than a default: with `honour_robots = false` and no
`robots_posture`, the posture is `permissive` — deliberately not overridden by a
default, or the compatibility key would be a key that does nothing when it
mattered.

## `[exports]`
## `[retention]`

Who may read a body by default (spec §7.7).

| Key | Type | Default | Purpose |
|-----|------|---------|---------|
| `default_body_audience` | `string` | `"full"` | The body-audience baseline. See the `BodyAudience` variants in `darkhold_domain::retention` |
| `body_request_min_trust` | `2` | `0` | The trust level a reader needs to ask this instance to fetch and keep an external body for them personally (§11.15b). Gates the **request** and nothing else: once a copy exists, every reader eligible for the work reads the same thing, and no read path consults this value. |

## `[roadmap]`

Who may participate in the roadmap (spec §29.4). `deny_unknown_fields`, like
every section here.

| Key | Type | Default | Purpose |
|-----|------|---------|---------|
| `min_trust` | `int` | `1` | The trust level a reader needs to ballot, vote, or suggest. Refusals name the bar and the reader's own level |

## `[retention_governance]`

Who may propose a change to body retention, and whether a passed proposal is
itself the decision (spec §5, §19.15, amendment §5). `deny_unknown_fields`.

| Key | Type | Default | Purpose |
|-----|------|---------|---------|
| `proposal_min_trust` | `int` | `1` | The trust level needed to open a proposal or cast a ballot |
| `widen_quorum` | `int` | `3` | The bar for a change that *widens* storage. Clamped **up** to 3: a value below three is refused, not honoured, because a bar under three is one person's second tap. A value *above* your reader count is honoured, and means "this instance does not change storage policy by vote" |
| `proposal_cooling_days` | `int` | `7` | How many days a ballot stays open. Stored per proposal as a `closes_at`, so changing this does not move the deadline of a ballot somebody is already voting in |
| `binding_mode` | `bool` | `false` | Whether a passed proposal changes the setting on its own. `false` (the default) is advisory: the readers' decision is a record, and an operator applies it with `POST /api/v1/admin/retention/proposals/{id}/respond` |

The two modes, because the difference is the whole feature:

* **Advisory** (`binding_mode = false`) — the settlement pass *reports* a passed
  ballot and closes nothing. The proposal stays `open`, `respond` stays reachable,
  and `retention_policy_changes` records the change only once an operator applies
  it.
* **Binding** (`binding_mode = true`) — a passed ballot commits at
  `closes_at` (`quorum` supporters plus `proposal_cooling_days`), and the change
  is recorded against the instance's system account, because no operator is
  involved and naming a voter would put their own name on the row their ballot
  produced.

A narrowing change is judged at the ordinary bar of three; only a *widening* is
judged at `widen_quorum`.

Export file retention and CTA settings (spec §38, §42).

| Key | Type | Default | Purpose |
|-----|------|---------|---------|
| `retention_days` | `int` | `7` | Days to keep export output (`0` = forever) |
| `grant_ttl_secs` | `int` | `3600` | Download grant lifetime |
| `cta_placement` | `string` | `"per_chapter"` | CTA placement: `per_chapter`, `per_work`, or `off` |
| `cta_html` | `string?` | unset | Custom CTA HTML (sanitized) |
| `cta_quorum` | `int` | `2` | Curator quorum for work-level CTA exemption |

## `[bulk_export]`

Bulk export limits (spec §38).

| Key | Type | Default | Purpose |
|-----|------|---------|---------|
| `max_items` | `int` | `50` | Maximum works in a bulk bundle |
| `max_bytes` | `int` | `1073741824` | Maximum bytes in a bulk bundle (1 GiB) |

## `[revisions]`

Source revision cache settings (spec §38).

| Key | Type | Default | Purpose |
|-----|------|---------|---------|
| `ttl_secs` | `int` | `604800` | Revision cache TTL (7 days) |

## `[jobs]`

Job queue settings (spec §38).

| Key | Type | Default | Purpose |
|-----|------|---------|---------|
| `terminal_retention_days` | `int` | `30` | Days to keep terminal jobs |

## `[library]`

Library update-check settings (spec §38).

| Key | Type | Default | Purpose |
|-----|------|---------|---------|
| `update_check_retention_days` | `int` | `90` | Days to keep update-check records |
| `check_batch` | `int` | `50` | Items per library update job |

## `[media_resilience]`

Media resilience and perceptual deduplication (spec §32.7.2). The table is
optional: with it absent, the defaults below apply.

| Key | Type | Default | Purpose |
|-----|------|---------|---------|
| `enabled` | `bool` | `true` | Whether perceptual dedup runs on this instance |
| `perceptual_hash_algorithm` | `string` | `"dhash"` | Image fingerprint: `dhash`, `phash`, `whash`, `ahash`. Only `dhash` is implemented, and it is the default because a setting naming an unbuilt algorithm makes the instance claim a dedup it cannot perform; asking for another is refused rather than answered with a different algorithm's output. An unknown value stops the instance rather than loading a default, because the alternative is an instance that claims to deduplicate with a hash it never computes |
| `perceptual_match_threshold` | `int` | `6` | Hamming distance at or below which two image hashes are offered as the same image. Must be 1–32; outside that range the instance refuses to start, because a threshold that matches nothing or everything fails silently |
| `require_curator_confirmation_below` | `int` | `3` | A perceptual match scoring below this confidence needs a curator to confirm the linkage. 0–100 |
| `require_curator_confirmation_above` | `int` | `0` | A perceptual match at or above this confidence auto-attaches. The spec's default of `0` means no perceptual match auto-attaches — only exact content-hash matches do. 0–100 |
| `audio_fingerprint` | `string` | `"chromaprint"` | Audio fingerprinting scheme: `chromaprint`, `acoustid` |

The `perceptual_match_threshold` is read by
`POST /api/v1/media/reverse-search`, which returns each match with a
`match_distance`, a `match_confidence` and a `match_kind` of `exact` or
`perceptual`. A reference whose `perceptual_hash` is `NULL` (its bytes were
never fetched) and any hash that is not hex are skipped rather than scored: a
malformed value is not evidence of similarity.

## `[tts]`

Text-to-speech engine settings.

| Key | Type | Default | Purpose |
|-----|------|---------|---------|
| `engine` | `string` | `"piper"` | Engine name (`piper`, `silent`) |
| `piper_path` | `string?` | `None` | Piper binary path |
| `piper_voice_model` | `string?` | `None` | Piper voice model |
| `default_voice` | `string?` | `None` | Default voice |
| `monthly_spend_cap_cents` | `int?` | `None` | Monthly spend cap |

## `[quality]`

The weights of the `quality` composite (M63-38), and which profile each surface ranks with.

`quality` is a weighted mean over five signals, renormalised over whichever ones a row actually has. The weights are an operator's
judgement about their own catalogue, so they are configuration rather than literals in `score_of`.

| Key | Type | Default | Purpose |
|-----|------|---------|---------|
| `default_profile` | `string` | `"balanced"` | Profile for every surface that names none |
| `profiles` | `map<string, QualityWeights>` | `balanced` (implicit) | Named weight vectors |
| `surfaces` | `map<string,string>` | unset | Surface name → profile name |

A profile states any subset of the five weights; unstated ones keep the shipped value, so one line changes one signal:

| Weight | Default | Signal |
|--------|---------|--------|
| `source_pct` | `0.40` | Percentile of likes within the row's own archive |
| `consensus` | `0.25` | Percentile plus the crosspost bonus |
| `density` | `0.15` | Likes per 1k words within the archive |
| `crosspost` | `0.10` | How many archives carry the story |
| `confidence` | `0.10` | How much the archive's sample is worth |

```toml
[quality]
default_profile = "balanced"

[quality.profiles.engagement]
density = 0.40
consensus = 0.30

[quality.surfaces]
catalogue = "balanced"
rec-run = "engagement"
```

Two refusals at load, both naming the string that is wrong: a `default_profile` or surface profile that resolves to nothing, and a
weight vector that cannot rank (negative, `NaN`, or all zero). Weights need not sum to one — the mean is renormalised — so `(0.4,
0.25, 0.15, 0.1, 0.1)` and `(4, 2.5, 1.5, 1, 1)` rank identically. `balanced` resolves even when `[profiles]` never mentions it, because a
TOML table replaces the map rather than merging into it.

## Routes added by the facet surfaces

| Route | Audience | Notes |
|-------|----------|-------|
| `GET /api/v1/facets` | public | length, status, rating, cadence counts for the current filter set, from one scan |
| `GET /api/v1/facets/words?buckets=10` | public | the word-count distribution: **population-equal** buckets with `low`/`high`/`count`/`share`, plus `measured`/`unknown`/`when_unknown` |

Unmeasured works are counted in `unknown` and excluded from every bucket — `revisions.word_count` is `NOT NULL`, so
"unmeasured" means a unit with no current revision, which is what a metadata-only import leaves behind. A chart drawn
from this endpoint cannot show an invented bar at zero.

## `[fandom_graph]`

The operator's fandom adjacency map (M64-08). Declared, never inferred: some fandoms are one community under two names — *Worm* and
*Ward*, *Naruto* and *Boruto*, the MCU films and the 616 comics — and no co-occurrence count will say so on a catalogue where those
fandoms have never appeared side by side.

| Key | Type | Default | Purpose |
|-----|------|---------|---------|
| `version` | `string` | `"unpublished"` | Identity of the map; travels on every response that used it |
| `edges` | `list<{from, to, weight}>` | `[]` | The declared relationships |

```toml
[fandom_graph]
version = "2026-11-07"

[[fandom_graph.edges]]
from = "Worm"
to = "Ward"
weight = 0.9

[[fandom_graph.edges]]
from = "SpaceBattles"
to = "SufficientVelocity"
weight = 0.6
```

Edges are **undirected** — write `Worm <-> Ward` once. Weight is `0 < weight <= 1` and is carried through to the ranking rather than
multiplied into a popularity count, so "40 likes in a fandom the operator called 0.9 related" and "400 likes behind a 0.4 edge" stay
two different claims. A self-loop, a weight outside `(0, 1]`, an unnamed end, or an empty `version` is refused at load with the edge
named. A duplicated pair keeps the strongest weight and its spelling, so the map does not depend on line order.

Served at `GET /api/v1/fandoms/{fandom}/related` — the map version, the neighbours with their weights, and the readable works they
reach. An unmapped fandom answers with empty lists rather than a 404: "nothing is declared next to this" is a true answer.

## `[forum]`

Forum, vote budget, and karma settings (spec §35).

| Key | Type | Default | Purpose |
|-----|------|---------|---------|
| `work_discussion_default` | `string` | `"thread_only"` | Default discussion mode |
| `vote_budget` | `map<string,int>` | (see spec §35.2) | Vote budget rungs by trust level |
| `karma_decay_percent` | `int` | `5` | Monthly karma decay % |
| `meta_mod_points` | `int` | `1` | Points per meta-mod verdict |
| `meta_mod_min_verdicts` | `int` | `3` | Min verdicts to elect moderator |
| `min_vote_weight_bp` | `int` | `100` | Min vote weight (basis points) |

## `[browse]`

Browsing defaults (spec §43).

| Key | Type | Default | Purpose |
|-----|------|---------|---------|
| `default_sort` | `string` | `"for-you"` | Default sort for signed-in readers |
| `anonymous_sort` | `string` | `"top"` | Default sort for anonymous visitors |
| `surface_defaults` | `map<string,string>` | unset | Per-surface overrides |

## `[weighting]`

Ranking weights (spec §16.16).

| Key | Type | Default | Purpose |
|-----|------|---------|---------|
| `mode` | `string` | `"trust_taste_contribution"` | Weighting mode |
| `taste_floor` | `float` | `0.75` | Taste multiplier floor |
| `taste_ceiling` | `float` | `1.25` | Taste multiplier ceiling |
| `contribution_floor` | `float` | `1.0` | Contribution multiplier floor |
| `contribution_ceiling` | `float` | `2.0` | Contribution multiplier ceiling |
| `contribution_window_days` | `int` | `180` | How far back contribution counts |
| `demand_diversity_percent` | `int` | `20` | Fraction of surfaced demand with no boost |

## `[discovery]`

Discovery settings: the feed's diversity, the recommender, the arena, and the switches that open one row's doors at a time.

| Key | Type | Default | Purpose |
|-----|------|---------|---------|
| `taste_sources` | `[table]` | `[{ kind = "admin" }]` | Taste sources and members |
| `taste_source_min_members` | `int` | `5` | Cohort-size floor |
| `per_fandom_cap` | `int` | `0` | Most works from one fandom per feed response; `0` = unlimited |
| `exploration_rate` | `float` | `0.0` | Fraction of a feed kept for fandoms the reader has not seen |
| `enable_half_life` | `bool` | `true` | Whether the half-life job runs and scores influence ranking |
| `half_life_min_age_days` | `int` | `14` | Age before a work is eligible for half-life scoring |
| `half_life_window_days` | `int` | `30` | Window for both reader counts |
| `rec_mode` | `string` | `"legacy"` | `legacy`, `pluggable` or `shadow` (§16.1a) |
| `rec_rrf_k` | `float` | `60.0` | RRF constant for the strategy blend |
| `rec_per_strategy_cap` | `int` | `100` | Most results one strategy may contribute |
| `rec_enabled_strategies` | `[string]` | `[]` | Strategies in play; empty = all |
| `experiment_buckets` | `int` | `0` | Arms per surface; `0` = no experiment (M64-02) |
| `arena_epsilon` | `float` | `0.15` | Share of arena rounds that explore instead of pairing proximate works (M69-11) |
| `live_badges` | `bool` | `false` | Whether badge state is decided and shown (M69-14) |
| `peering_suggestions` | `bool` | `false` | Whether the operator's peering suggestions door is open (M69-02) |
| `enforced_rules` | `[string]` | `[]` | Rules this instance enforces unconditionally, as declared here (M69-02) |
| `taste_passports` | `bool` | `false` | Whether readers export and present signed passports (M69-03) |
| `passport_key` | `string` | absent | Hex ed25519 signing key; export refuses without one (M69-03) |
| `work_tombstones` | `bool` | `false` | Whether tombstones are enforced and their doors open (M69-04) |
| `work_mirroring` | `bool` | `false` | Whether works may be mirrored and attribution shown (M69-05) |
| `mirror_floor` | `float` | absent | Taste similarity a peer needs to hold a mirror (M69-05) |
| `media_rescue` | `bool` | `false` | Whether the rescue ring is open (M69-06) |
| `evacuation` | `bool` | `false` | Whether an evacuation may be announced and planned (M69-07) |
| `morning_queue` | `bool` | `false` | Whether `/discovery/morning` serves a queue (M69-22) |
| `morning_queue_view` | `string` | absent | Slug of the saved filter the queue is drawn from; absent = `404`, never an invented filter |
| `morning_queue_size` | `int` | `5` | Works in the queue, clamped to 1–10 |
| `daily_surprise` | `bool` | on when the queue is on | Whether one work is drawn by (day, reader) |
| `saved_filters` | `bool` | `false` | Whether saved filters have a keybind, follows and blends (M69-21) |
| `surfaces` | `[table]` | `{}` | Per-surface algorithm choice (M64-01) |

**Everything from `live_badges` down is off by default, and that is the rule, not an accident.** A switch that makes a
claim about a reader — a badge, a passport, a peering suggestion, a queue drawn from somebody's filter — is off until the
operator turns it on, because a default-on claim is one nobody chose. When a switch is off its doors answer `404`, not `403`
(ADR 0030), so a reader cannot tell a switched-off surface from one that never existed.

## `[directory]`

Resource directory settings (spec §39).

| Key | Type | Default | Purpose |
|-----|------|---------|---------|
| `page_size` | `int` | `50` | Entries per page |
| `require_approval` | `bool` | `true` | Submissions need approval |
| `extra_categories` | `[string]` | `[]` | Operator-added categories |
| `weighting` | `string` | `"trust_and_taste"` | Vote weighting mode |
| `trust_vote_weights` | `[float]` | `"0.5,0.75,1.0,1.25,1.5,1.75,2.0"` | TL0–TL6 multipliers |
| `taste_floor` | `float` | `0.75` | Taste floor |
| `taste_ceiling` | `float` | `1.25` | Taste ceiling |

## `[rate_limits]`

Rate-limit quotas (spec §3.8).

The keys are **flat** — `<class>_burst` and `<class>_per_minute`, one line each.
A nested form (`auth = { burst = 5, per_minute = 20 }`) is not what the parser
accepts, and neither is a bare `burst` or `per_minute`. An unrecognised key is a
hard parse error, not a warning, so a config written from the shape this table
used to imply fails at startup with the field list in the message.

| Key | Default |
|-----|---------|
| `auth_burst` | 10 |
| `auth_per_minute` | 30 |
| `write_burst` | 20 |
| `write_per_minute` | 60 |
| `search_burst` | 30 |
| `search_per_minute` | 120 |
| `export_burst` | 30 |
| `export_per_minute` | 120 |
| `default_burst` | 120 |
| `default_per_minute` | 600 |
| `address_multiplier` | 4 |

`address_multiplier` scales the address-keyed bucket only, so one address behind
a shared NAT absorbs more before any individual account is affected. Zero is
rejected at startup: a zero burst would make every request un-rate-limited and
zero would make every request refused.

## `[accounts]`

Account settings.

| Key | Type | Default | Purpose |
|-----|------|---------|---------|
| `registration_open` | `bool` | `true` | Allow new registrations |

## `[age]`

Age-gate settings.

| Key | Type | Default | Purpose |
|-----|------|---------|---------|
| `threshold` | `int` | `14` | Age of consent threshold |
| `guardian_workflow_enabled` | `bool` | `false` | Guardian authorization flow |

---

## `[federation]` — what this instance accepts from other instances

Spec §25.1. One key.

```toml
[federation]
# Default: true.
require_signature = true
```

`require_signature` decides what `POST /federation/inbox` does with an activity that carries no valid HTTP signature
(draft-cavage, `rsa-sha256`). With the default every arriving activity must carry a `Signature` header covering
`(request-target)`, `host`, `date` and `digest`, with a `Date` inside a five-minute window and a `Digest` matching the bytes
actually received; the peer's key is read from its actor document (`publicKey.publicKeyPem`) and cached for the life of the
process.

**Why the default is `true`.** An inbox that accepts unsigned writes is a public write endpoint with a federation-shaped path:
anybody on the network can write an activity, and the instance has no way to tell a peer from an impersonator. Every arrival is
recorded in the intake ledger either way, and the ledger's `state` is the answer to "was this verified":

| Outcome | Response | Ledger row |
|---|---|---|
| Signature verifies | `200 {"status":"received"}` | `accepted`, note names the key id |
| Signature does not verify | `401` | `rejected`, note says why |
| No signature, `require_signature = true` | `401` | `rejected`, note `no Signature header` |
| No signature, `require_signature = false` | `200` | `quarantined` — waiting for the operator |

An operator who runs an open inbox for a peer they trust at the network layer turns this off; the rows stay `quarantined` so the
difference between "verified" and "allowed" never disappears from the record. Review and decide rows at
`GET /operator/federation/inbound` and `POST /operator/federation/inbound/{id}/state`.

## Startup errors

A malformed value refuses to start with a clear error message. Examples:

- `retention_days = -1` → "exports.retention_days must be >= 0, got -1"
- `media_resilience.{key} is a confidence percentage and must be between 0 and 100`
- An unknown key in `[roadmap]` or `[retention_governance]` is refused with
  `unknown field \`…\``, naming the key. A misspelled governance setting that
  loaded as a default would be a setting that reads as applied and is not

- `grant_ttl_secs = 0` → "exports.grant_ttl_secs must be > 0, got 0"
- Unknown TOML key → "unknown field `[section].foo`"

## Empty configuration

A configuration with all defaults omitted produces a working instance. The
shipped `darkhold.toml.example` documents every setting with comments.

## `[syndication]` — the periodic walk over external sources (`M67-03`)

Off by default: a fresh instance that starts fetching eight archives on boot is a surprise, and
outbound traffic is the one kind of default somebody else can see.

```toml
[syndication]
enabled = false            # an operator turns it on
interval_hours = 12        # one pass per window; 0 is floored to 1 so it cannot spin the queue
tags = ["Naruto", "Mass Effect"]   # what this instance cares about — a fandom list, not a crawl frontier
max_works_per_listing = 25         # per (source, tag, run); a 40,000-work tag is not 40,000 jobs
account_handle = "Syndication"
account_email = "syndication@darkhold.invalid"
```

The pass reads the catalogue in `crates/scrapers/sources.json`: 33 platforms, each with a **dated
`robots.txt` verdict** and a bodies policy. It walks only entries with an adapter *and* a discovery shape,
and there are two shapes: a **tag listing** (`/tags/{tag}/works`, one page per configured tag — the three
AO3-software archives) and a **tagless listing** (Scribble Hub's `/series-ranking/`, walked once per pass).
Each discovered work becomes one `Import` job, keyed `syndication:{source}:{work_id}`, so a second pass
over the same listing queues nothing.

`royalroad.com` is listed as **blocked**: its `robots.txt` allows the pages, but the site answers `403` to
every listing and work page from this host whatever the user agent, so nothing walks it. The adapter stays —
the code is right, the host is refusing — and an instance on another network should re-check the verdict. Each discovered work becomes one `Import`
job through the existing pipeline, keyed `syndication:{source}:{work_id}`, so a second pass over the same
listing queues nothing.

Bodies are governed by the source's own `robots.txt`, re-read at run time: `metadata_only` sources are
read for their catalogue and the reader is sent to the source for the prose, and `blocked` sources are not
fetched at all.

Imported works are owned by the **instance account**, with one pseud per source (`src-<source_id>`,
displayed as the archive's name) — a reader sees "SquidgeWorld" as the author of a work that came from
SquidgeWorld, and the operator's own profile is not borrowed to stand in for an archive.

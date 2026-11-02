# 32. Generalized Media Platform: Creators, Distributors, Collections, and the Media Query API

**Status:** reserved
**Milestone:** —
**Source:** §32 of `docs/spec.md`

Darkhold generalizes from "a fanfiction platform that also hosts media" to a
platform for written and recorded media of any kind — fiction, poetry, essays,
books, translations, audio, video, comics, scans, and preserved archive items —
such that it can stand in for AO3, FanFiction.net, Wattpad, StoryGraph,
Goodreads, the Internet Archive, Literotica and their peers. Every
differentiator in §0 is a default, never a casualty: positivity-first
feedback, trust-level governance, reader content controls, privacy,
quality-first curation, self-hosting. Design rationale lives in the review
note (`secondbrain: 10-projects-darkhold-platform-redesign-spec.md`) and
ADR 0019; this section is normative.

"Drop-in replacement" means three guarantees, never protocol emulation of
another site's private API:

1. **Data in.** Content migrates from peer sites through adapters (§11.1)
   and site APIs where they exist.
2. **Feature parity.** Every user-facing behavior a person relies on at
   those sites exists here in generalized form.
3. **Data out.** Everything the UI can show, the API can return: filtered,
   paginated, subscribed, and exportable. Scraping this instance is
   impossible by construction; scraping exists only in the ingestion
   direction, toward other sites.

## 32.1 Milestone 22 — media entity model

**Implement.** Migration 0024 (both dialects, per ADR 0004): `creators`
(local pseuds and external records; an external creator is never merged
into a local account by name or handle, §11.11; verification is quorum,
§19), `media_creators` attribution edges, `distributors` and
`distributorships` (who made it available, and how: published, hosted,
mirrored, preserved, narrated, translated, reprinted), `media_collections`
and `media_collection_items` (one typed-membership model for series,
anthologies, reading lists, archive collections, challenge anthologies,
preserved batches — M13 event collections keep their own tables),
`media_editions` (publication history; revisions stay the editing history),
`media_rights` (license, rights statement, lending class — `ai_training`
stays on works, §24.14), `quality_signals` (typed, sourced, recomputable,
0..=1000), and `works.format` defaulting to `prose` (§30.1's taxonomy,
widened: prose, poetry, essay, article, book, fanwork, translation, podfic,
audiobook, fan_film, video, fan_comic, comic, zine_scan, image,
interactive, dataset, other).

Domain rules in `darkhold-domain::media`: the vocabularies above with
`FromStr` round trips, creator-record consistency, quality-signal bounds.

**Acceptance.**
- 0024 applies on SQLite and PostgreSQL and the parity check passes.
- A creator record that is neither properly local nor properly external is
  refused at the edge.
- Pre-M22 works read as `format = prose` everywhere format is displayed.

## 32.2 Milestone 23 — the media query engine and API

**Implement.** One query engine behind seven doors, all returning the same
`MediaRecord` shape (id, format, title, creators, distributorships,
collections, canon, rating, warnings, dates, quality, language, status,
length/runtime, transcript availability, rights, files, canonical URL):

```text
GET  /api/v1/media                      GET  /api/v1/media/{id}
GET  /api/v1/media/{id}/files           GET  /api/v1/media/{id}/editions
GET  /api/v1/creators[/{id}][/media]    GET  /api/v1/distributors[/{id}][/media]
GET  /api/v1/media-collections[/{id}][/media]
GET  /api/v1/canons/{id}/media          GET  /api/v1/spaces/{id}/media
POST /api/v1/media/query                (complex queries as JSON)
```

Filter dimensions: quality (`min_quality`, individual signals,
`min_creator_trust`), dates (published/updated/ingested), format and media
type, rating, warnings and content notes (§15.16), completion status,
language, length or runtime, canon(s) and crossover, namespaced tags, mood
(§15), license, transcript availability (§30.8), preservation status; and
for authenticated callers, library-local filters (§14). Cursor-paginated,
stable-sorted, ETag/304 per query. Every query renders as Atom/RSS (§23.5)
and as an OPDS acquisition feed; every query can be watched by a webhook
(§21) and exported as a grant-gated bundle (§13, fair-queued §20). Public
media pages embed JSON-LD (`CreativeWork` family); `?format=dc` returns
Dublin Core.

**The ranking philosophy is filters, not scores.** A composite quality
score is instance configuration (weights documented, never purchasable,
§0.3); it powers filters and sorts but is not displayed as a public
leaderboard unless the operator chooses.

**Acceptance.**
- "All media by creator X", "all media in collection Y", and "all media in
  canon Z" are single authenticated-or-public calls, filtered by quality
  and date, cursor-stable.
- A query's ETag answers 304 when nothing eligible changed.
- A query renders as OPDS and downloads into an e-reader client without a
  bespoke client implementation.
- No response leaks media the caller is not eligible for (§7.6 on every
  door, including collection and canon scopes).
- Bulk export of a large query completes through the job queue, is rate
  limited and fair-queued, and never bypasses download grants.

## 32.3 Milestone 24 — site parity features

**Implement.** Anchored comments on any unit position — paragraph offsets
for text, timestamps for media (§12, §30.4); orphaning (a creator detaches
and the media persists under its provenance, pairing with succession
§24.15); a creator dashboard of aggregate, privacy-preserving, positivity-
framed statistics (§24.3 rules; no public shaming numbers); half-star
rating granularity as a reader setting (§9); shelf import from
Goodreads/StoryGraph CSV and works import from the Wattpad and AO3 APIs as
adapters (§11.1, ingestion only); bulk manuscript import (doc/epub/txt)
into the editor (§8); per-format reading goals (words, minutes, items).

**Acceptance.**
- An orphaned work keeps its comments, stats and editions and loses no
  eligibility; the orphaning is reversible only by quorum.
- A StoryGraph CSV import produces library states and reviews that respect
  the reader's existing ratings and dates, and refuses rows it cannot map,
  naming them.
- Anchored comments on a media unit address a timestamp, and on a text
  unit a paragraph, and both flow through the positivity filter (§12).

## 32.4 Milestone 25 — archive mode

**Implement.** Derivative pipeline over `content_blobs` (EPUB/PDF/text
renditions, OCR for scans, transcode for uploaded media) as jobs (§10.4);
full-text search joining transcripts and OCR text (§15, §30.8);
public-domain collections driven by the rights field; optional controlled
digital lending — **operator decision, default off** — with copy caps, loan
expiry, and revocation; IA-style item metadata export (Dublin Core,
§32.2); vanished-source marking (§30.10) extended to derivatives.

**Acceptance.**
- An instance with lending off serves rights metadata and refuses loan
  requests naming the policy.
- A loan grants one reader a bounded window, expires, revocates, and never
  multiplies copies beyond the configured cap.
- A scanned item with OCR text is full-text searchable and the OCR is
  labeled as machine-produced (§22.6).
- Every derivative records its parent blob checksum and re-verifies on a
  schedule.

## 32.5 Milestone 26 — adult content and audio parity

**Implement.** Adult category taxonomy as ordinary canon/tag data behind
the existing age and content-eligibility gates (§7.3, §7.6) — never
leaking into unauthenticated surfaces or feeds; author-approved TTS
narration as an edition (`narration`), machine audio labeled per §22.6/§30.8;
timestamp-anchored comments already covered by M24; gallery mechanics
(§21) applied to illustrated works.

**Acceptance.**
- An anonymous, underage, or opted-out reader meets zero adult items in
  any door, feed, OPDS catalog or search result.
- A narration edition is a first-class edition: it appears in editions
  lists, downloads as audio, and credits its narrator via
  `media_creators`.
- A TTS-generated narration is labeled machine-produced and is replaceable
  by an author recording without destroying the machine edition.

## 32.6 What this section deliberately does not do

- It does not emulate other sites' API shapes; interop is via OPDS,
  Dublin Core, JSON-LD, Atom and ActivityPub announcements (§23.6).
- It does not merge M13 event collections into `media_collections`; the
  event model stays, and a challenge anthology links across via
  `media_collections.challenge_anthology`.
- It does not display composite quality scores publicly by default, and no
  credit, payment or trust level can move any ranking signal (§0.3).
- It does not federate queries (§23.6 stays announce/notify); remote
  queries would be a scraping vector wearing a protocol.

## 32.7 Media Resilience & Availability Guarantee

Fanfic works reference external media constantly: character faceclaims on
Pinterest, moodboards on Tumblr, playlists on Spotify, reference art on
Twitter, cosplay photos on Instagram, dead links to fanart on defunct hosting
services. When these links break — and they always do — the reading experience
degrades silently. The reader doesn't know what they're missing; the author
doesn't know their work has decayed.

This section specifies a **Media Availability Guarantee System**: multi-link
redundancy with curated mirrors, continuous link health monitoring, and
community curator maintenance so readers always see a working link. When the
primary source dies, the reader is transparently served from a mirror. This is
like Wikipedia's citation preservation model (via Internet Archive) but
purpose-built for fanfic's specific media patterns.

### 32.7.1 The Media Reference Graph

Extend the existing media model with a **reference layer** that tracks every
piece of media *referenced by* a work (as opposed to media *hosted on* the
instance).

```rust
struct MediaReference {
    id: Uuid,
    perceptual_hash: PerceptualHash,        // for dedup and reverse-lookup
    content_hash: Blake3Hash,                // exact-match dedup
    media_kind: MediaKind,                   // image | audio | video | document | embed
    first_seen_at: DateTime,
    canonical_metadata: CanonicalMetadata,   // dimensions, duration, format, size
    content_notes: Vec<ContentNote>,         // NSFW, spoiler, disturbing imagery, etc.
    curator_verified: bool,
}

struct AvailabilityLink {
    id: Uuid,
    media_reference_id: Uuid,
    url: Url,
    provider: LinkProvider,                  // pinterest | tumblr | imgur | archive.org | local_mirror | ipfs | ...
    status: LinkStatus,                      // healthy | degraded | dead | quarantined | pending_verification
    last_checked_at: DateTime,
    last_healthy_at: Option<DateTime>,
    consecutive_failures: u32,
    added_by: AccountId,
    verified_by: Vec<AccountId>,             // curators who confirmed this link matches the reference
    reported_broken_by: Vec<AccountId>,      // readers who flagged it dead
    priority: u16,                            // higher = tried first (canonical > mirror > archive)
}

struct WorkMediaReference {
    work_id: Uuid,
    chapter_id: Option<Uuid>,
    media_reference_id: Uuid,
    context: MediaContext,                    // faceclaim(character_id) | moodboard | playlist | fanart | reference | inline_embed
    display_url: Url,                         // computed at render time from best available link
    author_note: Option<String>,             // "Elara's faceclaim", "chapter 3 soundtrack"
    inserted_at: DateTime,
}
```

**The three-link minimum invariant:** Every media reference must maintain
**≥3 healthy availability links** at all times, with at least one being an
**archival link** (Internet Archive, local mirror, or IPFS pin). When link
count drops below 3, the reference enters a **curator bounty state**
(see §32.7.5).

### 32.7.2 Perceptual Hashing & Deduplication

When a work is imported or an author inserts media, the system computes both
an exact content hash and a **perceptual hash** for fuzzy matching.

**Image perceptual hashing:** pHash, dHash, or wHash (configurable). Detects
the same image at different resolutions, formats, minor edits, watermarks,
compression variations. Threshold-based matching (e.g., Hamming distance ≤ 6
= same image).

**Audio perceptual hashing:** Chromaprint / AcoustID fingerprints. Detects
the same song across different bitrates, formats, minor edits.

**Video perceptual hashing:** Keyframe-based pHash sequences. Detects the
same video across encodings.

**Deduplication behavior:**

When a new URL is added, the fetcher (SSRF-safe, per existing §6) retrieves
the media, computes hashes, and checks against existing references:

- **Exact match** (content_hash identical): Attach as a new AvailabilityLink
  to the existing MediaReference. Zero new storage cost.
- **Perceptual match** (perceptual_hash within threshold): Present the curator
  with a match confidence score; they confirm or reject the linkage.
- **No match:** Create a new MediaReference.

This means **a single popular faceclaim image referenced across 500 fics has
one MediaReference and potentially hundreds of AvailabilityLinks** — one
dying link doesn't affect the others, and one new mirror benefits all 500
fics simultaneously.

**Config:**

```toml
[media_resilience]
enabled = true
perceptual_hash_algorithm = "dhash"          # dhash (implemented) | phash | whash | ahash
perceptual_match_threshold = 6                # Hamming distance
audio_fingerprint = "chromaprint"
require_curator_confirmation_below = 3        # perceptual matches with distance 3-6 need curator confirm
require_curator_confirmation_above = 0        # exact matches auto-attach
```

### 32.7.3 Reverse Media Search

Extends beyond deduplication to become a **first-class discovery feature**.

**For authors:** Upload or paste a URL → find all existing references to the
same media across the archive. "This faceclaim is used by 47 other characters
in this fandom." "This moodboard already exists with 3 healthy mirrors" →
one-click attach instead of adding a new URL.

**For readers:** Right-click any image in a fic → "Find works using this
media" → cross-work discovery. "Readers who bookmarked works using this
faceclaim also bookmarked…" → new recommendation signal (feeds into
meta-ranker as strategy `MediaReferenceCollaborative`). **Reverse-search
external images:** Paste a URL from outside the archive → find works that
reference the same image.

**For curators:** "Find all references to media hosted on tumblr.com that
haven't been mirrored in 90 days" → proactive curation queue. "Find
MediaReferences with < 3 healthy links" → curator bounty queue (§32.7.5).

**API:**

```
POST /api/v1/media/reverse-search
Body: { url: string } OR { image: base64 } OR { hash: string, algorithm: string }
Returns: { references: [MediaReference], works: [Work] }
```

### 32.7.4 Link Health Monitoring

A background job continuously checks the health of every AvailabilityLink.

**Check cadence (adaptive):**

| Link age | Consecutive failures | Check frequency |
|----------|---------------------|-----------------|
| < 7 days old | 0 | Every 24h |
| < 30 days old | 0 | Every 3 days |
| < 90 days old | 0 | Every 7 days |
| > 90 days old | 0 | Every 14 days |
| Any age | 1 | Every 24h |
| Any age | 2 | Every 6h |
| Any age | 3+ | Every 1h until resolved |

**Failure detection:**
- HTTP status codes (4xx, 5xx → likely dead)
- Redirect chains (redirect to homepage, generic error page, "content unavailable" pages)
- Content-type mismatch (link was image, now returns HTML)
- Perceptual hash mismatch (link still returns image, but it's a different image — hijacked or replaced with a "content removed" placeholder)
- Response time > 10 seconds (degraded)
- SSL/TLS certificate failures

**Reader-reported failures:** Any reader can flag a broken link with one click.
Three unrelated reports (different accounts, different IPs) within 48 hours
triggers immediate re-verification.

**Status transitions:**

```
healthy → degraded (1-2 failures, or slow response)
degraded → dead (3+ failures over 72h, or reader reports confirmed)
dead → quarantined (dead for 30+ days, deprioritized in link ordering)
quarantined → dead (removed from active rotation but kept for historical record)
* → pending_verification (curator re-added or re-verified)
```

**Never delete dead links.** Historical record is preserved. This lets curators
later attempt recovery and provides forensic data on link rot patterns.

**Config:**

```toml
[media_resilience.monitoring]
enabled = true
check_workers = 4
respect_robots_txt = true
respect_rate_limits = true
user_agent = "Darkhold-Media-Monitor/1.0 (+https://instance.example/about/monitor)"
max_response_size_mb = 50
reader_reports_to_trigger_check = 3
reader_reports_window_hours = 48
```

### 32.7.5 Curator Role & Bounty System

Curators are users who maintain media availability. This is a new role,
distinct from Vanguard (§16.18) — Vanguards curate *taste*, Curators curate
*infrastructure*.

**Becoming a Curator:**

```toml
[media_curator]
selection_method = "trust_level"    # trust_level | admin_appointment | contribution_volume | hybrid
min_trust_level = 3                  # TL3+ can opt-in to Curator role
requires_opt_in = true               # users must actively choose the role
max_curators_per_instance = 50       # 0 = unlimited
```

**Curator credit rewards** (feeds existing economy, §9.7):

| Action | Credits | Config |
|--------|---------|--------|
| Add a new working AvailabilityLink to an under-mirrored reference (< 3 links) | 15 | `curator.mirror_add_bounty` |
| Add an archival link (Internet Archive, IPFS pin, local mirror) | 25 | `curator.archive_add_bounty` |
| Verify a link matches a MediaReference (perceptual confirm) | 3 | `curator.verify_bounty` |
| Confirm a reported broken link is actually broken | 2 | `curator.confirm_broken_bounty` |
| Rescue a dead reference (add a working link to a reference with 0 healthy links) | 50 | `curator.rescue_bounty` |
| Add content notes to a MediaReference (NSFW, spoilers, disturbing) | 5 | `curator.content_note_bounty` |
| Merge duplicate MediaReferences (perceptual confirm across two entries) | 10 | `curator.merge_bounty` |

**Standing bounties** (extension of §20.3.1):

```toml
[[curator.standing_bounties]]
name = "Mirror all Tumblr-hosted images"
criteria = { provider = "tumblr.com", healthy_links_below = 3 }
reward = 30
enabled = true

[[curator.standing_bounties]]
name = "Archive-link every reference for admin-favorited works"
criteria = { work_admin_rating_min = 4, has_archive_link = false }
reward = 50
enabled = true
```

**Anti-gaming:**
- Curator credits are capped per day (existing §9.7 caps apply)
- Adding a link that fails health check within 7 days: credits clawed back
- Perceptual confirmation requires two independent curators (2-of-N quorum)
  before crediting the second confirmer
- Curators cannot verify links they added themselves
- Self-promoted mirrors require higher quorum

### 32.7.6 Mirroring & Archival Strategies

**Automatic Internet Archive submission:** When a media reference is added
and doesn't yet have an archive.org snapshot, the system automatically submits
the URL to the Wayback Machine's Save Page Now API and adds the resulting
archive URL as an AvailabilityLink.

```toml
[media_resilience.archive_org]
enabled = true
auto_submit_on_add = true
auto_submit_on_first_failure = true      # if primary link degrades, snapshot before it dies
respect_rate_limits = true
```

**Local mirroring:** Off by default (copyright caution).

```toml
[media_resilience.local_mirror]
enabled = false
max_storage_gb = 100
max_file_size_mb = 50
image_formats = ["jpg", "png", "webp", "avif", "gif"]
audio_formats = ["mp3", "ogg", "opus"]
video_formats = []
mirror_conditions = [
  { work_admin_rating_min = 4 },
  { healthy_links_below = 2 },
]
retention_policy = "keep_while_referenced"
storage_backend = "local_fs"
```

**Copyright-respectful defaults:** Only mirrors user-generated content (fanart,
moodboards, faceclaims). Never mirrors commercial sources (Getty, Shutterstock,
official studio releases). DMCA takedown endpoint removes local mirrors on
request (does not delete the MediaReference itself).

**IPFS pinning:**

```toml
[media_resilience.ipfs]
enabled = false
pin_service = "pinata"
auto_pin_conditions = [
  { work_admin_rating_min = 4 },
  { curator_verified = true },
]
```

**Federated mirroring:**

```toml
[media_resilience.federation]
enabled = false
mirror_pool_share_percent = 5
trusted_instances = ["https://otherinstance.example"]
```

### 32.7.7 Reader Experience

The reader never sees the complexity. They see media that just works.

**Rendering pipeline:**
1. When a work is rendered, every media reference is resolved via
   `get_best_available_link(reference_id)`.
2. Returns the highest-priority healthy link, preferring: canonical original
   > curator-verified mirror > archive.org snapshot > local mirror > IPFS gateway.
3. If all links are dead, the reader sees a graceful placeholder: "This media
   is currently unavailable. [Report] [View original URL] [Search for replacement]".
4. "Search for replacement" runs a reverse image search across the archive.

**Broken link reporting UI:** A small icon next to every embedded media
element. One click reports the link as broken. Contributes to the reader's
engagement credits (small reward, capped) and triggers re-verification.

**Preload preferences (user-configurable):**

```
Settings → Reading → Media
[✓] Preload media when I open a work (default off, saves bandwidth)
[✓] Show placeholder for adult media until I click (respects age gate)
[✓] Prefer archived versions over live sources (privacy-preserving)
[ ] Only show media from local mirrors (maximum privacy, may hide some media)
```

**Privacy-preserving mode:** When enabled, the reader's browser never contacts
external hosts. All media is served through the instance's local mirror or
archive.org proxy.

### 32.7.8 Author Experience

**Media insertion UI:**
1. Author pastes a URL → system immediately checks if URL already exists as
   an AvailabilityLink → shows: "This image is already in our system with 5
   healthy mirrors. Reusing existing reference."
2. If new, system fetches and computes hashes → checks perceptual matches →
   shows: "This looks similar to an existing image (87% match). Same image?"
   [Yes] [No]
3. If truly new, system creates MediaReference and immediately submits to
   Internet Archive.
4. Author sees a health indicator: "🟢 3+ healthy links (excellent)" or
   "🟡 Only 1 link — consider adding a mirror".

**Author dashboard — Media Health:**

```
Your Works — Media Health Report

"The Long Way Home" (Chapter 1-12)
├─ 47 media references
├─ 42 references with 3+ healthy links ✓
├─ 3 references with 1-2 healthy links (needs mirroring) ⚠
├─ 2 references with 0 healthy links (broken!) ✗
└─ [View report] [Post curator bounties for repairs]
```

Authors can spend their own credits to post targeted curator bounties for
their specific works.

### 32.7.9 Import Integration

When works are imported from other archives:
1. Every media URL in the work is extracted
2. Each URL is queued for perceptual hashing and deduplication
3. Existing MediaReferences are reused; new ones are created
4. All new references are automatically submitted to Internet Archive
5. Import is not blocked on media processing (async pipeline)
6. Import summary: "23 media references processed: 18 already in system,
   5 new (submitted to archive). 2 references had no working link — flagged
   for curator attention."

**Bulk media rescue during import:**

```toml
[import.media_rescue]
enabled = true
aggressive_mirror_sources = ["wattpad.com", "fanfiction.net"]
rescue_deadline = "2027-01-01T00:00:00Z"
```

### 32.7.10 Marketplace Integration

Recalling §9.10 (marketplace), several extension types support media
resilience:

- **Media classifiers** (WASM extensions): custom perceptual hash algorithms,
  content note detectors, NSFW classifiers
- **Mirror providers** (adapters): pluggable backends for new mirror services
- **Import adapters with media rescue**: source-specific adapters that know how
  to bulk-extract media from dying platforms
- **Curator tools**: bulk-verification interfaces, batch-mirror utilities,
  forensic link-rot dashboards

### 32.7.11 Metrics & Admin Dashboard

Admin dashboard (`/admin/media-health`):

- **Overall health:** % of media references with ≥3 healthy links, trend over time
- **Link rot rate:** links dying per week, by provider (Pinterest: 2.3%/week,
  Tumblr: 4.1%/week, Imgur: 0.8%/week…)
- **Rescue rate:** how quickly curators respond to under-mirrored references
  (median time from < 3 links to ≥ 3 links)
- **Curator leaderboard:** top curators by rescues, mirrors added, verifications
- **Standing bounty status:** which bounties are active, how much paid out
- **Storage usage:** local mirror storage consumed, IPFS pins active
- **Provider reliability:** ranked list of external providers by health rate

### 32.7.12 Interaction with Existing Systems

- **Signal Weighting (§9.7.3):** Curator actions are infrastructure, not taste.
  Curator credit rewards are flat (or trust-level-scaled), not resonance-weighted.
- **Meta-Ranker (§9.10):** New strategy available: `MediaReferenceCollaborative`
  — recommends works based on shared media references.
- **Foundational Protections (§0):** Media referenced only in private/unlisted
  works is still mirrored but never appears in reverse search results visible
  to others. Media in drafts is not mirrored until publication.
- **Content Eligibility (§4):** Media inherits content notes and age-gating
  from its referencing works, plus its own content notes if added by curators.
- **Federation (§8):** Media references and health status can federate; actual
  media content does not.

### 32.7.13 Spec Addition Summary

```
§32.7 Media Resilience & Availability Guarantee
  §32.7.1 Media Reference Graph — MediaReference, AvailabilityLink, WorkMediaReference
  §32.7.2 Perceptual Hashing & Deduplication — pHash/Chromaprint, exact + fuzzy match
  §32.7.3 Reverse Media Search — author/reader/curator interfaces
  §32.7.4 Link Health Monitoring — adaptive check cadence, failure detection
  §32.7.5 Curator Role & Bounty System — new role, credit rewards, standing bounties
  §32.7.6 Mirroring & Archival Strategies — Archive.org, local, IPFS, federated
  §32.7.7 Reader Experience — transparent fallback, broken link reporting
  §32.7.8 Author Experience — media health dashboard, insertion UI
  §32.7.9 Import Integration — media rescue during imports
  §32.7.10 Marketplace Integration — classifier extensions, mirror providers
  §32.7.11 Metrics & Admin Dashboard
  §32.7.12 System Interactions
```

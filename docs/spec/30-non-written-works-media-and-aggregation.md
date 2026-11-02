# 30. Non-written Works, Media, and Aggregation

**Status:** reserved
**Milestone:** —
**Source:** §30 of `docs/spec.md`

Written fiction is one format among several. Fan films, podfics, fan comics and zine scans are part of the same fandom, and an instance that catalogues only prose is not an archive of fandom — it is an archive of one corner of it. This milestone makes format a first-class property of a work, **without requiring any instance to store media it does not want to store**.

## 30.1 What counts as a work

Every work carries a format:

```text
Format: prose | poetry | fan_film | podfic | audiobook | fan_comic | zine_scan | interactive | other
```

A work's format determines how its units are rendered and which capabilities apply. It never determines eligibility for search, ratings, collections, series, challenges, comments or federation. A podfic is a work. A fan film is a work.

A non-written work is the same kind of record as a written one: owning pseud, contributors, tags, fandoms, characters, relationships, content notes, rating, visibility, lifecycle, completion, series and collection membership, comments, reactions, bookmarks and credit events. The differences are the unit type and the hosting policy below.

## 30.2 Hosting policy is an instance decision, stated once

Each instance declares what it will store. The setting applies to the instance, never to a request, a work or an uploader:

```text
Media hosting: host | reference | catalogue
```

- **`host`** — media bytes are uploaded, stored content-addressed, and served from this instance, under the same retention, backup and deletion rules as any other object.
- **`reference`** — metadata is stored and the media is played or opened at its origin. No bytes are ever fetched into this instance's storage: not by an import, not by a preservation batch, not by a cache, not by a federated announcement.
- **`catalogue`** — metadata only, with no playback shell embedded at all. The work page presents the work and links out.

The setting is the operator's, and an instance that chooses `reference` or `catalogue` is a complete instance: every other feature in this specification behaves identically. No work, contributor, importer, extension or federated peer may raise the setting on its own behalf. An instance set to `reference` refuses an upload rather than silently storing a file, and the refusal names the instance's policy.

This is why the preference is an instance setting and not a per-work flag. "My instance does not host media, and the fan film it aggregates is still a work" is the combination to be expressible, and a per-work flag cannot express it without turning every work into a decision somebody has to make.

Media bytes are one axis; the text of an imported work is another. This section governs the bytes and §11.15 governs the text, and the two are independent: an instance may host media while aggregating text, or hold nothing but text. They follow one principle, stated in both places because it is the same principle — the operator decides it once, and no work, uploader, importer, extension or federated peer raises it.

**The two settings deliberately do not share a vocabulary, or a number of states.** Media needed a third value: `catalogue` renders no player shell at all, which is a different thing from playing at the origin, because media has a shell to omit. Text has no shell to omit — a work whose body this instance does not hold is read at its origin in the reader's own browser, and there is nothing here to render or to refrain from rendering. A third text value would be a state with no behaviour behind it, and one shared vocabulary would mean either giving text a mode it cannot act on or taking `catalogue` away from an operator who needs it. The names differ because the choices differ.

## 30.3 Media references, and where the metadata comes from

When hosting is `reference`, a media work is an external reference: a canonical URL, the source it belongs to, and the metadata this instance holds about it.

For a site with an adapter (§11.1) the adapter extracts the metadata and the reference is an ordinary imported record. For a fan film on a video platform, the adapter reads the public page and records:

- Title, as the author gave it.
- Channel or uploader, recorded as the work's attribution — never merged into a local account by matching names or handles (§11.11).
- Canonical URL and the platform's own identifier for the item.
- Duration, where the platform states one.
- Publication date, or **no date**. A guessed date is worse than an absent one.
- Description, sanitized and stored as the work's summary, with the boilerplate platforms append to descriptions removed rather than displayed.
- Thumbnail reference, subject to §30.6.
- Tags, fandoms, characters, relationships and content notes (§15.16), drawn from the description or the platform's own fields and **verified or corrected before the work is public** — by the original author if they are on this instance, otherwise by a curator. A fan film's tags are frequently empty, wrong, or a keyword wall, and an uncurated import that displays them is presenting noise as taxonomy.
- A transcript or caption track reference where one exists.

A record with only a URL is not a work. A media work becomes public only when it carries attribution, a description or a transcript, at least one fandom, and a format.

## 30.4 Units

A work's chapters generalize to units. A unit is text or media, and one work may contain both — a podfic with a text version, a fan film series with written episode notes, a comic with the author's commentary.

Each unit records its position, its title, its runtime where it has one, its own external reference or storage key, and an optional transcript.

An imported media series — a fan film in episodes, a podfic in parts — imports as one work with one unit per episode or part, discovered from the source's own listing the way chapter lists are walked (§11.7). The count is reconciled against the source's stated total, and a disagreement refuses the import naming both numbers.

## 30.5 Playback and embedding

Playback is served by this instance's own player shell. Third-party embed markup is never accepted from an author, an importer, an extension or a federated payload, and the work document schema (§8.3) stays free of frames, scripts and embedded objects: a media unit is a field on a record, never markup inside a chapter.

The shell renders a provider drawn from an allow-list that is configuration rather than code. The initial providers are those the initial adapters support; adding one is an operator decision recorded in the ADR set (§1.4).

Rules that hold for every provider:

- No autoplay, and no audio before a reader-initiated action.
- No third-party cookies or trackers before consent; a privacy-preserving embed host is used where the provider offers one.
- No tracking parameter is forwarded, and no referrer beyond the origin is sent.
- The player is replaced by a plain link when the provider is unreachable, when the reader has disabled embeds, or when the instance's hosting policy is `catalogue`. A work never becomes unreadable because an embed failed.
- A reader-level setting disables embedded players everywhere, and it is honored with no per-work exception.

## 30.6 Thumbnails, covers and caching

Thumbnail and cover references are stored, not mirrored, unless hosting is `host`. Displaying a platform thumbnail uses the provider's own image URL with a size hint, and sends nothing about the reader beyond what a plain image request carries.

Metadata extracted from a platform is cached with a short retention and refreshed through the job queue on a schedule derived from the instance's crawl policy. The media itself is never cached, never proxied, and never used to serve a byte this instance has no permission to serve. A cache miss re-fetches the page, not the media.

Search indexes hold metadata only. A private or restricted work's media reference never lands in a shared cache (§10.4).

## 30.7 Discovery, search and eligibility

Format is a filter and a facet everywhere works are filtered: search, saved views, the library, collections, series, challenges, recommendations and statistics.

Discovery is not prose-only by default. A reader who has expressed no format preference meets media works in proportion to their presence in the eligible corpus — neither buried beneath it nor promoted above it.

A media work takes part in mood search, tag search, the people directory, fandom landing pages and the recommendation engines on the same terms as a written one. The engine inputs that assume text are not applied to a work that has no words: runtime substitutes for length, and the length histogram (§15.15) uses runtime for these works rather than omitting them. Completion for a media work means the reader reached the end of the last unit, recorded by the same explicit action a written work uses (§9.8), and **never inferred from a third-party player's playback position**.

## 30.8 Accessibility

A media work states whether a transcript, a caption track, or neither is available, and the work page says which. "No transcript available" is displayed rather than omitted, because a reader who needs one is deciding whether to start.

Where a transcript exists it is readable as text on the work page and is included in full-text search for that work, under the same privacy rules as any other body text.

Auto-transcription is an AI task: it costs credits, quotes before it runs, respects the budget guardrails (§22.11), is labeled as machine-produced (§22.6), and is never presented as the author's own words. An author may replace a machine transcript with their own; the machine version is then retained as a revision rather than overwritten.

A work with neither a transcript nor a caption track is listable and searchable, and is marked in the accessibility facets so a reader filtering for transcripts can exclude it deliberately rather than by accident.

## 30.9 Aggregating media from other instances

A federated media work is announced by reference. The receiving instance applies its own hosting policy: an instance set to `host` may fetch and store media it is permitted to fetch, one set to `reference` stores the announcement and the metadata and plays at the origin, and one set to `catalogue` lists it and links out.

No instance's policy changes another's. A peer that hosts media does not cause a reference-only instance to store bytes, and a reference-only instance does not cause a peer to stop announcing.

## 30.10 Legal and takedown

Where this instance does not host the media, a takedown concerning the media itself is answered by stating that this instance holds metadata and a reference, identifying the host, and complying with any requirement that the listing itself be removed. §19.11 applies unchanged, including the counter-notice path and the modlog.

Where this instance hosts the media, the ordinary copyright workflow applies to it exactly as to uploaded text.

A media reference whose origin becomes unreachable is marked as such rather than deleted, and the work remains in the archive as a record that it existed — the same treatment §11.13 gives a vanished source.

## 30.11 API

```text
GET    /api/v1/works/:id/units
POST   /api/v1/works/:id/units
PATCH  /api/v1/units/:id
DELETE /api/v1/units/:id

POST   /api/v1/media/references
GET    /api/v1/media/references/:id
POST   /api/v1/media/transcripts/:unitId
GET    /api/v1/works/:id/transcript

GET    /api/v1/admin/media/policy
PATCH  /api/v1/admin/media/policy
```

## 30.12 Data model

| Table | Important fields |
|---|---|
| `work_units` | work_id, position, unit_kind, title, runtime_seconds, media_reference_id, chapter_id |
| `media_references` | canonical_url, source_key, provider, provider_id, duration_seconds, published_at, transcript_blob_id, availability, last_checked_at |
| `unit_transcripts` | unit_id, origin (author / machine / provided), language, blob_id, revision |
| `instance_media_policy` | hosting_mode, provider_allowlist, updated_by, updated_at |

`works` gains `format`. `chapters` remains the text unit and is referenced by `work_units` rather than replaced, so existing content and every path that reads a chapter keep working.

Where the instance hosts media, a unit's bytes live in `content_blobs` (§4.4) under a media retention class, exactly as an imported chapter's text does. `media_assets` (§4.3) is unaffected and remains what it is: assets an author uploads *about* a work, such as a cover or a mood-board image. That is a different question from media that *is* the work, and the two are deliberately not merged.

## 30.13 Acceptance

- A fan film on a supported video platform imports with title, attribution, description, duration and a canonical link, and is public only once it carries a fandom, a description and a format.
- An instance set to `reference` refuses an upload and names its policy, and no import, batch, cache or federated announcement stores media bytes on it.
- An instance set to `catalogue` renders no player and links out, and the work page remains complete.
- A media work appears in search, mood search, fandom landing pages and collections, and filters by format.
- A media work is excluded from word-count and reading-time filters, and the length histogram uses its runtime.
- Completion for a media work is recorded by an explicit reader action and never inferred from a third-party player.
- A media series imports as one work with one unit per episode, and a count disagreement refuses the import naming both numbers.
- No embed markup authored by a user, an extension or a peer is ever rendered; only the configured provider allow-list is.
- No stored page or payload contains a player frame, and the editor document schema is unchanged.
- A work with no transcript says so, and a work with one is full-text searchable through it.
- A machine transcript is labeled as machine-produced and can be replaced by the author without destroying the machine revision.
- Disabling embeds instance-wide leaves every media work readable as a link.
- Unreachable media is marked, not deleted, and the work remains in the archive.
- A takedown of non-hosted media identifies the host, removes the listing if required, and follows the §19.11 workflow including the counter-notice.

---

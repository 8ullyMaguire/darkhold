# 46. User Configuration — the customizable website belongs to its users

**Status:** reserved
**Milestone:** —
**Source:** §46 of `docs/spec.md`

> **2026-09-23 addition.** Priority 1 (§0.2) has meant operator
> customization: themes, engines, widgets, challenges through the
> marketplace (§21) and tuning through §38. This section extends the same
> promise to the reader and writer: the user's experience of the instance
> is configurable at every level, with the same first-class treatment.
> The operator shapes the instance; each user shapes their own experience
> of it; neither is an afterthought of the other.

## 46.1 The principle

1. **Every user-visible behavior has a setting with a documented
   default.** Where a behavior is deliberately not configurable, the spec
   says so explicitly (as §45.10 and §38.6 do) — non-configurability is a
   decision, never an omission.
2. **Configuration is layered, and the layering is visible.** A user
   always knows whether a value is theirs, their account's, or the
   instance's.
3. **Defaults are safe and private** — §38.1's principles applied
   userward: a fresh account needs no configuration to read comfortably,
   and every default favors privacy (§7) over exposure.

## 46.2 Resolution hierarchy

Every configurable behavior resolves through one precedence chain:

```text
active context override → per-pseud setting → account default → instance default
```

- **Context overrides** are session- or surface-scoped — the reader
  typography panel for this reading session, a search URL parameter for
  this query. Never persisted unless the user explicitly saves.
- **Per-pseud settings** cover everything §7.2 already separates per
  pseud (profile, works, follows, messages, recommendations, bookmarks,
  reading history, dashboard, extensions, notifications, presence,
  language) plus reader, search, appearance, and community preferences.
- **Account defaults** are the fallback for any pseud that has not
  overridden, and the only level for the inherently account-scoped:
  security (§7), accessibility (§25.3), privacy floors, age policy
  (§7.3).
- **Instance defaults** are the documented per-key defaults in domain
  code — §38.1's principle 3, applied userward. Operators tune behavior
  through §38 keys; they do not set per-user defaults (§38.6 stands).

## 46.3 Storage and API

Settings live in **per-domain tables, never one JSONB blob** — migration
0002's founding decision ("different defaults and different lifecycles,
so different tables") governs every new namespace. `privacy_settings` and
`content_settings` (§7) are the pattern: key–value rows scoped by
`account_id` or `pseud_id` with partial unique indexes.

The API follows the existing settings route family (§26):

```text
GET    /api/v1/settings/:namespace        → resolved merge, with per-key _source
PATCH  /api/v1/settings/:namespace        → write at active level (pseud if
                                            one is active, else account)
DELETE /api/v1/settings/:namespace/:key   → drop override, inherit next level
GET    /api/v1/settings/export            → resolved settings document (§46.6)
POST   /api/v1/settings/import            → validated import (§46.6)
```

- Writes validate against the namespace's registered key set; unknown
  keys are rejected with a named error, never silently dropped.
- `_source` per key is `context` | `pseud` | `account` | `instance`, so
  the UI marks customized values and offers reset at every level.
- Every settings write records an audit-log row (the existing governance
  audit table): who, when, namespace, key, old value, new value.
  Retention follows §24 policy; the audit is for account recovery and
  abuse investigation, not surveillance.

New namespaces and their tables (next free migration number):

```text
search_settings      (pseud)   default filters pre-populating every search surface
content_filters      (pseud)   blocked tags/fandoms/warnings, server-enforced
notification_routes  (account) per-event channel routing, extending §17.5
```

## 46.4 Namespaces

| Namespace | Level | Home |
|---|---|---|
| privacy | account + pseud | §7, `privacy_settings` |
| content | account | §7, `content_settings` (rating ceiling) |
| content_filter | pseud | §46.3, `content_filters` — new |
| reader | pseud + per-work | §9.2 typography and per-work layout; defaults per pseud |
| appearance | account + pseud | §6 presets, accent, reader themes, bundle export |
| accessibility | account | §25.3 |
| search | pseud | §46.3, `search_settings` — new |
| notifications | account + pseud | §17.5, extended by `notification_routes` |
| discovery | account + pseud | §16.5 dial, feed toggles, §16.8 dashboard |
| community | pseud | §17 forum and messaging preferences |
| library | pseud | §14 shelves, history, auto-add rules |
| writing | pseud | §8 editor defaults, feedback-preferences template |
| translation | account + pseud | §22 reading languages, permissions |
| extensions | account + pseud | §21 update and data-sharing preferences |
| import | account + pseud | §11 destinations, formatting |

The full key inventory per namespace is **data in the domain crates**,
validated at the boundary — this spec states contracts, not
enumerations. Search defaults map bidirectionally onto the §15.4 query
language: the structured object is canonical, the query string a
serialization for display and sharing. Saved presets are §14.2 saved
searches — one mechanism, no duplicate.

**Content filters are enforced server-side** on every query, feed,
recommendation, and notification pipeline — a blocked tag never reaches
the client. Filter chips in the search UI show a lock, not a dismiss
button; only the settings page removes a content filter.

## 46.5 The settings surface

- `/settings` groups by domain (reader, search, privacy, …); every
  section deep-links (`/settings/reader#typography`).
- Inherited values display provenance — "(account default)", "(instance
  default)" — with a customize affordance; every level has a reset.
- **Settings search is client-side**, built from the same schema
  definitions that validate storage; labels, descriptions, and keywords
  come from the §6 message catalogs per locale. The §6.4 command palette
  indexes settings alongside pages; selecting a result deep-links with
  the section open. No server-side settings index exists.
- **Inline surfaces sync back**: the reader toolbar writes per-work
  overrides and offers "set as default" (→ `reader` namespace); search
  filter chips are context overrides with "save as default" (→
  `search`); dashboard drag-and-drop writes the dashboard layout
  (§16.8); "hide works like this" appends to `content_filters`.

## 46.6 Portability

Export produces one resolved-settings JSON document. Import validates
against the target instance's schemas, applies the compatible subset,
and lists incompatibilities (e.g. a theme UUID absent on the new
instance) for manual resolution. This generalizes §6's appearance bundle
to the full settings surface.

## 46.7 Invariants

1. Content filters are never bypassable from a surface — only from the
   settings page (§46.4).
2. The age state machine (§7.3) locks the rating ceiling; no settings
   surface can raise it.
3. Settings never reveal taste internals (§0.3): the §16.5 dial exposes
   the choice, never the score, dimensions, or weights behind it.
4. No setting overrides a foundational protection (§0.3).
5. Settings are not content: excluded from search indexes, federation,
   and the public API; only the owner reads their own settings.
6. Every write is audited (§46.3).

## 46.8 Acceptance

- Resolution order holds — context beats pseud beats account beats
  instance — and `_source` reports the winning level for every key.
- Deleting an override falls back to the next level; the UI reflects the
  new provenance.
- An unknown key in a PATCH is rejected with a named error.
- A blocked tag is absent from search results, feeds, recommendations,
  and notification payloads — verified server-side, not by UI hiding.
- A declared minor's rating ceiling cannot be raised from any settings
  surface.
- Export → import on a second instance applies the compatible subset and
  reports the incompatible keys.
- The command palette finds "font" → Reader → Typography and deep-links
  with the section open.

## 46.9 What this section deliberately does not do

- **No single `user_settings` JSONB table.** Per-domain tables stand
  (migration 0002's decision).
- **No server-side settings search index.** Search is client-side from
  schema definitions; a per-locale index table is over-engineering.
- **No personal search-history store.** §14.2 saved searches cover
  intentional persistence; §18.5 demand aggregates stay aggregate.
  Deferred, not forgotten.
- **No operator configuration of per-user defaults.** §38.6 stands:
  instance config is global.
- **No settings federation.** Settings are account data, never exchanged
  between instances; portability (§46.6) is user-initiated file
  exchange.

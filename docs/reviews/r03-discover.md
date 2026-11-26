# R-03 — `/discover` live DOM review

Evidence: Playwright at 1440×1000 and 390×844, `r03_discover_audit.py`, 2026-11-26.
Screenshots: `r03-discover-desktop.png`, `r03-discover-phone.png`. DOM JSON: `r03-discover-audit.json`.

## GOOD
- One `h1` (`Discover`) at both widths, `scrollWidth` equals viewport (`1440/1440`, `390/390`), zero overflow, zero page errors, zero small controls.
- The introduction states the privacy promise plainly, and the feed loads real works without a script gate.
- The ranked list is a dense Archive-style list, not a card gallery — a deliberate ADR 0033 choice that suits the archive voice.
- Two reader-surface rails (`New in your fandoms`, `Most bookmarked this week`) are placed above the feed.

## BAD
- Every row's monospace eyebrow repeated the bold title verbatim (`Fox Hole 159 · by devwriter` above `Fox Hole 159`), doubling row height for no information.
- `DiscoveryItem.word_count` was already on the wire but never shown; a reader had no length or recency signal.
- Titles were bold black with no underline, colour or chevron — the list read as dead text.
- The feed stopped silently after ~20 rows with no end marker.
- `Sort: For you` labelled a feed mode as a sort order.
- At 1440px the list occupied a narrow column with ~500px of cream to its right.

## UGLY
- `· by devwriter` repeated on every row looked like test data shipped as a personalized feed.
- One title leaked raw data into display text: `A Song of Ice and Fire - George R. R. Martin serial (1 chapters, Boombox117)`.
- `Repost:` entries and `Quick Announcement` were rendered identically to stories, with no item-type distinction.

## SUGGESTION (data/operator, not this row)
- The live feed is ~100% one author and contains near-duplicate/repost titles; that contradicts the "community" and "weighted toward what you enjoy" introduction. Fix at ranking/import level with observed supply, not by client-side guessing or silently hiding works.
- Item-type badges and repost folding need a data field before the UI can distinguish them.
- Estate chrome: duplicate Sign in, unlabeled icon buttons, low-contrast Polaris wordmark, orphan `Radio` footer item.

## Fix in this row
Only Darkhold-owned SPA code: the eyebrow now carries author and the server's word count; titles are underlined accent links; an end-of-feed line closes the list; the sort label becomes `Order` for this surface.
Before: eyebrow = `Discovery Test · by alice`; no end line; label `Sort`.
After measurement is appended to `docs/reviews/page-review-queue.md` when the row is closed.

## After measurement (deployed build)
- `/discover` `200`; final Vite exit `0`; release build exit `0` in 7m 24s. An earlier probe right after restart returned `000` because the public homepage was still warming; local `127.0.0.1:8081/` was `200` and a direct re-probe returned `200` in 1.4s.
- Both widths: `h1=Discover`, `scroll=1440/1440` and `390/390`, `overflow=0`, `small=0`, zero Darkhold page errors.
- Eyebrow now carries the server's own explanation and match estimate, e.g. `Readers have rated it highly. · by devwriter · 100% match`, followed by the title once as an underlined accent link. No row repeats its title.
- Label reads `Order`; the feed closes with `That's the whole feed for now.`.
- The first implementation used `word_count`, but the live `/api/v1/discovery` response has no such field — it has `reason`, `reason_code` and `match_percent`. The frontend type and the eyebrow were corrected to use those real fields rather than an empty fallback.
- Targeted proof: `Discover.test.ts` `5 passed`, `SortControl.test.ts` `12 passed`, `svelte-check` `0 errors and 10 warnings`. Mutant restoring the title-duplicating eyebrow failed `expected 'Discovery Test · by alice' to be 'Readers have rated it highly. · by alice · 87% match · 3,000 words'`, then was restored green.

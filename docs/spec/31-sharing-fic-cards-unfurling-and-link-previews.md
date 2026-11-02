# 31. Sharing: Fic Cards, Unfurling, and Link Previews

**Status:** reserved
**Milestone:** —
**Source:** §31 of `docs/spec.md`

Fiction is shared by link, and a link that shows a title, an author and a fandom travels where a bare URL does not. This milestone makes every shareable work render a card, and makes a link posted inside the community resolve to the work it names.

## 31.1 The fic card

Every public eligible work URL renders a card image, server-side and deterministically, for social platforms and for direct download.

The card carries: title; the authors as displayed; fandom; format; rating; completion state; length as word count or runtime; up to the configured number of content notes; the instance name and base URL; and a short link that resolves to the work.

The card is generated from the work record at request time and cached by content hash and template version, so it is stable while the work is unchanged and regenerated exactly when the work changes.

Rules:

- **Public eligible works only.** A draft, a withdrawn work, a restricted work, an unlisted work, and a work the requester may not read render no card, and the response for a non-eligible identifier is identical to the response for a non-existent one, so the endpoint is not an existence oracle (§25.2).
- **The summary appears only if the author allows it**, truncated rather than reformatted. The author controls the cover image, the summary, the content notes, and whether co-authors are named.
- **A card never un-masks an author.** A work published under a pseud the author has hidden does not name it.
- **No reader data on the card.** No reading counts, no reader identities, and no per-viewer variation that could reveal who looked.
- The image is produced by this instance, and no third-party card service receives the work's text.
- A text-only fallback describes the work for platforms that do not render images, under the same eligibility rules.

## 31.2 Sharing from the interface

A work page and the end-of-work page (§9.10) offer: copy link, open the card image, download the card, and a share action that opens the platform's own share URL for the reader's choice. **The instance never posts to a platform on a reader's behalf.** A reader's own note may accompany a shared link, bounded in length and added by the platform's composer rather than by this instance.

## 31.3 Unfurling inside the community

When a post, comment, message or profile field contains a URL, the server may resolve it into a preview card at render time. Previews are rendered server-side, so they appear without JavaScript and to every client, including the API and the federated representation.

**What is resolved, and what is not:**

- **A URL on this instance is rendered from internal data, with no outbound request.** The preview links internally and shows the work, series, collection or thread it names, respecting the reader's access: a work the reader may not read produces a preview that says a work exists and offers the link, never its metadata. This is the common case and the one that matters most — a link to a work here never leaves the instance to render, so there is nothing to scrape and nothing to leak.
- **A URL on a known source** (a site with an adapter, §11.1) may be rendered from a cached metadata record, with attribution and a link to the origin. The fetch that fills that record happens through the job queue under the source's robots and pacing rules (§11.5) and the revision cache (§10.4) — never inline, on request, in the rendering path.
- **Every other URL is a plain link.** No outbound request is made for it, ever.

That last rule is the security-relevant one and it is not a limitation awaiting a later milestone. Fetching an arbitrary URL to build a preview is an outbound request that any signed-in account can aim wherever it likes; it is the same hazard the import path closes with a pinned resolver and an explicit policy (§11.5), and a rendering path cannot apply that policy and remain a rendering path. The set of unfurlable origins is the set this instance already trusts: its own content, and sources it has an adapter for.

Further rules:

- Previews are cached per URL with the same retention as the metadata they show, and a preview of a work that has become private is invalidated rather than served.
- A preview never renders the body text of a work, a held comment, a private note, a private-library entry, a reading position or any personal statistic.
- A preview of a remote Darkhold instance's work uses that instance's own published card and metadata. One instance never scrapes another's pages to build one (§23.6).
- A preview never appears for content whose eligibility the viewer fails, and the rendered markup is identical between "does not exist" and "you may not see it".
- **An author may disable unfurling of their works**, after which the link renders as a plain link everywhere on the instance. The setting is recorded, so a later reader can see why a preview is absent.
- A shared preview is not an endorsement and is never counted as one: it feeds no ranking signal and no credit event.

## 31.4 API

```text
GET  /api/v1/works/:id/card.png
GET  /api/v1/works/:id/card.svg
GET  /works/:id/card
POST /api/v1/unfurl
GET  /api/v1/admin/sharing
PATCH /api/v1/admin/sharing
```

## 31.5 Acceptance

- A public eligible work's URL renders a card image and the Open Graph and Twitter/X metadata that point at it (§23.9).
- A draft, unlisted, restricted or withdrawn work renders no card, and its response is indistinguishable from a non-existent work's.
- A card omits the summary when the author has not allowed it, and omits a hidden pseud.
- No card or preview carries a reading count, a reader identity or any per-viewer variation.
- A link to a work on this instance renders a preview with no outbound request, verified by network assertion.
- A URL pointing at an unknown host makes no outbound request when a post containing it is rendered.
- A URL on a known source renders from cache, and the fetch that fills that cache runs through the queue under the source's pacing rules.
- A viewer without access sees a preview that reveals no metadata, and the markup matches the non-existent case.
- A preview of a work that has become private is invalidated and no longer served from cache.
- An author who disables unfurling sees plain links in the forum, in the preview and in the federated representation.
- A shared preview changes no ranking signal and awards no credits.
- A remote instance's work is previewed from its own published card, with no scrape of its pages.

---

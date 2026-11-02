# 42. Export CTAs — instance-configurable, curator-marked

**Status:** reserved
**Milestone:** —
**Source:** §42 of `docs/spec.md`

Exported ebook files carry a call-to-action (CTA) pointing readers back to
the instance: "read more at …", "leave a comment", "support the author".
CTAs are growth surface, and growth surface is exactly what must not be
forced on authors who already do their own outreach.

## 42.1 Placement

The CTA is appended after the last paragraph of a chapter's XHTML:

- **`per_chapter`** (default): every chapter ends with the CTA.
- **`per_work`**: only the final chapter carries the CTA.
- **`off`**: no CTA anywhere.

```toml
[exports]
cta_placement = "per_chapter"   # per_chapter | per_work | off
cta_html = "Read more at <a href=\"https://example.org\">Example</a>."
```

`cta_html` is an operator-authored XHTML fragment. It is sanitized with the
same allow-list as chapter bodies ([`crate::document`]) before it is
embedded — an operator's configure-file typo must not produce an invalid
EPUB, and a compromised config file must not smuggle script into readers'
devices.

## 42.2 The author-CTA exemption

A work whose author already includes their own CTA (in an author's note,
a bio block, a "support me" paragraph) must not receive the instance CTA:
doubling up is noise, and speaking over the author is the one thing this
platform does not do.

Whether a work carries its own CTA is **marked by curators** — accounts at
trust level ≥ 3 — through a quorum vote:

- `cta_marks(work_id, curator, has_own_cta, marked_at)` records each mark.
- A work is exempt when marks agreeing on `has_own_cta = true` reach
  **quorum** (default 2 agreeing marks, `[exports] cta_quorum = 2`).
- Marks are revocable; retracting a mark recomputes the exemption.
- The exemption is evaluated at export time, never cached in the work row:
  a curator's retraction takes effect on the next export.

## 42.3 Rules

- The exemption applies to the whole work, not per chapter.
- The CTA never renders inside the attribution block (§13.3) — attribution
  is legal notice, not growth surface.
- `validate()` reports `cta_chapters: Vec<u32>` so tests can assert exactly
  which chapters carry the CTA.
- If `cta_html` is unset, the default is a plain link to the instance's
  base URL.

## 42.4 Acceptance

- Default config exports a work with `per_chapter` CTAs on every chapter.
- `per_work` places the CTA on the final chapter only; `off` places none.
- A work with quorum-marked `has_own_cta` exports with no CTA in any
  placement mode.
- A single curator's mark without quorum does not suppress the CTA.
- Retracting marks below quorum restores the CTA.
- Malformed `cta_html` (e.g. `<script>`) is sanitized or refused, never
  embedded raw.
- The CTA does not appear in the attribution section of any export.

---

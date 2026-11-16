# Ideas adjacent to the syndicated trope frontier (2026-11-17)

Companion to `docs/spec-amendments/71-syndicated-trope-frontier.md` and `.hermes/plans/2026-11-17_002000-syndicated-trope-frontier.md`. These are candidate rows, not commitments. Each is phrased so it can be tested; "show a badge" is not an idea here.

## Discovery and frontier

1. Keep a per-author frontier cursor (`last_seen_post_id`, `last_successful_at`) so a pass only walks authors whose cursor is older than the configured interval.
2. Record recommendation edges (`from_author`, `to_author`, `first_seen`, `source`) so the frontier can explain why it found somebody, not just that it did.
3. Score recommendation edges by how many distinct seeds recommend the same author; a hub found from five favourite authors is a stronger signal than one.
4. Put a per-round cap on new authors (e.g. 25) and expand the remainder on the next pass, so one popular author cannot consume the whole crawl budget.
5. Add an operator review queue for frontier candidates: proposed author, depth, evidence path, accept/reject with a reason.
6. Let a reader opt into a personal frontier from their own favourites, with the same depth bound and no shared instance budget.
7. Export the frontier as a plain JSONL sidecar so a human can read it without the app.
8. Mark a source's discovery shape in the catalogue (`tag_page`, `listing`, `author_frontier`, `none`) and fail the catalogue gate when an adapter cannot service its declared shape.
9. Keep a dated robots verdict next to each seed; a stale verdict moves the source to "needs review" rather than crawling anyway.
10. Add a "why was this imported" panel on the work page naming source, seed author and depth.
11. Detect a recommendation cycle and collapse it into one component, so A→B→A does not spend two visits.
12. Add an explicit blocklist for authors and domains; the BFS prunes before fetching, not after importing.

## Freshness and scheduling

13. Show the effective freshness rule per source in the operator console, including the default that applies when a source has no row.
14. Support absolute bounds as well as ages (`2026-01-01..2026-06-01`) with the same parser the CLI already uses.
15. Give each source a jittered window so all eleven archives are not hit in the same minute.
16. Split "discovery ran" from "imports queued" in the run report; both are numbers with different failure modes.
17. Add a freshness check that reads the job's last-run artefact and warns when it is older than three intervals, not just when the worker is inactive.
18. Write a status JSON file atomically on both success and failure, with `discovered/queued/known/skipped` per source.
19. Make `--dry-run` mean "discover and report, enqueue nothing" for the frontier; do not reuse a flag that still writes cursors.
20. Keep the interval bucket key and the fire condition in one function so config edits cannot desynchronise them.
21. Add a backfill mode whose `older_than` is deliberately wide but whose author set is one seed at a time.

## Identity and merging

22. Add a unit-level dedupe check by `(source, post_id)` before a new chapter is appended, so a re-import is a repair, not a duplicate.
23. Keep a stable alias table for story-title variants ("Endergirl 10", "Endergirl - Chapter 10") with a human-reviewed mapping.
24. Store the raw post title alongside the normalised one; a normaliser change must be able to re-derive without a re-fetch.
25. Detect two authors posting the same story title and flag a possible shared work rather than merging automatically.
26. Add a canonical-URL field so a work that later moves from Patreon mirror to AO3 keeps one identity.
27. Represent a story's chapter list as an ordered unit set with ordinals from the post sequence, and detect gaps.
28. Show a maintenance report for works with chapters but missing metadata (no date, no tags, no summary).
29. Keep links-only posts as metadata-only evidence ("a post exists at this URL") without inventing a body or a chapter count.

## Trust and low-trust honesty

30. Gate body text at `trust_at_least:2` but keep tags and fandom metadata public; prove the withheld set by comparing two works in one payload.
31. Add a server-side test that a low-trust reader's HTML contains no "restricted/unlock/TL2" hint, not merely no body.
32. Give staff an entitlement exemption so a review queue can open what it must review.
33. Derive a work's "restricted" state from schema, not a UI flag; a client must not decide what is withheld.
34. Add an audit row when a TL2 body is opened, keyed by work and account, never storing the body in the audit.
35. Test the visibility contract on every surface that lists works: search, tag, author, collection, series, API and sitemap.

## Ranking and taste

36. Use frontier depth as a weak prior, not a quality signal: depth 1 from a favourite is a stronger candidate than depth 3.
37. Separate "system" and "dark MC" into distinct freeform tags rather than one combined tag; a work may have either or both.
38. Add a drought detector for this niche: no new TL2-readable work in N days, with the frontier report as evidence.
39. Compute a per-source yield (works queued per page fetched) and lower the cap on low-yield sources.
40. Feed accepted frontier works into the existing recommendation slot mechanism; do not create a second ranking pipeline.
41. Keep a "not interested" signal per source/author so the frontier can skip without hiding unrelated works.
42. Add a monthly digest of newly reachable works with the reason each one matched, capped and cursor-advanced even when empty.

## Operator and data hygiene

43. Add a CLI verb `dh supply frontier --source pawchive --seed <url> --depth 3 --dry-run` that prints the planned author set.
44. Add a CLI verb `dh supply freshness --source pawchive` showing the parsed window and the count of candidates it would accept/reject.
45. Keep a `sources.json` entry for every source discovered here, blocked or not, with a dated note.
46. Add a gate that fails when a source has an adapter but no catalogue row, or a catalogue row naming a missing adapter.
47. Keep the importer as the only body-writing path; the frontier discovers URLs and never writes prose itself.
48. Add a replay fixture from the live Pawchive recommended page so parser changes are tested against real markup shape.
49. Record every skipped candidate with a reason (`no date`, `too old`, `too new`, `blocked`, `already known`) in the run report.
50. Add a small "source health by host" tile that shows the last successful fetch date, never a green heartbeat with no timestamp.

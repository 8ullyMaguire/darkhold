# docs/plans/milestone-06-imports.md — plan card

Source: `docs/plans/milestone-06-imports.md`. Translated 2026-11-26 into the uniform card shape; no new work is invented. The source document remains authoritative.

## Exact file paths

`docs/plans/junior-implementation-plan.md` says what M6 was *meant* to be. This
`docs/plans/milestone-05-jobs.md`, and written at the same time as the work
(§5 of `docs/sessions/2026-09-11.md`). What remains is the **adapter set** — five
| The adapter crate, its trait and the registry | `crates/scrapers/src/{lib,registry,sites}.rs` | 185 in-crate |
| The safe fetcher and URL guard | `crates/scrapers/src/safety.rs` | 41 |
| A browser TLS/HTTP2 fingerprint | `crates/scrapers/src/engine.rs` | 8 |
| A FlareSolverr-v1 solver service | `crates/scrapers/src/solver.rs` | 13 |
| An Internet Archive snapshot | `crates/scrapers/src/archive.rs` | 12 |
| Charset decoding for fetched pages | `crates/scrapers/src/safety.rs` (`decode_body`) | 9 in-crate |
| `robots.txt`: the source's own pace and path rules | `crates/scrapers/src/robots.rs` | 21 |
| The fixture tests, by family | `crates/scrapers/tests/{ao3,royalroad,syosetu,efiction}_fixtures.rs` | 9 + 21 + 31 + 37 |
| Chapter sanitation | `crates/scrapers/src/sanitize.rs` | 17 |

## Copy-pasteable code / commands

plain client: refused, as expected
browser fingerprint: 46808 bytes of the real chapter page, from https://www.fanfiction.net/s/12345678/1/

plain client: refused, as expected
browser fingerprint: also refused, which is why the solver tier exists
solver: 241315 bytes of the real story page via http://127.0.0.1:8191
guard: an undeclared host is refused before any solve is attempted

INFO:     Challenge detected, waiting for it to clear...
INFO:     Clicked the challenge checkbox (attempt 1).
INFO:     Clicked the challenge checkbox (attempt 2).
INFO:     Done https://www.fimfiction.net/story/594215/... in 12.26s

## Verification command and expected output

| Migration 0006, both dialects | `migrations/{sqlite,postgres}/0006_imports.sql` | applied by every acceptance test |
| Migration 0007, both dialects | `migrations/{sqlite,postgres}/0007_revision_cache.sql` | applied by every acceptance test |
  count check fires first and says more — which is recorded in the test file rather
this milestone's fixtures exist to catch, and it is why the fixture test asserts
test can pass: real dates or none, the source's own chapter id rather than a loop
the bulk pass is that list, not "port 51 files".
like it was posted today. A table of expected formats is a table of the formats
member that answers with the print template anyway. The fixtures assert the prose
adapter requires the block and the label together, and the fixture test asserts the
| A content gate (age acknowledgement) | `AuthRequired` | The job pauses and asks the reader; a credential with `adult_allowed` satisfies it by following the page's own link, once |
| A challenge wall | `Blocked` | Detected by the interstitial's own markers, not by the absence of content — identification by absence turns the next new gate into "no such work" |
failing anywhere**. Titles like `This week\u{fffd}s shows` pass every assertion a
test would think to make.
from the letter of the standard, in the safe direction, and it has its own test.
| **www.fimfiction.net** | **`403`**, served to a browser fingerprint | a solver is needed; the fingerprint alone does not pass it |
(`is_bot_challenge`'s fixtures and the live test below).
which is itself the finding: these archives gate a warned story behind a
content-warning interstitial, and the recording shows the gate rather than the
adds no pacing of its own: every step runs *through* the same `robots.txt` gate, so
the test next to it exists because of a real mistake: `challenge-platform` is the

## RED/GREEN evidence

/imports/:id/retry-failed-chapters` queues another attempt at an import that has
finished, and the attempt re-reads only the ordinals recorded as failed — and
| `let Ok(toc) = fetch(..) else continue` | a failed list page is an error, not a skipped 100 episodes |
| `fetch_chapter_text` → `String::new()` | a failed body is an error, never a blank chapter stored as success |
is short in the middle. Row five is the same failure for a single chapter: a failed
page and the contract's endpoint is `/v1`. Every stub test passed while this was
been treating a failed creation as "carry on statelessly", so reads worked and

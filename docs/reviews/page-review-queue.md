# Page review queue — Darkhold

Authority: owner instruction "finish every roadmap row before any review row; for review rows load each page at
desktop and phone width in a real browser, read the live DOM, be critical, write GOOD/BAD/UGLY/SUGGESTION, then work
on anything relevant, re-measure before/after, and move the row to Done in the same commit as the proving line."

Roadmap queue is empty of unblocked rows (`FS-02` and `TA3-B3` are blocked and logged in
`docs/plans/roadmap-remaining.md`), so review work starts now.

| row | page | widths | status | evidence |
|---|---|---|---|---|
| R-01 | `/` public landing | 1440×1000, 390×844 | Done | before small=7/3, `141939`, orphan `general`, two Sign in; after small=2/0 (remaining desktop links are 652×28, above WCAG 24px), grouped `141,940`, one Sign in, `meta-main`/`meta-state`, no orphan, `Start reading`; home 200; lib 1 passed; landing 5 passed; mutant exit 101; fd4e68ff |
| R-02 | `/library` signed out and signed in | 1440×1000, 390×844 | Done | before small=1/1 signed-out, 6/6 signed-in, filters+storage at 0 works, identity clipped; after small=0 in all four cases, no filter/storage/duplicate import, one primary Import a work, identity scrollWidth=clientWidth=176; Library 10/10, App 14/14, Ring 5/5, svelte 0 errors; mutant exit 101; home 200; 8651de6f |
| R-03 | `/discover` | 1440×1000, 390×844 | Done | before eyebrow repeated title, no rationale, label Sort; after eyebrow = server reason + author + match percent, title once as underlined link, Order label, end-of-feed line, small=0/0, overflow=0, errors=0; Discover 5/5, SortControl 12/12, svelte 0 errors; mutant failed title-duplication assertion; home 200; b56e69bd |
| R-04 | `/search` SPA (in-app navigation) | 1440×1000, 390×844 | Done | before phone `?q=winter` scroll 515/390, 9 overflowing histogram bars, 30 small controls, raw counts, no Results heading; after scroll 390/390, overflow 0, small 0, grouped counts, Results heading, 43/43 targeted tests, svelte 0 errors, fmt_count mutant failed, home 200; 3e647694 |
| R-05 | a published `/works/{id}` and its reader | 1440×1000, 390×844 | Done | before work small=4/4 with literal `<p>` in summary and `5367 words · en`, reader repeated title 2x, reader small=11/10; after work small=0/0, `5,367 words · English`, one title, no raw markup; reader title 1x, overflow=0, errors=0; r05 2/2, ui_tests 4/4, reader_surface 9/9, mutant failed, home 200; b80ceb7b |
| R-06 | `/sign-in` | 1440×1000, 390×844 | Done | before small desktop/phone 3/2 with 25px secondary links, left-hugged form, underlined Polaris button, 3 page Sign in controls; after small 1/0 (desktop select only), form centred, Polaris no underline, Forgot beside Password, header/nav Sign in gone, error and default both overflow=0/errors=0; SignIn 4/4, App 14/14, svelte 0 errors; mutant failed, home 200; 52061b82 |
| R-07 | `/roadmap` | 1440×1000, 390×844 | Done | before board small desktop/phone 3/2 with 22px inline CTAs, filled-pill active tab and left-hugged void; after small 1/0 (estate select only) on all tabs, underline tabs 44px, centred bordered empty panel, primary Sign in to suggest / Sign in to vote, overflow=0/errors=0, Roadmap 10/10, svelte 0 errors, mutant failed, home 200; e1c5b05f |
| R-08 | `/community` | 1440×1000, 390×844 | Done | before signed-out small 2/1 with 22px inline gate link, signed-in Forums 2/2 with 21px filter and nested 22px link; after signed-out 1/0 (desktop theme select only), signed-in all tabs 0/0, overflow=0, Darkhold errors=0; SignInGate 4/4, Community 4/4, svelte 0 errors, mutant failed, home 200; 74e60363 |
| R-09 | `/docs` | 1440×1000, 390×844 | Done | before index small 5/4, detail 6/5, duplicate intro, 22px TOC bullets, mid-word summaries, one desktop column; after index 5/4 (estate select + static kbd chips), detail 2/1 (estate select + 44px-tall breadcrumb), overflow 0, no Darkhold errors; Docs 4/4, docs registry 7/7, svelte 0 errors, TOC-href mutant failed, home 200; ebb7bfc6 |

Ordering rule: always take the first row whose status is Open. A row becomes Done only with a commit whose message
names the row and whose body carries the before/after proving line. Blocked review rows get an exact unblock and
owner here, then the next Open row is taken.

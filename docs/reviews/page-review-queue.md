# Page review queue — Darkhold

Authority: owner instruction "finish every roadmap row before any review row; for review rows load each page at
desktop and phone width in a real browser, read the live DOM, be critical, write GOOD/BAD/UGLY/SUGGESTION, then work
on anything relevant, re-measure before/after, and move the row to Done in the same commit as the proving line."

Roadmap queue is empty of unblocked rows (`FS-02` and `TA3-B3` are blocked and logged in
`docs/plans/roadmap-remaining.md`), so review work starts now.

| row | page | widths | status | evidence |
|---|---|---|---|---|
| R-01 | `/` public landing | 1440×1000, 390×844 | Done | before small=7/3, `141939`, orphan `general`, two Sign in; after small=2/0 (remaining desktop links are 652×28, above WCAG 24px), grouped `141,940`, one Sign in, `meta-main`/`meta-state`, no orphan, `Start reading`; home 200; lib 1 passed; landing 5 passed; mutant exit 101; THIS_COMMIT |
| R-02 | `/library` signed out and signed in | 1440×1000, 390×844 | Pending | pending |
| R-03 | `/discover` | 1440×1000, 390×844 | Pending | pending |
| R-04 | `/search` SPA (in-app navigation) | 1440×1000, 390×844 | Pending | pending |
| R-05 | a published `/works/{id}` and its reader | 1440×1000, 390×844 | Pending | pending |
| R-06 | `/sign-in` | 1440×1000, 390×844 | Pending | pending |
| R-07 | `/roadmap` | 1440×1000, 390×844 | Pending | pending |
| R-08 | `/community` | 1440×1000, 390×844 | Pending | pending |
| R-09 | `/docs` | 1440×1000, 390×844 | Pending | pending |

Ordering rule: always take the first row whose status is Open. A row becomes Done only with a commit whose message
names the row and whose body carries the before/after proving line. Blocked review rows get an exact unblock and
owner here, then the next Open row is taken.

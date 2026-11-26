# 0035 — Port skeleton patterns, not the skeleton stack

**Date:** 2026-11-26
**Status:** Accepted, not built
**Milestone:** none assigned
**Evidence:** `docs/spec-amendments/ui-refactor-from-skeleton-2026-11-26.md`;
`/home/alvaro/darkhold-build-comparison-2026-11-26.md`;
screenshots and probe JSON in `/home/alvaro/darkhold-comparison-evidence/`.

## Problem

A greenfield React skeleton scored 8.15/10 against the live Darkhold build's 5.20/10 on the
fixed pairwise-comparison rubric. The largest gaps were first-run wayfinding, availability/evidence
on every work row, and decision scaffolding (reasons, choose-if/avoid-if, NO VERDICT). The live
build's strengths are real content, provenance and a working backend. The tempting conclusion —
replace the frontend — would throw away the working parts to gain patterns that can be ported.

## Decision

Darkhold keeps Rust/Axum, Svelte, the two-renderer topology (ADR 0006), the Archive-chrome /
Reading-Room-reader registers (ADR 0033), and the reader's no-inline-script guarantee (ADR 0034).
The following are ported as **patterns** into the existing renderers:

1. one Darkhold-destination nav with one active state on every surface, separate from the injected
   Polaris network switcher;
2. availability and evidence stated in words on every work row;
3. work-dossier anatomy: provenance facts, choose-if/avoid-if, biggest unknown, export action;
4. URL-held search constraints, closed sort vocabulary, and named zero-result recovery;
5. the copy deck for absent, degraded, conflict and module-unavailable states.

Explicit ranking feedback stays parked as `XW-1`, gated on the experiment loop. Live hard failures
(reader control strip at `opacity: 0.14`, conflicting work metadata) are repaired before porting.

## Consequences

- The code must keep the Archive/Reading-Room separation: `--reader-*` tokens stay independent of
  site tokens, and a warm-paper reader inside dark chrome is not a theme defect.
- Server-rendered documents stay documents; nav and state copy are emitted server-side, not behind
  an inline script or a client-only mount.
- The injected `<polaris-chrome>` shadow root remains owned by the estate; site CSS reaches it only
  through inherited custom properties.
- Portfolio cost: this is a multi-surface refactor, not a single CSS pass. Full parity with the
  skeleton's interactions is deliberately not claimed.
- The prototype's invented routes, fandom data, module toggles, mock publishing and localStorage
  persistence are explicitly not adopted.

## Alternatives considered

**Replace the Svelte frontend with the React skeleton.** Rejected: it would retire a working backend
and two-renderer topology to gain information patterns; ADR 0006 and ADR 0033 remain decisive.

**Restyle only (tokens and colours).** Rejected: the measured gap is in decision support and
wayfinding, not palette. A restyle would leave the 25%-weighted gap untouched.

**Adopt the prototype route map verbatim.** Rejected: `Arrive/Find/Keep/Tune/…` were invented for a
visual embargo brief; Darkhold already owns its route vocabulary and error codes.

**Ship reader-side ranking feedback with this refactor.** Rejected: the live experiment loop has
0 snapshots and 0 decisions; the UI would be inert. It remains `XW-1`.

## Verification

```bash
grep -n 'patterns, not the stack' docs/spec-amendments/ui-refactor-from-skeleton-2026-11-26.md
grep -n 'UIR-0' docs/plans/ui-refactor-from-skeleton-2026-11-26.md
grep -n 'XW-1' docs/plans/suki-crosswalk-intake-2026-11-26.md
```

Each prints at least one matching line. This record is `accepted, not built`: no code, build or
deploy accompanied it.

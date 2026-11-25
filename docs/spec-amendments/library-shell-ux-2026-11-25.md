# Spec amendment — Library and shell UX, 2026-11-25

## Problem

A live visual audit of `https://darkhold.polarisocial.xyz/library` at 1440×1000
and 390×844 found a signed-out Library page whose main area is one heading, a
small inline sign-in sentence, and ~500px of empty background. Measured: 17
interactive elements under 44px on desktop, 6 on phone; the main sign-in link
is 44×22; `main` padding is 16px; page height 1129px against a 1000px viewport.
The desktop header exposes 9 destinations plus controls with overlapping names,
and the phone header has no account affordance. The PWA install strip is the
first content under the header and cannot be dismissed. The saved-login attempt
re-rendered the form with no error surface visible.

## Goals

1. Make the signed-out Library page explain itself and offer a real next step.
2. Reduce the top-level navigation to five unambiguous destinations while
   keeping every existing destination reachable through More.
3. Put an account affordance in the header at every width: styled Sign in
   signed-out, Account menu signed-in.
4. Meet a 44px minimum hit target for standalone interactive controls.
5. Demote, style and dismiss the install prompt.
6. Make the footer compact, useful and not cut off; keep family-site links.
7. Use one typographic voice for body text and label the appearance control.
8. Keep capability-gated nav, sessions, PWA update and every route intact.

## Non-goals

No backend or API changes; no new routes; no removal of Write, Read, Create,
Forum, Modules, theme selector, install/update actions, reading goal, or
family-site links. No wholesale design-system rewrite. No change to sign-in
credential handling — the form already has an `ErrorSummary`; the failed
attempt was recorded, not silently "fixed" by inventing a message.

## Requirements and checks

- **UX-1** Signed-out Library shows a card: heading, one-sentence explanation
  of imports/shelves, a primary "Sign in" button, a secondary "Browse
  Discover" button, and "Set a reading goal" as a tertiary line after the
  explanation. `ReadingGoalRing` moves into the signed-in branch.
- **UX-2** Top-level desktop and phone nav is Discover, Search, Library,
  Shelf, More. `Write` moves into the Create group; Read/Forum groups are
  renamed Explore more/Community. Every old href remains in the More menu on desktop and the More drawer on phone.
- **UX-3** Both headers have an account door: signed-out Sign in, signed-in
  Account opening the drawer. Appearance is labelled on desktop and in the drawer.
- **UX-4** Standalone nav/header/PWA controls are >= 44px on both widths.
- **UX-5** The install notice has a dismiss button that persists, and both
  notice actions use the design-system button rather than a native default.
- **UX-6** Footer has one compact brand/tagline block, local utility links
  (Docs, Roadmap, Community) and FamilyNav; no decorative wordmark divider.
- **UX-7** Body copy in `main` is 16px; the system theme option is labelled
  "System default" while the control itself is visibly labelled "Appearance".
- **UX-8** The live page, not just source, is re-measured: no horizontal
  overflow, zero standalone controls under 44px at 390px, and a signed-out
  Library card with two visible actions.

## Decisions

- Five destinations beat ten: the first pass only renamed groups and kept all
  six group menus inline, and a live screenshot caught a 812px row in a 522px
  container. The desktop row now renders four links plus one More menu; module
  gating still filters both places from the same data.
- Shelf points at `/library/history` because the thing on a shelf is its
  contents; `/library` itself remains the library.
- The install prompt stays near the top but becomes dismissible and styled;
  a context-triggered prompt would need a work-open event that does not exist
  in this shell yet.
- The failed sign-in is reported as observed. The existing `ErrorSummary` is
  the right surface; no duplicate error UI is added.

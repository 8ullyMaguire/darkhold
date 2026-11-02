# 40. Remix — fork with provenance, and permission statements made enforceable

**Status:** reserved
**Milestone:** —
**Source:** §40 of `docs/spec.md`

> **2026-09-21 addition.** Spec §33.1 defined permission statements and
> derivative lineage as a draft. This section schedules their implementation
> and adds the one thing §33.1 described only as data: the *affordance*. A
> derivative edge nobody can create is a schema, not a feature.

## 40.1 The fork affordance

A **Fork this work** action on every eligible work page. One click creates a
new draft owned by the forker, linked to its parent through the §33.1
derivative lineage edge (`kind = remix` by default), inheriting the parent's
tags, fandoms, characters and relationships, and copying no body text — a
fork starts empty, because the forker's words are the point.

Guards, in order:

1. **Permission statement.** The parent's `remix` statement is checked: `no`
   refuses with the statement named in the refusal; `ask` routes to a
   request the author answers (never auto-approved); `unstated` and `yes`
   proceed. The refusal names the statement, per §33.1.
2. **Exclusion registry.** A parent on the exclusion registry is refused by
   name.
3. **Depth limit.** `[works].max_fork_depth` (default 3): a work whose
   lineage chain is already that deep refuses with the chain shown. Fork
   chains that go forever are how low-effort copies drown a platform.
4. **Visibility.** A fork of a private work is private; a fork never widens
   the parent's audience.

The parent's page lists its children under a **Remixes** heading, each with
its lineage kind and a link. The child's page names its parent the same way.
The edge survives orphaning (§32.3) and deleting a parent never deletes a
child.

## 40.2 Permission statements, implemented

The §33.1 statement model becomes real on every work: `podfic`,
`translation`, `remix`, `continuation`, `redistribution`, `ai_training` —
each `yes | ask | no | unstated`, defaulting to `unstated`, editable only by
the owner, surviving orphaning and account deletion. The work editor grows a
**Sharing & permissions** panel with one control per statement and a plain
sentence under each ("Readers may translate this work without asking" /
"Readers must ask first" / "Readers may not translate this work").

Enforcement doors, each naming the statement in its refusal:

- Fork (§40.1) checks `remix`.
- The translation pipeline (M17) checks `translation` before a request is
  created.
- Narration/TTS (M26) checks `podfic`.
- The import adapters (M6) carry a source-published statement through with
  provenance and never upgrade `unstated` to `yes`.

## 40.3 Acceptance

- The fork button on a work whose statement is `remix: no` refuses and names
  the statement; `ask` creates a request; `yes` and `unstated` create a
  draft.
- A fork inherits tags and lineage, copies no body text, and appears in its
  parent's Remixes list.
- A chain at `max_fork_depth` refuses with the chain shown.
- Every statement is editable by the owner only, and the editor shows the
  effective sentence for each.
- A statement change is recorded with its before and after values.

## 40.4 What this section deliberately does not do

- **No automatic enforcement of external platforms' rules.** The statement is
  the author's declaration; chasing violations off-instance is the author's
  choice, not the instance's job.
- **No licence generation.** The statement is not a legal licence (§32 owns
  rights); it is a machine-readable social contract.
- **No retroactivity.** Statements gate new derivatives; existing ones stand.

---

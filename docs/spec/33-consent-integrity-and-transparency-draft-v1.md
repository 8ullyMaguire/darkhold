# 33. Consent, Integrity, and Transparency (draft v1, spec-only)

**Status:** reserved
**Milestone:** —
**Source:** §33 of `docs/spec.md`

**Nothing in this section is implemented.** Milestones 27–29 were promoted on 2026-09-19 from a cross-project review (the vault notes `[[gravity-darkhold-feature-crosswalk]]` and `[[darkhold-jev-system-one-ideas]]`) after checking each item against §0–§32 rather than assuming it absent: the community roadmap and best-worst ballots already exist (§28.12, §29), canonicalization infrastructure already exists (§15.11), and a pluggable classifier with confidence scores is already anticipated (§12.2). What follows is what survived that check.

## 33.1 Milestone 27 — Permission statements, derivative lineage, and the exclusion registry

**Implement.** A **permission statement** on every work and every creator, covering podfic, translation, remix/fork, continuation, redistribution and AI training, each `yes | ask | no | unstated` and defaulting to `unstated`; editable only by the owner, and surviving orphaning and account deletion. A **derivative lineage** edge written whenever one work derives from another, carrying a relationship kind (`translation | podfic | remix | continuation | inspired_by`) and provenance; an imported work whose source states a parent carries the edge, and a parent lists its children. An **exclusion registry** naming external creators and works that must not be imported, narrated, translated, remixed or announced on this instance. Enforcement sits at every door that can create a derivative — the M25 derivative pipeline, TTS narration (M26), the M6 import adapters, the remix path, and ActivityPub announcements (§23.6) — enforced by the server and named in the refusal, never by interface alone.

**Acceptance.**
- A work stating `podfic: no` refuses a narration-edition request and names the statement; `ask` routes the request to the author rather than refusing it in the author's name.
- A remix records its parent and the parent lists the child; the edge survives orphaning (§32.3), and deleting a parent never deletes a child.
- An imported work whose source publishes a statement carries it with provenance; the instance never upgrades `unstated` to `yes`, and a source's `no` is honoured before the bytes are stored.
- A creator or work on the exclusion registry is refused at import by name.

**Deliberately out of scope:** the statement is not a licence (rights remain §32's), not a moderation tool, and not retroactive — it gates the creation of new derivatives, never the existence of existing ones.

## 33.2 Milestone 28 — Rating signal integrity

**Implement.** Trust-weighted aggregation for the ratings and reactions that feed quality signals — a fresh account's first ratings weigh less, and weights come from trust alone, never from credits, subscriptions, bounties or marketplace revenue (§0.3). Anomaly detection over the ratings stream: burst detection per work and per reviewer cohort, and per-account rating-profile outliers, feeding the existing §24.5 anomaly alerts and the §19.3 report queue rather than a parallel pipeline. Brigade early warning that marks a work's aggregate **contested** — visible to curators, never published as a public score — and pauses that work's promotion into recommendation surfaces until a quorum clears it. The honesty rules: a reader always sees their own rating unchanged, an aggregate display states when a cohort guard is suppressing a number (§24.3's k-anonymity pattern), and no individual rating is ever publicly attributed to its reader.

**Acceptance.**
- A rating burst from a fresh cohort raises a contested mark and stops promotion without hiding the work from any reader.
- Trust weighting changes the aggregate while every individual rating stays visible to the reader who made it.
- A contested work clears through quorum and resumes promotion; the clearance appears in the public log (§19.12).
- A test asserts that credits, subscriptions, bounties and marketplace revenue can change no rating weight.

**Extends:** §9.5 ratings, §9.7.8 anti-gaming (the credits half exists; this is the ratings half), §19.13, §24.5.

## 33.3 Milestone 29 — Recommendation transparency and curation labour

**Implement.** (a) **"Why am I seeing this"** on every recommendation slot in the M11 discovery surfaces and recipe dashboards: the reader-side reasons that produced the slot — taste signals, filters, the recipe stage, and the comparison that seeded it (§29.2's arena language where a preference produced it). The explanation must never reveal the administrator's taste multiplier or its breakdown (§0.3, §24.3); operator influence appears as one undifferentiated "instance curation" line. (b) A **private attention report**, off by default: what the reader read, what their own filters changed, what their own settings held back, and what instance curation did — aggregate only. (c) **Curation labour**: a tag-wrangling queue that turns canonicalization (§15.11) into visible trust-gated work — proposals (alias, merge, namespace move, canonical rename) enter a queue, trust levels gate who may propose and who may approve (§19.1), merges keep their history and stay reversible, and wrangling earns the same credits and reputation as any other curation, with the §0.3 refusal that no credit buys wrangling authority and no wrangling authority buys ranking.

**Acceptance.**
- Every recommended slot can name its reader-side reasons, and a test asserts no explanation path can surface the admin multiplier.
- The attention report is private to its reader, includes at least one "held back by your own settings" line, and stays disabled until the reader enables it.
- A tag merge requires the configured trust level, records its approver, is reversible, and appears in the public log (§19.12).
- No wrangling proposal or vote is visible in another user's surface.

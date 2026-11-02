# 34. Decision Services (calibrated classifiers)

**Status:** reserved
**Milestone:** —
**Source:** §34 of `docs/spec.md`

Cross-cutting contract rather than a milestone: it changes how §12.2, §22.8, §11.14, §11.10, M21's saved-search alerts and §24.14 are implemented, and it exists so those features stop each inventing their own threshold and their own silent failure mode. §12.2 already anticipates "an optional AI classifier returning a confidence score"; this section states the obligations that confidence carries.

**34.1 The contract.** A decision service answers a declared question with a member of a declared answer set plus a calibrated probability — `decide(task, state) → { label, confidence }` — and never returns prose. Structured output is a schema guarantee rather than a hope: an answer outside the set is not representable, which is what makes a decision safe to place deep in a chain where a hallucination is a page failure rather than a bad paragraph. The answer set is declared per task, so a question with more than a few hundred options is narrowed by retrieval first (the §15 index), then decided.

**34.2 Providers are pluggable and optional.** §12.2's optional classifier generalises to a `DecisionProvider` configured per task: a hosted service, a local model (the Ollama path §12.2 already names), or none. With none configured, every task falls back to its deterministic path — §12.2's rules and heuristics with the wider ambiguous band — and the fallback is disclosed on `/api/v1/meta` beside the active content policy. A hosted-only task is refused by design: self-hosting (§2.4) means no task may require a third party for the instance to keep working.

**34.3 Thresholds, tail owners, and what is never decided.** Each task declares a confidence threshold and names the human path that owns the uncertain tail (§12.5 quorum, the §19.3 report queue, the curator queue), and the instance counts escalations — a classifier routing half a comment stream to moderators has automated nothing. **Never decided here:** sanctions (§19.6), shadowban (§19.7), DMCA notices (§19.11), money (§20.9), trust levels (§19.1), and the AI-training assertion as a verdict (M21 keeps it a signal). These are human by construction, and the decision surface cannot express them.

**34.4 Declared tasks (v1).**
- `positivity_class` — the §12.1 classes with calibrated confidence, so §12.3's delivery rules become threshold rules (high-confidence appreciation delivers; critique delivers only where the author opted in; the ambiguous band routes to §12.5 quorum exactly as §12.2 already states). This is the flagship: the pipeline is already specified; this is the confidence that lets delivery be trusted to it.
- `translation_quality` — p(adequate) gating auto-published machine translation, which turns §22.5's "AI never overrides human" into a threshold with human-version precedence.
- `import_outcome` — per-item import classification (§11.14) in the M5 worker, with the low-confidence tail to the batch review UI.
- `identity_match` — p(same creator) over retrieved candidates for §11.10 cross-source identity; above threshold it *suggests*, and never merges (the no-force-merge rule stands).
- `alert_match` — p(matches saved search) for M21's alerts, evaluated per new work across that reader's saved searches; a per-user threshold decides whether an alert is sent, which is what makes the alert queue affordable.
- `mood_class` — §15.8 mood/tone labels for new units and as a corpus backfill: one worker pass that turns a planned search feature on.
- `language_id` — per-unit language for transcripts and media (§30.8): a bounded choice among languages, which is exactly this shape.
- `crawler_signal` — behavioural p(crawler) at the edge for §24.14, as a second line behind the deterministic rules the posture already states.

**34.5 Batch passes produce signals, never verdicts.** The same contract run by the worker over the corpus (backfills, reindex passes). A backfilled label is metadata: it never retro-hides content, never changes an eligibility verdict on its own, and a re-run may change it.

**34.6 Audit and disclosure.** Every automated decision that changes what a reader sees is recorded — task, label, confidence, model identity and version, timestamp — operator-only, hashes and labels rather than copied text, consistent with §24.3's privacy rules and §0.3's ban on hidden influence. Where an explanation surface exists (M29's "why am I seeing this"), it may cite a decision's task and label; it may never expose the operator's weights.

**Acceptance.**
- With no provider configured, §12's classification runs on the deterministic path, the wider ambiguous band reaches quorum, and `/api/v1/meta` says so.
- No task can be invoked for a sanction, shadowban, DMCA case, payout, trust level or AI-training verdict; a test asserts the surface cannot express them.
- A comment's class and confidence are recorded, and a published delivery decision can name the class that produced it.
- Every task defaults to disabled, and an instance with all tasks disabled behaves exactly as §12.2's rules path describes.

---


---

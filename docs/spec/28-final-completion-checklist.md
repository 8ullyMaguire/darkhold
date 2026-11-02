# 28. Final Completion Checklist

**Status:** reserved
**Milestone:** —
**Source:** §28 of `docs/spec.md`

## 28.1 Customization (Priority 1)

- [ ] Extension slot architecture from Milestone 1.
- [ ] Themes and layout presets installable from marketplace.
- [ ] Recommendation engines and recipes installable.
- [ ] Widgets composable per pseud with multiple dashboard views.
- [ ] Extension permissions require user consent.
- [ ] Paid extensions honor entitlements.
- [ ] Reset and safe mode work with broken extensions.
- [ ] Webhooks allow event-driven integration.
- [ ] Custom CSS is sandboxed against exfiltration.
- [ ] Navigation customization respects safety controls.

## 28.2 Available fiction (Priority 2)

- [ ] Paste URL, drag file, and raw text import.
- [ ] Author bibliography batch import.
- [ ] Author watches with rate-limited scheduling.
- [ ] Cross-posting to external sites.
- [ ] Preservation batches with attribution.
- [ ] Source credentials encrypted and scoped.
- [ ] Runtime source health separate from support status.
- [ ] Completion rate as quality signal.
- [ ] Mood/tone search.
- [ ] Fuzzy matching with confirmation.
- [ ] Length histogram filter.
- [ ] Editorial curator picks.
- [ ] Fandom landing pages.
- [ ] Bookmark CSV import/export.
- [ ] Non-written works — video, audio, comic, zine — are importable, searchable and readable under the instance's media policy.
- [ ] Fan-film aggregation records attribution, description, tags, characters and duration, and a reference-only instance stores no media bytes.
- [ ] Every public work URL renders a share card, and a link posted in the community previews internally without an outbound request.
- [ ] New voices and first works are surfaced by a bounded, labeled discovery slot.
- [ ] Activity status distinguishes active, slow, dormant and concluded without penalizing any of them.
- [ ] Content notes are a separate axis from tags, reader-configurable and quorum-suggestible.
- [ ] Sorts are an explicit menu, exact sorts are exact, and random paging is stable within a request window.
- [ ] Saved-search alerts run with the reader's permissions at run time, are pausable, and never reveal matching activity to authors.
- [ ] Author notes, footnotes and endnotes are editor-native blocks, collapsible in the reader, excluded from word counts, and survive export.
- [ ] Text-to-speech reads any chapter, prefers a published podfic unit over generated speech, and stores nothing.
- [ ] A year-in-review page exists, is private by default, and shares history's deletion behavior.
- [ ] Reader layout modes and typography are saved per work and per reader, not per browser.
- [ ] Custom reader CSS is opt-in, sanitized, scoped to the reading surface, and off by default.
- [ ] Accent colour is choosable independently of themes, and the appearance bundle imports and exports as one file.
- [ ] Recipes are per-surface, forkable from shipped first-party recipes, diffable before install, and overridable for-now without editing anything saved.
- [ ] The administrator-influence control is a reader-settable dial whose zero is honored everywhere influence flows, and feed reasons name the reader's own matched terms.
- [ ] Subscriptions to works, series, collections, fandoms and authors deliver update notifications without exposing the subscriber list.

## 28.3 Positive feedback (Priority 3)

- [ ] Positivity filter classifies all author-facing comments.
- [ ] Destructive comments never reach author without explicit opt-in.
- [ ] Constructive critique opt-in per work.
- [ ] Quick reactions bypass filter as pre-classified positive.
- [ ] Anonymous appreciation notes.
- [ ] Reading milestone celebrations.
- [ ] Author feedback preferences per work.
- [ ] Moderator quorum reviews held comments.
- [ ] Repeated negativity from an account triggers rate limits.
- [ ] No public downvote counts or negative review aggregates.
- [ ] Ollama supported as classification provider.

## 28.4 Admin-enjoyable without monothematic (Priority 4)

- [ ] Admin taste profile drives writing prompts and recommendations.
- [ ] Diversity budget reserved in recommendations.
- [ ] Blind-spot suggestions.
- [ ] Surprise-me mode.
- [ ] Cross-fandom dynamic matching.
- [ ] Admin wishlist items appear without ranking advantage.
- [ ] Temporary taste boosts.
- [ ] Meaningful opt-out affects all discovery surfaces.
- [ ] Request candidates match user's own content only.

## 28.5 Trust-level self-governance (Priority 5)

- [ ] TL0–TL6 trust levels based on reviewed conduct.
- [ ] Higher trust grants moderation eligibility.
- [ ] Bootstrap mode labeled honestly.
- [ ] Bootstrap-to-community transition workflow.
- [ ] Administrator override logged with reason.
- [ ] Trust cannot be purchased.
- [ ] Credits and gamification rewards never advance trust level.
- [ ] Leaderboards exist for engagement but do not determine governance authority.

## 28.6 Quorum-based moderation (Priority 6)

- [ ] Two-reviewer minimum for routine curation.
- [ ] Three-reviewer minimum for high-impact changes.
- [ ] Appeals exclude original decision-makers.
- [ ] Positivity classification overrides quorum-reviewed.
- [ ] Extension approvals require independent quorum.
- [ ] Process feedback advisory only.
- [ ] Shadowbans require quorum and expire automatically.
- [ ] DMCA counter-notices route to quorum review.

## 28.7 Translation (Priority 7)

- [ ] Interface fully translated in initial locales.
- [ ] Content translation on demand with credit cost.
- [ ] Human translations override AI translations.
- [ ] Machine translations clearly labeled.
- [ ] Author permission per work.
- [ ] Translator credit awards.
- [ ] Optional comment translation.
- [ ] Shared translation memory with opt-in contribution.

## 28.8 Revenue (Priority 8)

- [ ] Credit ledger with balanced entries.
- [ ] Free tier retains core functionality.
- [ ] Subscription tiers grant quota, not authority.
- [ ] Marketplace revenue split with developers.
- [ ] Standard queues cannot be starved by paid load.
- [ ] Wishlist bounties escrow correctly.
- [ ] Webhook idempotency.

## 28.9 Foundational protections

- [ ] Private libraries and reading history stay private.
- [ ] Pseud linkage remains hidden.
- [ ] Source credentials encrypted and scoped.
- [ ] Extensions have bounded execution at every level.
- [ ] Age policy enforced server-side.
- [ ] Child safety controls operate independently of paid features.
- [ ] Data export excludes secrets.
- [ ] Deletion propagates across systems.
- [ ] Accessibility manually verified.
- [ ] Documentation matches delivered repository.
- [ ] Presence and typing indicators opt-in per pseud.
- [ ] Reading goals and streaks produce no coercive messaging, no guilt-framed loss notifications, and no credit multipliers tied to streak length.
- [ ] Gamification rewards never reveal the administrator's taste profile through any label, multiplier name, or breakdown.
- [ ] Badge counts display "Awarded ×N" per user and global rarity.
- [ ] An instance's media hosting policy is enforced with no per-work, per-user or per-peer override.
- [ ] No embed markup authored by a user, extension or peer is rendered; only the configured provider allow-list is.
- [ ] Unfurling makes no outbound request for an unknown host.
- [ ] A share card or preview never reveals a work the viewer may not read, and never differs between "not found" and "no access".
- [ ] AI spend has a ceiling that no request, subscriber or job batch can raise.

## 28.10 Deliberately not adopted

- **XP and level progression.** Not adopted. A lifetime personal counter — XP, levels, points — is refused for the same reason this section's metric rule exists: it compounds, so it ends up rewarding tenure and volume, it demands a source for every action, and it turns recognition into a race (§9.7.1). What replaces it is episodic and needs no new vocabulary: weekly and monthly leaderboards (§9.7.5) and badges (§9.7.6).
- **Volume-based leaderboards.** No "most words written," "most posts," or "most kudos given" categories, and no operator-added category whose metric is volume — for every category, including a per-public-topic one (§0.4.1), the metric is a quality or theme signal. A volume metric in a leaderboard is a specification error, not a configuration choice. No category is all-time: a placement expires with its window (§9.7.5).
- **Reputation scores.** Not adopted. Three things are refused: a lifetime personal total (XP, levels, points); a score that spans surfaces and compounds; and any score at all — visible or not — that determines trust or governance authority. A per-surface signal that decays on a schedule, confers nothing and weights nothing — §35.2's forum karma — is the boundary case, and it stays what it already is: it displays and does nothing.
- **Coercive streak mechanics.** No guilt-framed loss notifications, no streak-length credit multipliers, no "your streak will break" push notifications. Streaks are cosmetic with optional freezes.
- **Purchased trust or governance authority.** Credits buy compute priority, not moderation power or search ranking.
- **Visible admin taste influence.** The demand multiplier affecting author credits is never labeled, broken down, or hinted at in any user-facing surface. Authors see quality bonuses based on reader behavior only.
- **A promised source count.** Counts follow verified adapter support.
- **A permanent archive of every fetched body.** Shared infrastructure may deduplicate; public preservation requires explicit workflow.
- **Credentials as an SSRF exception.** Internal-network integrations require separate configuration.
- **Popularity-based moderation verdicts.** Process feedback is advisory.
- **Invasive or exclusionary bot detection.** No default fingerprinting, blanket headless-browser bans, compulsory JavaScript for reading.
- **Coercive reading analytics.** No mandatory streaks, no guilt-framed loss warnings, no leaderboard category ranking reading volume or streak length, and no credit multiplier tied to streak length.
- **Automatic publication of AI outputs.** All translations, suggestions, and generated content require review.
- **Purchased visibility or authority.** No paid ranking, trust, or moderation.
- **Silent public visibility of hidden comments.** If a comment is hidden from the author, it is hidden from the public work page.
- **Scripted recipes.** Recipes are declarative configuration; scripting lives in the WASM extension sandbox.
- **Multi-tenancy.** One instance per deployment. Multiple communities run multiple instances.
- **Community suggestions of other users' social links.** Only the author adds their own social links.
- **NodeBB import.** Not applicable to a new platform.
- **Author social proposals as a public queue.** Suggestions may be sent privately to the author, not published.
- **A roadmap that binds the operator.** The board is advice. No rating threshold ships anything, and no ballot overturns a decision that has been made.
- **Purchased roadmap position.** Credits, subscriptions, bounties and marketplace revenue buy no rating and no card movement.
- **Published individual ballots.** Ratings are public; who voted for what is not. A vote that can be seen is a vote that can be pressured.
- **A ranked list of contributors.** The board ranks ideas and never people.
- **Automatic promotion across a threshold.** Crossing a rating promotes nothing; a person decides, in the open.
- **Re-hosting third-party media.** An instance that references media stores metadata and a reference; it never mirrors a video or audio file it was not given permission to host.
- **Storing media an instance has not agreed to host.** The hosting policy is the operator's, stated once, and no upload, import, batch, cache or peer raises it.
- **Author-authored embed markup.** A player is a configured provider on a record, never a frame, script or object inside a document.
- **Unfurling arbitrary URLs.** Only this instance's own content and sources with an adapter are resolved; a preview is never an outbound request aimed by whoever wrote the link.
- **Autoplay, and tracking before consent, in media embeds.**
- **Inferring completion from a third-party player's playback position.**
- **AI spend without a ceiling.** Every task class has a budget, and exhausting it degrades to a named non-AI behavior rather than to silence.
- **Automatic deletion of dormant accounts.** Dormancy changes quota and ends credentials; it never removes what a person wrote.
- **A card or preview that reveals a work to someone who may not read it.**
- **Media hosting as a per-work decision.** "This instance does not host media" and "this media is a work on this instance" must both be expressible, and only an instance-level policy expresses both.
- **A compatibility surface for another service's API by default.** None ships. If an instance ever needs one, §23.11 fixes its shape: a separate prefix, provenance-marked, pinned by tests, with its incompatibilities documented and no privileged path.
- **Treating an unreadable page as an outage.** §11.14 separates a source's health from the quality of what it returned; one challenge page is a rejected candidate with a reason, not a degraded source.
- **Body retention as a per-work decision.** "This instance holds the words" and "this instance knows this work exists and points at it" must both be expressible, and only an instance-level setting expresses both without making every import a decision (§11.15).
- **An aggregating instance that caches "while it is there".** An instance set to `aggregate` stores no body from any path, including a cache fill that would have been convenient.
- **A caching instance that downgrades a failed fetch into a link.** A body that could not be fetched is a failed, retryable import, not a reclassification of the work.
- **Unmoderated guest commenting.** Comments require an account; anonymous appreciation notes (§8.4) are the anonymous channel, because holding and reviewing destructive content (§12) needs an accountable identity behind the text.

## 28.11 Gamification

- [ ] Daily login credits award correctly.
- [ ] Action credits respect daily caps and anti-gaming thresholds.
- [ ] Completion credits enforce minimum time-on-page.
- [ ] Author credits include quality multiplier with visible breakdown.
- [ ] Author credits include demand multiplier with no visible trace of admin taste.
- [ ] Leaderboards rotate on schedule and pay rewards.
- [ ] Leaderboard opt-out hides user from public display.
- [ ] Badges display "Awarded ×N" and global rarity count.
- [ ] Streak resets produce neutral messaging.
- [ ] Streak freezes deduct credits and preserve streak.
- [ ] Seasonal events run with boosted multipliers and unique badges.
- [ ] Credit earnings never affect trust level.
- [ ] Same-account pseuds cannot farm each other's author credits.
- [ ] New/low-trust accounts cannot farm author credits through reactions.
- [ ] No XP, level, lifetime point total or cross-surface reputation score exists on any surface; the only permitted per-surface signal is one that decays and confers nothing (§9.7.1, §35.2).
- [ ] No leaderboard category's metric is volume, and no category is all-time (§9.7.5, §28.10).
- [ ] A badge count never sums into a rank, gates a feature or orders a leaderboard (§9.7.6).
- [ ] Under every `sort` value, the reachable set for one filter is identical (§43.3).
- [ ] No demand weight, weight component or weight-derived label appears in any response, export, error or log line (§16.16.2).
- [ ] A weighted demand result never silently overturns the unweighted one (§16.16.3).

## 28.12 Community roadmap

- [ ] A suggestion is stored verbatim before any interpretation of it.
- [ ] A suggestion is never lost because the embedding provider was unavailable.
- [ ] Daily suggestion limit applies per identity, and signing out does not reset it.
- [ ] A card's representative text is written by its first suggester and never rewritten by later joiners.
- [ ] A card carries the requests that motivated it as evidence.
- [ ] Every card starts at the configured rating and is updated by pairwise comparison, not by a count.
- [ ] A best-worst answer is translated into the full set of pairwise updates, not into one.
- [ ] Rating updates are symmetric: a winner's gain equals the loser's loss.
- [ ] The arena draws least-compared cards first so a new idea is not last for lack of exposure.
- [ ] A pair a voter has already judged is not shown to them again.
- [ ] Every stage on the board is defined as data, and every column sorts by rating descending.
- [ ] No column offers a sort mode that ignores the rating.
- [ ] Moving a card out of Ideas freezes its eligibility according to configuration, and the platform's configuration is stated on the board.
- [ ] Voting requires the configured trust level, and a refusal explains itself.
- [ ] Moving a card is gated on trust level, never on a role.
- [ ] Moving a card never changes its rating.
- [ ] Every move is recorded with its actor, its from and to stage, and a reason.
- [ ] No rating threshold ships a feature; promotion is always a person's decision.
- [ ] The changelog entry for a shipped card links to the card, and the card links back.
- [ ] Only published changelog entries are listed.
- [ ] Who voted on what is not a surface anywhere in the product.

## 28.13 Non-written works and media

- [ ] A work carries a format, and a non-written work is a full citizen of search, collections, series, challenges, comments and federation.
- [ ] An instance states its media hosting policy once, and every surface honors it.
- [ ] A fan film imports with attribution, description, tags, characters and duration, and never as a bare URL.
- [ ] A media series imports as one work with one unit per episode, reconciled against the source's stated count.
- [ ] No player frame is stored or rendered from user, extension or peer markup.
- [ ] A work with no transcript says so; a machine transcript is labeled and replaceable.
- [ ] Unreachable media is marked, not deleted.

## 28.14 Sharing and link previews

- [ ] Every public eligible work URL renders a card and the metadata that points at it.
- [ ] No card or preview exists for a work the viewer may not read, and the markup is identical to the non-existent case.
- [ ] A link to this instance's own work previews with no outbound request.
- [ ] A link to an unknown host is a plain link and triggers no fetch.
- [ ] An author can disable unfurling, and the setting is honored everywhere.
- [ ] Sharing changes no ranking signal and awards no credits.

## 28.15 Monetization, subscriptions, and scraping posture

- [ ] Work monetization follows the instance's eligibility setting, and an imported work is never monetizable in `original` mode.
- [ ] Rights assertions are demanded when `any-with-assertion` is enabled and re-demanded on price changes.
- [ ] Money earnings and credit transfers are separate ledgers, and credits are never convertible to money by the platform.
- [ ] Early-access chapters unlock permanently at `public_at`, and no previously-free chapter is ever locked retroactively.
- [ ] Entitlements survive pseud switching and pricing changes.
- [ ] Purchases, tips and patronages are private; a supporters list is opt-in and the author sees totals, never a roster.
- [ ] Paid works gain no ranking, recommendation or moderation advantage, and their comments pass the positivity filter unchanged.
- [ ] Self-purchases and same-account pseud transfers are refused.
- [ ] Subscriptions meter resource-intensive features without removing the free tier's reduced form of them.
- [ ] AI-training crawlers are disallowed by default in generated `robots.txt`, bulk text endpoints do not exist, and the author `ai_training` assertion is displayed and exported as a stated preference rather than advertised as protection.

---

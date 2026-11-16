# What I would not build if I were building Darkhold from scratch (2026-11-17)

This is an opinionated subtraction list, not a deletion plan. It is written against the checked-in surface inventory (`frontend/scripts/baselines/feature-inventory-v1.json`: 817 routes, 22 nav links, 38 client paths, 958 requirements), `crates/app/src/routes/mod.rs`, `crates/domain/src/modules/registry.rs` and `docs/requirements.csv`. Keep the archive, the reading room, authorship, search, library, access control, imports, basic moderation, privacy, and a small set of admin doors. Almost everything below is a second system built beside one of those.

## Would keep, so the exclusions make sense

Identity and sessions; works and units; server-rendered reader and navigation; search with one query language; library/shelves; ratings/reviews/notes; imports with provenance; moderation and erasure; operator health; tokens/PWA/offline reading; accessible shell; the API surface required by those.

## Repeated ranking and engagement machinery — drop almost all of it

1. Multi-strategy recommendation engine (`rec_run`, strategy blend docs).
2. Recommendation transparency slot-by-slot page.
3. Gravity dial and per-surface quality weights.
4. Gravity ramp / onboarding calibration.
5. Arena pairwise voting as a permanent surface.
6. Jury/GS-5 seating and voting workflow.
7. Finishers signals and "finished" celebration.
8. North-star onboarding quiz.
9. Taste profile ingestion from external platforms as a first-class feature.
10. Taste health dashboard.
11. Taste notification fan-out.
12. Taste proposal/admin surfaces.
13. Quizzes as a content type.
14. Operator hit-rate counter.
15. Trend dashboards.
16. Audience segmentation.
17. CTA engine.
18. "Discover" feed carousels beyond search and tags.
19. Saved-filter scheduler as a product surface.
20. Morning saved-filter digest.
21. Concierge recommendation assistant.
22. Supply drought/keystone operator console.
23. Community-specific supply routing.
24. Cross-language supply and translation-credit fund.
25. Specialists directory by taste domain.
26. WIP adoption program.
27. Retention proposals plus settlement workflow.
28. Preservation credit economy.
29. Reputation scoring beyond a simple trust level.
30. Operator-designed experiment sweep.

## Economy and marketplace — drop the whole band

31. Economy ledger/balances.
32. Payments.
33. Marketplace.
34. Subscriptions.
35. Monetization settings and payout doors.
36. Exchange and exchanges.
37. Exchange admin.
38. Lending.
39. Succession/inheritance.
40. Gifts/tips if they ever land.
41. Bounties.
42. Reputation-as-currency conversions.
43. Financial compliance tooling attached to the above.
44. Invoice/export surfaces for creators.
45. Patron-only tier mirroring.

## Governance, compliance and admin sprawl — keep the doors, drop the ceremony

46. Vanguard.
47. Typed votes.
48. Retention proposal admin.
49. DSAR/compliance tooling beyond a single export/erase path.
50. Policy administration as a UI.
51. Moderation appeal workflow beyond a simple decision log.
52. Trust-level evidence inspector as a standalone page.
53. Rating-integrity audit dashboard.
54. Account permission matrix editor.
55. Admin discovery console.
56. Admin fixity console.
57. Admin action log browser beyond operator audit.
58. Identity-admin trust editing surface.
59. Operator dashboard tiles that duplicate health endpoints.
60. Events stream as a product surface.
61. Webhook operations console.
62. Sandbox surface.
63. Roadmap-as-site feature.
64. Module host UI (keep the registry internally, not the console).
65. Extension review/dispatch UI.
66. OpenAPI browser as a first-class user surface (serve the document, not a page).

## Media and preservation machinery — keep one hash, drop the fleet

67. Media resilience rescuer.
68. Media mirror admin.
69. Automatic mirroring sweeps.
70. Reader body copies and body-fetch doors (one canonical body per unit).
71. Gallery as a separate surface.
72. Per-work media health panels.
73. Reader skins/themes beyond a small reading preset set.
74. Site theme editor.
75. Instance theme database tables.
76. Taxonomy export UI.
77. Tag inference as a user-visible workflow.
78. Tag assertion proposals.
79. Character/coordinate/trope-coverage surfaces unless they serve search directly.
80. Exclusion/leakage dashboards.
81. Canonical/derivative work machinery.
82. Passport/identity portability UI.
83. Peering/ATProto outbound bridge as a user-visible feature.
84. Federation inbox proxy as a first-class surface.
85. Snapshot exports and browser.
86. Bulk export pipeline beyond a single account-data export.
87. OpenSearch integration as an operational dependency.

## Community and social extras — drop or fold into work discussion

88. Community directory.
89. Resource directory.
90. Events calendar.
91. Thread modes.
92. Work discussion as a separate surface (fold into work page).
93. Spoilers system.
94. Notifications centre beyond import/review essentials.
95. Inbox proxy.
96. Follower/subscription graph UI.
97. Public profiles beyond pseud + works.
98. Collaborators/roles editor.
99. Claims workflow.
100. Import claims as a social feature.
101. Sharing/tracking pixels.
102. Reader-side "continue reading" as a separate page (one link on the reader is enough).
103. DNF as a tracked state with reasons; a private note is enough.
104. Preread surface.
105. Reading progress analytics.

## Admin/operator diagnostics that can remain CLI-only

106. Admin overview page (the CLI/health JSON is enough for a one-instance site).
107. Jobs console.
108. Flows console.
109. Analytics page.
110. Work metrics page.
111. Quality page.
112. Preservation page.
113. Health page beyond a status endpoint.
114. Source engagement dashboard.
115. Mirror admin page.
116. Media rescue page.
117. Operator tag scripts.
118. Retention settle UI.
119. Export retention UI.
120. Webhook ops UI.
121. Migration/ops status pages.
122. Atlas of every source and adapter as a user-facing catalogue; a machine-readable file is enough.

## Import/source complexity to replace with one front door

123. Eleven bespoke adapters where three families would do (AO3-software, forum, generic JSON).
124. Declarative CSS-selector manifest as a curator-facing product.
125. WASM adapter sandbox (never worth it for this corpus).
126. Credential/cookie-login choreography for sources that disallow bodies.
127. Per-source body policy matrix (keep one global robots decision).
128. Source adapter route surface.
129. Source health scoring beyond "last good fetch".
130. Multi-route import intake queue with proposal/accept phases.
131. Import claim arbitration.
132. Fanfiction read-list sidecar plus audit sidecar plus separate ranking prior (one private rating table is enough).

## Reader and writing extras to cut

133. In-work search across a whole work.
134. Auto-hiding controls.
135. Bookmarks inside prose.
136. Justification and dyslexia presets beyond two or three fonts.
137. Per-work typography level separate from account typography.
138. TTS/narration voices and audio caching.
139. Side-by-side translation reader.
140. Draft collaboration surface.
141. Writing/planning tools (outside the archive's job).
142. Versioned series metadata beyond one ordered list.
143. Collections with multiple visibility models.
144. Multiple pseud-specific reading profiles.

## What this would look like

A much smaller Darkhold: one catalogue, one reader, one search box, one shelf, one import pipe, one moderation queue and one status page. The removed surfaces are not worthless; they are expensive in tests, migrations, route inventory, accessibility review and operator attention. If a need appears later, add it behind a measurement rather than shipping all 144 doors on day one.

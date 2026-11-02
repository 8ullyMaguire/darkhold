# 0. The Darkhold specification — the index

**This directory is the specification.** It is split from **two masters**, and the
numbers form one space because a citation is a number:

| range | master | what it is |
|---|---|---|
| **§1–27** | `~/secondbrain/10-Projects/ultimate-fanfiction-site-spec.md` | the vault file — remains the master for this range until the split is complete |
| **§28–61** | `../spec.md` | the implementation specification, whose §28–61 are cited by ADRs, amendments and the audit |

**The four rules** (spec §24.2):

1. **A section number is a citation target, and it is never renumbered.** The number space is reserved in blocks of ten, the block titles are fixed when the block opens, and the directory numbers and the document's section numbers are the same numbers — so the split does not renumber anything.
2. **A retired section keeps its number** and gets a withdrawn line. A number is never reused, so a citation to a retired section still resolves to the thing it cited.
3. **A new section takes the next free number,** and the index is updated in the same commit as the section.
4. **Each master remains the master for its own range until the split is complete.** §1–27 come from the vault file and §28–61 from `docs/spec.md`; after the split each becomes an index, and this directory is the specification.

## The section map — §1–27, from the vault master

| slot | file | title | milestone |
|---|---|---|---|
| **01** | `thesis.md` | The thesis | M0 |
| **02** | `evidence.md` | Evidence base | M0 |
| **03** | `inheritance-and-decisions.md` | Inheritance | M0 |
| **04** | `non-goals.md` | Non-goals — the hard refusals | M0 |
| **05** | `vocabulary.md` | The canonical vocabulary | M1 |
| **06** | `domain-model.md` | Domain model | M1 |
| **07** | `data-model.md` | Data model | M1 |
| **08** | `architecture.md` | Architecture | M1 |
| **09** | `ingestion.md` | Ingestion | M8 |
| **10** | `reading.md` | Reading | M5 |
| **11** | `search-and-discovery.md` | Search and discovery | M7 |
| **12** | `taste-themes-moods-channels.md` | Taste, themes, moods and channels | M13, M14, M17, M18 |
| **13** | `community.md` | Community | M12 |
| **14** | `governance.md` | Governance — guests, trust, jury | M3, M20, M21 |
| **15** | `privacy-and-safety.md` | Privacy and safety | M22 |
| **16** | `extensions.md` | Extensions and the marketplace | M23 |
| **17** | `federation.md` | Federation and interoperability | M25 |
| **18** | `the-supporter-tier.md` | The Supporter tier — compute, not rank | M24 |
| **19** | `configuration.md` | The configuration contract | M1 |
| **20** | `api-conventions.md` | The API conventions | M19 |
| **21** | `frontend-and-design.md` | Frontend and design | M26 |
| **22** | `operations-and-budgets.md` | Operations, deployment and budgets | M32 |
| **23** | `verification.md` | The verification discipline | M1 |
| **24** | `the-ledger-and-the-milestones.md` | The ledger and the milestone plan | M0 |
| **25** | `risks.md` | The risk register | — |
| **26** | `open-questions.md` | Open questions | — |
| **27** | `appendix.md` | Appendix — the measured numbers this spec inherits | M0 |

## The section map — §28–61, from `../spec.md`

| slot | file | title | source |
|---|---|---|---|
| **28** | `28-final-completion-checklist.md` | Final Completion Checklist | `docs/spec.md` §28 |
| **29** | `29-community-roadmap-feature-consensus-and.md` | Community Roadmap, Feature Consensus, and Changelog | `docs/spec.md` §29 |
| **30** | `30-non-written-works-media-and-aggregation.md` | Non-written Works, Media, and Aggregation | `docs/spec.md` §30 |
| **31** | `31-sharing-fic-cards-unfurling-and-link-previews.md` | Sharing: Fic Cards, Unfurling, and Link Previews | `docs/spec.md` §31 |
| **32** | `32-generalized-media-platform-creators.md` | Generalized Media Platform: Creators, Distributors, Collections, and the Media Query API | `docs/spec.md` §32 |
| **33** | `33-consent-integrity-and-transparency-draft-v1.md` | Consent, Integrity, and Transparency (draft v1, spec-only) | `docs/spec.md` §33 |
| **34** | `34-decision-services-calibrated-classifiers.md` | Decision Services (calibrated classifiers) | `docs/spec.md` §34 |
| **35** | `35-forum-as-a-first-class-surface-milestones-3135.md` | Forum as a First-Class Surface (Milestones 31–35, repo) | `docs/spec.md` §35 |
| **36** | `36-growth-sharing-and-ecosystem-expansion.md` | Growth, Sharing & Ecosystem Expansion | `docs/spec.md` §36 |
| **37** | `37-multi-platform-companion-bot.md` | Multi-Platform Companion Bot | `docs/spec.md` §37 |
| **38** | `38-instance-configuration-the-self-hosted-contract.md` | Instance Configuration — The Self-Hosted Contract | `docs/spec.md` §38 |
| **39** | `39-resource-directory-a-community-curated-map-of.md` | Resource Directory — a community-curated map of the fandom ecosystem | `docs/spec.md` §39 |
| **40** | `40-remix-fork-with-provenance-and-permission.md` | Remix — fork with provenance, and permission statements made enforceable | `docs/spec.md` §40 |
| **41** | `41-longevity-and-ambient-social-signals.md` | Longevity and ambient social signals | `docs/spec.md` §41 |
| **42** | `42-export-ctas-instance-configurable-curator-marked.md` | Export CTAs — instance-configurable, curator-marked | `docs/spec.md` §42 |
| **43** | `43-recommendation-first-browsing-one-ordering.md` | Recommendation-first browsing — one ordering contract for every surface | `docs/spec.md` §43 |
| **44** | `44-roadmap-consensus-an-elo-ranked-community-voted.md` | Roadmap Consensus — an Elo-ranked, community-voted feature board | `docs/spec.md` §44 |
| **45** | `45-directory-category-governance-community.md` | Directory Category Governance — community-moderated categories and entry management | `docs/spec.md` §45 |
| **46** | `46-user-configuration-the-customizable-website.md` | User Configuration — the customizable website belongs to its users | `docs/spec.md` §46 |
| **47** | `47-ranking-the-substrate-the-discovery-mechanisms.md` | Ranking — the substrate the discovery mechanisms plug into | `docs/spec.md` §47 |
| **48** | `48-the-firefox-companion-a-browser-extension-over.md` | The Firefox companion — a browser extension over `/api/v1` | `docs/spec.md` §48 |
| **49** | `49-taste-signal-making-the-profile-worth-ranking-on.md` | Taste signal — making the profile worth ranking on | `docs/spec.md` §49 |
| **50** | `50-reasons-dimensions-and-canon-blind-discovery.md` | Reasons, dimensions, and canon-blind discovery | `docs/spec.md` §50 |
| **51** | `51-generated-content-and-who-gets-credit-for-what.md` | Generated content, and who gets credit for what | `docs/spec.md` §51 |
| **52** | `52-taste-leakage-what-a-public-artifact-gives-away.md` | Taste leakage: what a public artifact gives away | `docs/spec.md` §52 |
| **53** | `53-faucets-and-sinks-the-economy-as-an-operator.md` | Faucets and sinks: the economy as an operator can read it | `docs/spec.md` §53 |
| **54** | `54-the-personal-concierge.md` | The personal concierge | `docs/spec.md` §54 |
| **55** | `55-curator-submitted-source-adapters.md` | Curator-submitted source adapters | `docs/spec.md` §55 |
| **56** | `56-wip-adoption-closure-notes-and-lineage.md` | WIP adoption, closure notes, and lineage | `docs/spec.md` §56 |
| **57** | `57-continuing-and-the-honest-shape-of-coming-back.md` | Continuing, and the honest shape of "coming back" | `docs/spec.md` §57 |
| **58** | `58-supply-keystone-authors-droughts-and-the.md` | Supply: keystone authors, droughts, and the routing record | `docs/spec.md` §58 |
| **59** | `59-the-module-host-m65-a-small-core-and-every.md` | The module host (M65): a small core, and every surface as a switchable module | `docs/spec.md` §59 |
| **60** | `60-finisher-signals-say-what-was-finished-never.md` | Finisher signals: say what was finished, never what was not | `docs/spec.md` §60 |
| **61** | `61-cross-language-supply-fund-a-translation-and.md` | Cross-language supply: fund a translation, and report the gap honestly | `docs/spec.md` §61 |

**Slots 62 to 70 are reserved and unallocated:** **62**, **63**, **64**, **65**, **66**, **67**, **68**, **69**, **70**.

## Which document governs

| | | |
|---|---|---|
| **this directory** | the Darkhold specification, split into sections — **the governing document** |
| **the first master** | `~/secondbrain/10-Projects/ultimate-fanfiction-site-spec.md` — §1–27, the master until the split is complete |
| **the second master** | `../spec.md` — §28–61, still cited directly by ADRs and amendments, so it is a master rather than history |
| **the implementation plan** | `~/secondbrain/10-Projects/darkhold-implementation-plan.md` — 255 task cards, M0–M32, the build order |
| **the authority for what is built** | `../requirements.csv` — the requirement ledger, and ADR 0028 for how it is re-triaged |

## How this directory was made

`python3 scripts/split-spec.py` re-derives every file from the two masters, and
`python3 scripts/check-split-spec.py` asserts that each file's heading number
matches its filename and that the split is in step with **both** masters. **A split
that is done by hand is a split that is done once**, and a section that changed
in a master would show up as a divergence nobody notices.

## How to read this

**Normative vocabulary.** MUST / MUST NOT / SHOULD / MAY are binding. "Defined" means a term has
exactly one meaning and the three source projects' words for it are mapped in §5. "Proven" means a
number was measured on real hardware against real data and is re-derivable — never "estimated".

**The evidence rule.** Every claim of the form "X works" cites a measurement or says plainly that
it has not been built. A feature mentioned in a document is not evidence of implementation.

**Reading order for a fresh agent.** §1 (thesis) → §3 (inheritance) → §8 (architecture) →
§12 (taste) → §23 (verification) → §24 (ledger). Everything else in the order it is needed.

---

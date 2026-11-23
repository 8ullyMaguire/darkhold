# Optional community products — milestones M12–M20

**Decision 2026-11-23 (delegated best judgement).** These nine milestones are a *different product* from the core
catalogue and reader. `reviews/darkhold.md` C6 named the scope: comments and forums, collections and challenges,
quorum governance and appeals, credits/queues/bounties/billing, marketplace and webhooks, translation, public API
and provider integrations, administration and abuse defence, hardening. The registry may enable them per instance;
the monolithic spec still costs every instance all of them. This file separates the costing.

## Rules

1. **Core does not depend on any row below.** A deployment with all nine disabled must pass its own gates. A core
   change that requires M12–M20 to be enabled is a defect in the core change.
2. **Each row needs a named enabling instance before work starts.** A per-instance flag with no instance asking for
   it is an unproven feature (estate ADR 0079). The flag is off by default.
3. **Each row keeps its own gate and evidence.** Do not add these to the core milestone counter; do not let a green
   core suite be reported as proof of an optional product.
4. **Billing is the hardest gate, not the default.** Credits, fair queues and bounties (M15) need a written payments
   story and a legal review before they are enabled anywhere; a queue that can be bought is a different contract
   from a queue that can be waited in.

## Dispositions

| milestone | product | disposition | gate to start |
|---|---|---|---|
| M12 | comments and forums | OPTIONAL, off by default | one instance asks for moderated comments and names its moderator |
| M13 | collections and challenges | OPTIONAL, off by default | one instance names a curator and a challenge dataset |
| M14 | quorum governance and appeals | OPTIONAL, off by default | one instance adopts the quorum roles and names an appeals owner |
| M15 | credits, fair queues, bounties, billing | GATED — payments story and legal review first | written payments story; no native commerce before that (estate LinkArena rule, row 516) |
| M16 | marketplace and webhooks | OPTIONAL, off by default | one external consumer and its signed call site |
| M17 | translation | OPTIONAL, off by default | one locale with a named maintainer |
| M18 | public API, bots, push, federation, AI providers | SPLIT — API and bots are core-adjacent; federation and AI providers are separate contracts | each sub-contract has its own enabling instance and gate |
| M19 | administration and abuse defence | CORE-ADJACENT — moderation tooling needed by any instance with users | name the moderator role and the audit-log reader |
| M20 | hardening | CORE — applies to every instance | never optional; this is a property of the core, not a product |

## What this is not

Not a deletion of M12–M20, not a claim that their spec text has been removed, and not a claim that any instance has
enabled them. This is a costing and sequencing decision; the spec text and requirement rows remain until an owner
decides to reclassify them in place.

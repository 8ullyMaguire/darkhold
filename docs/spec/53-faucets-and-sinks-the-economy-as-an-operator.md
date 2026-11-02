# 53. Faucets and sinks: the economy as an operator can read it

**Status:** reserved
**Milestone:** —
**Source:** §53 of `docs/spec.md`

M45-18, from A11. A11's concern was inflation: "faucets have multiplied —
curation, referrals, topic bonuses, standing bounties, lifecycle multipliers" — and
its rule of thumb is the sentence this section has to make enforceable:

> **Faucets pay for signal or supply. Sinks convert credits into supply. Credits
> stay closed-loop and never cashable.**

§20.3 lists thirty-odd faucets and §20.2 a table of job costs. Read as a list,
that is thirty-odd rules and no shape. A11's contribution is the *classification*,
and the classification is the part worth enforcing, because it is the part an
operator can check without reading every row.

## 53.1 Every faucet and every sink declares which side it is on

- **A mechanism is a faucet, a sink, or neither, and it says so.** The
  declaration is stored, not inferred from the sign of its ledger entries. Signs
  are a *consequence* of the design and a bug in a faucet produces a negative
  amount, which is exactly the situation where inferring the side from the sign
  would be wrong.
- **A faucet pays for signal or supply.** Signal means an action that tells the
  instance something it did not know — a reader finishing, a reader saying why, an
  import bringing a work in. Supply means work, labour, or attention that makes
  the corpus better: an author publishing, a translator producing a chapter, a
  curator answering a drought.
- **A sink converts credits into supply.** It spends credits to obtain a thing,
  and the thing is the point. A sink that bought *rank* would be a purchase of
  ranking, which §0.3 forbids outright, so that is not a category any sink may
  declare.
- **Neither is allowed to be undeclared.** A mechanism with no declaration is a
  mechanism the operator cannot reason about, and a dashboard that silently omits
  it would be reporting a smaller economy than exists — which is the inflation
  problem restated.

## 53.2 What the dashboard shows, and what it refuses to show

- **The dashboard is a balance and a composition, not a leaderboard of
  mechanisms.** Per-window totals for faucets and for sinks, the net, and the
  ratio between them. An operator's question is "is this instance minting or
  burning", and one number answers it.
- **A net above a configured threshold is a warning, never an automatic
  throttle.** §0.3 makes bought ranking and bought trust non-negotiable, and a
  threshold that silently clamped would be the economy deciding what a reader may
  earn. The dashboard tells the operator; the operator acts (§43 proposal
  machinery), or does not.
- **Per-account balances are never in this view.** It is an economy view, not a
  ledger: §0.3's "readers control their data" means an operator looking at
  aggregate flow is doing something different from an operator looking at one
  person's balance, and only the first belongs here.
- **No mechanism is shown as "too generous".** Inflation is a property of the
  *total*, not of any row, and a dashboard that named a culprit would push the
  operator toward cutting the visible faucet instead of the invisible one.

## 53.3 The closed-loop guarantee is structural, not a policy statement

- **Credits are never cashable.** No mechanism, at any tier, converts credits
  into money or anything redeemable for it. §20.9 moves real money for *work
  purchases* and tips — a reader pays the author, not the author's credit balance
  — and the distinction is load-bearing: it is what lets §20.9 exist at all
  without becoming an exit.
- **There is no exchange rate.** A closed loop has no conversion, so a rate would
  be a rate out of the system. This is why the guarantee is structural: there is
  no number in the schema that could be pointed at money.
- **A credit that cannot be spent is a bug, and the dashboard says so.** Credits
  exist to be consumed by sinks; an unspent balance is either a reader who has not
  found a sink yet or a mechanism paying for nothing. Both are worth an operator's
  attention, and neither is a reason to invent an exit.
- **No credit purchase of any kind.** §20.1's ledger has an `earned|granted|
  purchased|held` bucket vocabulary, and `purchased` is where an exit would
  appear. It may be used only for credits granted in exchange for money already
  spent on this instance's work (a tip, a subscription month), never for credits
  sold.

## 53.4 What this section does not do

- **It does not set the faucet values.** §20.3's table is unchanged; the
  classification says what a faucet is *for*, not how much it pays.
- **It does not replace §9.7's caps.** A faucet with a cap is still a faucet, and
  the cap stays where it is.
- **It does not predict inflation.** The dashboard reports what happened in a
  window. Attributing a ratio to a mechanism would be the same error §52.2
  refuses — a payment standing in for a cause.

## 53.5 Hit rate: the operator's one number

The list this section answers asks for a north-star metric — "the share of feed
items you finish or rate ≥4" — and every other lever judged against it. The number
does not exist, and it is the cheapest thing here with the largest effect on every
other decision, because without it the operator is tuning blind and can only tell
whether a change helped by remembering.

- **Hit rate is defined over the operator's own signals, and over nothing else.**
  Of the works the operator was *shown*, the share they went on to finish or rate
  four stars or better. Two clauses, both deliberate: a work never shown cannot be a
  miss, and a work shown and never acted on counts as a miss rather than as
  absent — otherwise the metric rewards showing less.
- **It is a ratio with a denominator, never a bare count.** "Twelve hits" is
  indistinguishable between a feed of twelve and a feed of two hundred, and an
  operator comparing two weeks needs the rate.
- **It is scoped to a window and reports the window's own denominator.** A lifetime
  rate on a feed whose character changed three months ago is an average of two
  different questions.
- **It is operator-only, for §0.3's reason and not merely as convention.** On a
  single-reader instance a hit rate *is* a description of that reader's taste. It
  belongs in the operator view beside §52's leakage view and nowhere else, and it
  is never aggregated into anything a reader can see.
- **It is a diagnostic, not a target.** §53.2's refusal of automatic throttles
  applies here with more force: an instance that optimised for hit rate would
  surface only safe picks, which is the same narrowing §52.2 refuses. The number is
  read, not chased. A window with a very low rate is a statement about the recipe or
  the import mix, and the response is to change one of those deliberately.
- **It reports its own missing inputs.** With no ratings and no completions the
  rate is undefined, not zero: reporting zero would read as total failure and
  invite a change to a recipe that has simply not been tested yet.

**As built (`2026-11-02`, `M66-09`).** The store existed and nothing served it: `grep -rn hit_rate
crates/app` returned no route, so the one number this section calls the cheapest thing here could
only be read by writing SQL against the instance by hand. It now has a door,
`GET /api/v1/operator/hit-rate?window_days=N`, registered as the extension `operator_hit_rate`
(`default_on: true`, `toggle: operator`).

- **The window is clamped, not trusted** — `window_days` is bounded to `1..=365` and echoed back with
  `since`/`until` (unix seconds). The bound is what keeps "a lifetime average" out of a metric whose
  whole argument is that a rate is scoped to a window.
- **The window is half-open, `[since, until)`.** An impression served in the second the request
  arrives is not yet inside it. That is correct — a slot cannot be a hit before it is served — but a
  fixture that seeds "now" measures zero, which is how the first version of the test failed.
- **Undefined is a field, not an inference.** The response carries `"rate": null` *and*
  `"defined": false`; a client must never have to decide whether a null means "not measured" or
  "measured nothing".
- **Refusal is one shape.** A non-operator gets `404`, and an instance that does not run the
  extension also gets `404` (`MODULE_UNAVAILABLE`). A `403` for the first would confirm the surface
  exists, which on a single-reader instance is itself a statement about the reader; two different
  codes would let a caller map the operator's configuration from outside.
- **The pseud is the session's, else the account's first.** With no pseud there is no feed and no
  impressions, so the door answers `404` rather than inventing an account-wide number this section
  does not define.

Five tests in `crates/app/tests/operator_hit_rate.rs`; mutation-checked by making `defined` always
true, dropping the operator check, and removing the clamp — each fails exactly the named test.

## 53.6 Earned bookmark: the ratio that cannot be faked

The ideas list asks for a bookmark-to-hit ratio, on the argument that it is cheap to
compute and hard to fake: faking a bookmark costs one click and faking a completion
costs a reader's time. The argument is better than the feature's priority suggests,
because this is a **quality signal** rather than a ranking tweak, and §20.3's author
multipliers are where it would pay.

**The term needed defining first, and the definition has a real choice inside it.**
A "hit" could be a chapter view (`work_view_log`) — plentiful and free to inflate, since
a refresh costs nothing — or a `reading_status` transition to `finished` — sparse and
expensive to fake. A ratio whose numerator is the cheap signal measures nothing: any
ratio of bookmarks to views is a statement about how the reader browses.

- **A "hit" here is a completion, and it is `finished` alone — not §53.5's
  "finished or rated >=4".** The two definitions coexist deliberately. §53.5's hit
  answers "is my feed working?", where a four-star rating is real evidence that a
  reader engaged. This ratio answers "does this work earn the reader's time?", and a
  four-star rating on chapter one is not that: it costs one click, exactly what the
  ratio is meant to be measured against. Reusing §53.5's definition would put the cheap
  signal inside the expensive one.
- **It is per work, over a window, with the denominator reported.** Both counts are
  scoped the same way, because a ratio whose numerator is windowed and whose
  denominator is not measures a change in the denominator.
- **A work with no bookmarks is excluded, not counted as a ratio of zero.** A zero
  denominator is not a measurement; §53.5's "undefined, not zero" applies here for the
  same reason.
- **It is an input to §20.3's author multipliers and to nothing else.** It does not
  become a ranking signal, and it is never surfaced to a reader — a work's ratio is a
  statement about the people who read it, which is the same §0.3 reasoning that keeps
  the leak rate in the operator's view.

---

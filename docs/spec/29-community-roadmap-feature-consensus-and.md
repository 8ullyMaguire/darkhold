# 29. Community Roadmap, Feature Consensus, and Changelog

**Status:** reserved
**Milestone:** —
**Source:** §29 of `docs/spec.md`

Readers propose what the platform should build next, the community ranks the proposals against each other, and an operator moves cards across a board and ships them. The ranking does not replace the operator's judgement; it makes the community's judgement legible before the operator exercises theirs. A wish list sorted by nothing tells an operator what people said. A ranked board tells them what people chose when they had to choose, which is a different and more useful thing.

Everything in this section is subject to §0.3. Ranking here is advice, never authority: it cannot purchase, confer, or override anything the foundational protections reserve.

## 29.1 Suggestions and clustering

A suggestion is 1–1000 characters of free text.

- **The raw text is stored before it is interpreted.** It is the reader's own words and the record of what they asked for; nothing derived from it may take its place.
- **Rate limit: three suggestions per identity per rolling day.** The identity is the acting pseud for a signed-in reader and a client identifier for a guest, so signing out does not clear the allowance. The limit exists because a suggestion system without one becomes a channel for one determined voice to look like a crowd.
- **Clustering.** The text is embedded and compared against the cards still in Ideas. Within the configured distance threshold it joins that card as another voice for the same idea; beyond it, it opens a new card seeded with its own text. Clustering is what keeps a board of forty cards from being a board of four hundred, and it is what makes a card's rating mean something: one idea, ranked once.
- **A card's representative text is written once, by whoever named the idea first, and later joiners never rewrite it.** A text that drifted toward the newest suggestion would make a card's name a function of arrival order rather than of the idea, and a reader who joined a card called "Better search filters" would find it renamed tomorrow.
- **Embedding is best-effort, and its failure is not the reader's.** When the embedding provider is unavailable the suggestion is stored unclustered and the reader is told it was recorded. A suggestion is never lost because a model was down, and the stored raw text is what makes the idea recoverable — and clusterable — once the provider returns.
- **A card carries the requests that motivated it** as evidence, so its existence is the record of a real demand rather than an operator's assertion that one exists.

## 29.2 Ranking: pairwise comparison, not vote counts

Cards are ranked by a rating derived from best-worst comparison (MaxDiff) rather than by an upvote count.

- Every card starts at the configured seed rating with the configured K-factor, and the values are configuration, not constants in code.
- A voter is shown four cards and names the best and the worst. That one act becomes the full set of virtual one-to-one matches implied by it: the best beats each of the other three, the worst loses to each of the other three, and each of the two between them beats the worst, loses to the best, and draws the other.
- **Why not upvotes.** An upvote measures how many people saw an idea; it does not measure how much they prefer it. Two popular but interchangeable ideas both accumulate votes and neither is distinguished, and an idea that is widely liked but never anyone's favourite outranks the one a community actually wants built. Forcing a choice produces an ordering, which is the only thing a roadmap needs.
- **The update is symmetric.** A winner gains exactly what the loser drops, so a board cannot inflate as votes accumulate and a card's rating means the same thing in a quiet month as in a busy one.
- **The arena draws the least-compared cards first**, with a random tiebreak among them. A card's position must reflect preference rather than exposure: without this, a new idea sits last because nobody has seen it, and stays there because nobody sees a card that is last.
- A pairing a voter has already judged is not shown to them again.

## 29.3 The board, its stages, and what freezes

A card is in exactly one stage. Stages are defined as data — identifier, label, colour, board position, and whether they are open to comparison — so the board's shape is configuration rather than a list hard-coded into the interface. Board position determines left-to-right order, and the shape a deployment ships is:

| Position | Stage | Label | Open to comparison |
|---|---|---|---|
| 1 | `up_next` | Up Next | no, by default |
| 2 | `in_progress` | In Progress | no, by default |
| 3 | `finished` | Finished (not yet shipped) | no, by default |
| 4 | `shipped` | Shipped | no, by default |
| 5 | `medium_term` | Medium-term | no, by default |
| 6 | `long_term` | Long-term | no, by default |
| 7 | `idea` | Ideas | yes |
| 8 | `rejected` | Rejected | no |

- **Moving a card out of Ideas freezes it by default**, so a decision the operator has already made cannot be dragged around afterwards by traffic that arrives later. A deployment may open every stage except Rejected, so consensus keeps accumulating on planned work; whichever it does, the board says which, because a reader who cannot tell whether their vote still counts has been told nothing.
- **Every column sorts by rating, descending, and no column offers a mode that ignores it.** A "popular" sort beside a rating sort would mean two answers to the same question, and the one people clicked would become the real one.
- **The rating is shown on the card.** A board that ranks by community preference and hides the preference asks readers to take the ordering on faith.
- Categories (search, scraper, social, reader, admin, recs, general, and their labels) are defined as data as well and filter across all columns rather than within one.
- Rejected is reachable and carries a reason. A suggestion system that cannot say no teaches readers that their ideas were not read.

## 29.4 Who may vote, who may move, and what a move means

- **Voting requires the configured minimum trust level** (§19.1), and a reader below it is refused with an explanation rather than shown a ballot that will be discarded.
- **Moving a card is gated on trust level, not on a role.** The platform has no role column; the gate is the same trust ladder that governs every other curation tool, and it is set high enough to be a deliberate appointment rather than a consequence of participation.
- **A move never changes a card's rating.** The community's ordering and the operator's decision are separate facts, and a system that adjusted the rating to match the stage would have quietly replaced the first with the second.
- **Every move is recorded** with its actor, the from and to stage, and a reason, in the same public log that moderation actions use (§19.12). A roadmap whose cards move with no record invites the belief that they move for reasons nobody will name.
- **No rating threshold ships a feature.** Crossing a number promotes nothing; promotion is always a person's decision, taken in the open.

## 29.5 Changelog

Shipped work is announced, and the announcement is linked to the demand that produced it.

- An entry has a date, a title, a body, one of three kinds — New, Improved, Fixed — and an optional link to the card it shipped.
- Entries have a draft and a published state, and only published entries are listed.
- **The link is bidirectional**: a shipped card points at the entry that shipped it and the entry points at the card. The purpose is that a reader who voted for something can find out what became of it without asking, which is the difference between a roadmap and a suggestion box.

## 29.6 Privacy

- **Which reader voted on what is not a surface anywhere in the product.** Ratings are public; individual ballots are not. A visible vote is a vote that can be socially pressured, and a roadmap where readers can see each other's choices ranks the ideas that are safe to endorse rather than the ones people want.
- A suggestion is attributed to a pseud and the pseud's linkage rules apply as everywhere else (§7.2). A reader can see their own suggestions; nobody can see another reader's ballot.
- The board is not a place for the demand multiplier or the administrator's taste profile to be inferred (§0.3). Board order derives from reader comparisons and from nothing else.

## 29.7 Data model

| Table | Holds |
|---|---|
| `feature_clusters` | One card: representative text, its embedding, its rating, how many comparisons it has taken part in, how often it was named best and worst, its stage, its category, and its provenance. |
| `feature_suggestions` | One raw suggestion: its text, its identity, when it was made, and the card it landed on or none if it could not be clustered. |
| `feature_votes` | One ballot: the cards it compared, which was named best and worst, and who cast it — retained so a voter is not shown the same pairing twice, and not exposed. |
| `roadmap_stage_events` | One move: the card, the from and to stage, the actor, the reason, and when. |
| `roadmap_changelog` | One entry: title, body, kind, the card it shipped, its author, and its published state and moment. |

Counts and ratings are ordinary columns, not derived views: a rating that had to be recomputed from every ballot would make the board's cost grow with the community's engagement, which is the opposite of what a growing community needs.

## 29.8 API

| Method | Path | Purpose | Minimum trust |
|---|---|---|---|
| POST | `/api/v1/roadmap/suggestions` | Record a suggestion and cluster it if possible. | any reader (rate limited) |
| GET | `/api/v1/roadmap/arena` | Return the cards to compare next. | any reader |
| POST | `/api/v1/roadmap/arena/votes` | Record a best-worst ballot and apply its rating updates. | voting minimum |
| GET | `/api/v1/roadmap/features` | List cards by stage and category, rated descending. | any reader |
| GET | `/api/v1/roadmap/board` | Every column, grouped and rated descending. | any reader |
| PATCH | `/api/v1/roadmap/features/{id}` | Move a card between stages with a reason. | curation minimum |
| GET | `/api/v1/roadmap/changelog` | Published entries, newest first. | any reader |
| POST | `/api/v1/roadmap/changelog` | Write an entry, optionally linked to a card. | curation minimum |

## 29.9 Acceptance

- A suggestion is readable in its own words after a clustering failure, and clusters correctly once embedding succeeds.
- A best-worst ballot moves all the ratings the pairing implies, and the sum of gains equals the sum of losses.
- A card moved out of Ideas is no longer drawn by the arena under the freezing configuration, and remains available to it under the open one.
- A reader below the voting minimum is refused, and the refusal explains the requirement.
- A reader below the curation minimum cannot move a card, and the attempt is recorded rather than silently dropped.
- The board renders every stage in its configured order, each column rated descending, with no alternative sort.
- A move appears in the log with its actor and reason, and the card's rating is unchanged by it.
- A published changelog entry and the card it shipped link to each other.
- No request in this section returns a ballot cast by another reader.

---

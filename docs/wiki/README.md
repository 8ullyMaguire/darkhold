# Vendored reference: the AO3 recommender study

**Upstream:** <https://github.com/mmarcet/AO3_recommender_system> — wiki at
<https://github.com/mmarcet/AO3_recommender_system/wiki>
**Retrieved:** 2026-11-02, by `git clone --depth 1 https://github.com/mmarcet/AO3_recommender_system.wiki.git`
**Author:** mmarcet (Kschool data-science master project, 2020)
**Licence:** the upstream repository states none at the time of vendoring. This copy is kept as a
**cited reference**, not as code to execute: nothing here is linked, compiled or imported, and nothing
here should be relicensed or republished from this repository. If upstream adds a licence, record it
in this file before anything else changes.

## Why it is here

It is the only *measured, published* comparison of every recommender family this repository names —
popularity, tag content-based, item→item, user-based collaborative filtering, and matrix
factorisation — on real AO3 data, with the numbers and the traps written down. Two of its findings are
now normative here and neither is guessable from first principles:

- **recency-only and length-only ordering scored *below random*** as recommenders (§12.7a);
- **a pure tag-profile cosine scored 0.0016 f1@20 against popularity's 0.0364**, while the same tags
  used item→item reached 0.0351 (§12.7a, §12.8).

The doctrine derived from it lives in the specification, not in this directory:
`docs/spec/12-taste-themes-moods-channels.md` §12.7a, `docs/spec/24-the-ledger-and-the-milestones.md`
§24.4, and the harness plan `docs/plans/m64-16-offline-pre-registration-harness.md`.

## Contents

| file | what it is |
|---|---|
| `Home.md` | the study's summary: four recommender families, the 121k-work Marvel sample, the headline improvement graph |
| `0.-Set-up.md` | data files, Python 3.6.9 requirements, the `common_functions.py` path trap |
| `1.--Data-retrieval-and-pre-processing.md` | the scraper, the 5-second delay the archive asked for, duplicate removal across multi-day runs, the **5,000-page cap per filter set**, the user→item file, the masked train/val/test split, the author-rating file |
| `2.--Non-personalized-recommender.md` | random / longest / most-recent / kudos / bookmarks / comments / hits, and their metrics |
| `3.-Content-based-recommender.md` | TF-IDF and counts over tag classes, the reader-profile dot product, the item→item variant, the tag-class ablation |
| `4.-Collaborative-based-recommender-(Similarity).md` | cosine user neighbourhoods, the neighbour-count sweep, the threshold sweep |
| `5.-Collaborative-based-recommender-(Matrix-factorization).md` | `implicit` ALS/LMF/BPR, random hyperparameter search, the winning figures, the variety histogram |
| `6.--Evaluation-tool.md` | precision@k, recall@k, F1@k, MAP@k — the tool every number above came from |
| `7.-Additional-recommendations.md` | item→item by tags and by the MF model, the author-level LightFM variant with 1–5 graded ratings |
| `8.--Dashboard.md` | the Dash dashboard, and the "a new user gets the most-kudos list" default |

**Images are not vendored.** The wiki's figures live in the upstream repository under
`master/figures/` and the pages link them by URL; this copy is text-only, so a figure renders only
with network access.

## What does not transfer, and must not be copied as a target

- **The corpus.** 9,998 readers and 121,367 Marvel works, a 2020 snapshot, kudos as the only positive
  signal. This instance's readers have progress, completion, ratings, arena ballots and shelves.
- **The numbers.** They are reference points on a ladder, not goals (§12.7a says so in as many words).
- **The split.** The study masked interactions at random because its corpus carried no timestamps, so
  it trained on the future and scored the past. Every interaction in this repository has a clock, so
  the harness here uses a temporal holdout and a random mask is the labelled fallback.
- **The scraper's mechanics.** The 5,000-page cap and the 5-second delay describe AO3's UI and its
  request of 2020; the ingestion posture here is governed by §9 and M59, not by this page.
- **`bookmarks` as a ranking signal.** The study measured it slightly worse than kudos, and this
  repository's own `work_metric_aggregates` carries both anyway — read the numbers, do not copy the
  choice.

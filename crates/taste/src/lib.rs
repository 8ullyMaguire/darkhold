//! The taste engine.
//!
//! **This crate is a pure function of a catalogue.** It does no I/O to the archive's
//! database, and that is the design rather than a style preference: the catalogue is a
//! table (D-7), the taste engine's working set is an in-memory graph, and a ranking that
//! reads the archive's state is a ranking that cannot be measured. The ranker is the one
//! thing in Darkhold whose output has to be *comparable across runs*, and that requires
//! its inputs to be.
//!
//! The crate exists as a separate one because of a measurement, not a preference. The
//! archive's working set is a request/response process with a database pool, a sanitiser, a
//! job queue and a web server. The taste engine's working set is ~27 MB of capped adjacency,
//! ~80 MB of 32-dimensional embeddings, ~23 MB of a co-bookmark index and ~30 MB of an
//! entity index. **In one process the peak is the sum of the phases, not the maximum**, and
//! the sum does not fit on the target hardware (spec §8.2).
//!
//! So the split is configurable — `[taste] mode = "embedded" | "service"` — and **the
//! domain is shared either way**. The two processes never share a database connection.
//!
//! **Nothing in here pretends to work.** A module is either implemented and tested or empty
//! and explicit about which section of the specification it will implement. A stub that
//! returns a plausible number is worse than an empty module, because a stub is a ranking that
//! looks measured and is not.
//!
//! Implemented so far: [`eval`] (the ranking metrics the parity harness scores with) and
//! [`ladder`] (the baseline ladder a challenger is measured against). Everything else is still a
//! declaration.
//!
//! **The Glicko implementation left on 2026-11-04**, with its 15 unit tests and its kindred parity
//! fixture: the arena's ladder is the estate's taste service (`polaris-taste`, ADR 0001), because a
//! rating of a work must mean the same thing to four products and a crate cannot be linked into the
//! Go and Python ones. The parity test moved with it, so the proof that the Rust port matches
//! kindred's oracle still runs — in the crate that now owns the algorithm.

/// The catalogue: the instance's works, tags and entities, ingested into the on-disk
/// index the engine reads (spec §8.2, §12.7).
pub mod catalogue;

/// The co-occurrence graph: the capped adjacency the walk and the neighbourhood
/// expansion run over (spec §12.2).
pub mod graph;

/// Embeddings: the 32-dimensional tag and entity vectors, and the similarity they
/// support (spec §12.2).
pub mod embed;

/// Collaborative filtering: the item-item neighbourhood the recommender leans on, and
/// the co-bookmark index beside it (spec §12.7).
pub mod collab;

/// The signals: the individual pieces of evidence a reader's profile is built from, each
/// carrying its own origin and age (spec §12.7).
pub mod signal;

/// Diversification: the re-weighting that keeps a page from being one author's work
/// (spec §12.7).
pub mod diversify;

/// The arena: the pairwise comparison surface and the ladder it puts works on
/// (spec §12.4).

/// Ranking metrics: `nDCG@10`, `recall@50`, `precision@k`, `F1@k`, `AP@k` and overlap, which is how
/// "we match kindred" becomes a number instead of a claim (spec §8.3, R-6).
/// The ranking metrics, re-exported from `polaris-eval`: nDCG, recall, precision, MAP and the parity report.
///
/// A re-export rather than a module so this crate's existing paths (`crate::eval::ndcg_at`,
/// `darkhold_taste::eval::Judgement`) keep working unchanged — the callers never learn the code moved.
pub mod eval {
    pub use polaris_eval::*;
}

/// The baseline ladder: every rung a recommender is placed against on one split, and the
/// pre-registered conjunction a challenger has to clear (spec §12.7a, §24.4; requirement M64-16).
pub mod ladder;

/// The ranker: the blend that turns a reader's profile and a candidate work into an
/// order (spec §12.7, §19).
pub mod rank;

/// The budgets: the RSS caps the engine enforces on itself, and the gate that measures
/// them (spec §8.2, §22.3).
pub mod budget;

/// The reader's profile: the weights, the evidence and the decay that make a reader's
/// taste (spec §12.2).
pub mod profile;

/// The engine's own health: the `/healthz` that returns **503** when the process is over
/// its own RSS cap, because a stub `200` hides the failure the cap exists to catch
/// (spec §8.2).
pub mod health;

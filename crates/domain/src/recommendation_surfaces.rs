//! The closed registry of recommendation surfaces (M64-01).
//!
//! A "surface" is a place that ranks works: `story.similar`, `home.trending`, `library.because_you_shelved`. Before this
//! module each one was a route with an inline list of engines, so the same question — *which algorithms rank this
//! surface, and what happens when one has no data* — had a different answer in every file, and two of them had no answer
//! at all.
//!
//! # Three rules, and why each is a rule
//!
//! * **The ids are closed.** A surface that is not in [`SURFACES`] cannot be configured, because a configured surface
//!   nothing serves is a setting that lies. A route that wants a new surface adds it here, where the next reader of the
//!   config can see it.
//! * **One to four algorithms per surface.** One is legal and is where every surface starts: a surface with one
//!   algorithm cannot have a blend bug. Four is the ceiling because a fifth contribution is worth less than the
//!   bookkeeping, and the decay in `discovery::blend_with` makes that arithmetic rather than taste.
//! * **Every algorithm declares the data it needs.** An algorithm whose table is empty must refuse **by name**, naming
//!   the milestone that would fill it, rather than returning nothing. An empty result is indistinguishable from "no
//!   works match", which is a lie about the catalogue.
//!
//! Nothing here does I/O: [`DataNeed`] is the declaration, and the route that probes it lives in `routes::surfaces`.

use std::collections::BTreeMap;

/// The data an algorithm needs before it can say anything.
///
/// One variant per table-shaped dependency in the tree today. A new dependency is a new variant, which forces the probe
/// in `routes::surfaces` to answer for it — the alternative is a `&str` table name that nothing validates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataNeed {
    /// Nothing but the works table.
    None,
    /// Tag co-occurrence (`work_tags`).
    WorkTags,
    /// Shelves and shelf membership (`shelves`, `shelf_items`).
    Shelves,
    /// Reader ratings (`work_ratings`).
    Ratings,
    /// Arena ballots (`arena_ballots`).
    ArenaBallots,
}

impl DataNeed {
    /// The milestone that populates it, for a refusal that tells an operator what to do next.
    pub fn milestone(self) -> &'static str {
        match self {
            DataNeed::None => "",
            DataNeed::WorkTags => "M64-06",
            DataNeed::Shelves => "M64-11",
            DataNeed::Ratings => "M69-10",
            DataNeed::ArenaBallots => "M69-10",
        }
    }

    /// The table the probe counts, or `None` for a dependency that needs no data.
    pub fn table(self) -> Option<&'static str> {
        match self {
            DataNeed::None => None,
            DataNeed::WorkTags => Some("work_tags"),
            DataNeed::Shelves => Some("shelf_items"),
            DataNeed::Ratings => Some("work_ratings"),
            DataNeed::ArenaBallots => Some("arena_ballots"),
        }
    }
}

/// One ranking surface: its id, its reader-facing label, what its algorithms need, and what ranks it by default.
#[derive(Debug, Clone, Copy)]
pub struct Surface {
    /// The canonical id. Dotted, lowercase, stable — it appears in config and in the operator console.
    pub id: &'static str,
    /// What a reader sees this surface called.
    pub label: &'static str,
    /// The data every algorithm on this surface needs before any of them can run.
    pub need: DataNeed,
    /// The default algorithm set. One to four names, each a strategy this build can run.
    pub defaults: &'static [&'static str],
}

/// The minimum and maximum algorithms a surface may be configured with.
pub const MIN_ALGORITHMS: usize = 1;
pub const MAX_ALGORITHMS: usize = 4;

/// Every surface this build ranks. Closed: see the module docs.
pub const SURFACES: &[Surface] = &[
    // The discover feed's default set is the four engines `build_candidate_engines` actually runs, in the order they
    // contribute: the works ladder, tag affinity, popularity and media-reference collaboration. A default that named an
    // algorithm no engine implements would be documentation of a feed that does not exist.
    Surface { id: "discover.feed", label: "Discover", need: DataNeed::WorkTags, defaults: &["rated_by_readers", "tag_jaccard", "popular", "media_ref_collab"] },
    Surface { id: "story.similar", label: "Similar works", need: DataNeed::WorkTags, defaults: &["tag_jaccard", "tag_cooccurrence", "co_shelving"] },
    Surface { id: "story.also_enjoyed", label: "Also enjoyed", need: DataNeed::Shelves, defaults: &["co_read", "engagement_overlap"] },
    Surface { id: "story.more_by_author", label: "More by this author", need: DataNeed::None, defaults: &["same_author"] },
    Surface { id: "story.lists_containing", label: "Lists containing this", need: DataNeed::Shelves, defaults: &["co_shelving"] },
    Surface { id: "search.results", label: "Search results", need: DataNeed::None, defaults: &["text_relevance"] },
    Surface { id: "search.facets", label: "Search facets", need: DataNeed::None, defaults: &["text_relevance"] },
    Surface { id: "author.works", label: "An author's works", need: DataNeed::None, defaults: &["same_author"] },
    Surface { id: "author.similar", label: "Similar authors", need: DataNeed::WorkTags, defaults: &["author_graph", "tag_jaccard"] },
    Surface { id: "author.colists", label: "Co-listed authors", need: DataNeed::Shelves, defaults: &["co_shelving"] },
    Surface { id: "author.fandoms", label: "An author's fandoms", need: DataNeed::WorkTags, defaults: &["fandom_graph"] },
    Surface { id: "home.trending", label: "Trending", need: DataNeed::None, defaults: &["crosspost_union", "time_decay", "popular"] },
    Surface { id: "home.hidden_gems", label: "Hidden gems", need: DataNeed::WorkTags, defaults: &["tag_jaccard", "quality_signals"] },
    Surface { id: "home.popular_in_fandom", label: "Popular in a fandom", need: DataNeed::WorkTags, defaults: &["popular", "fandom_graph"] },
    Surface { id: "home.recently_updated", label: "Recently updated", need: DataNeed::None, defaults: &["time_decay"] },
    Surface { id: "home.new_this_week", label: "New this week", need: DataNeed::None, defaults: &["time_decay"] },
    Surface { id: "home.curated", label: "Curated", need: DataNeed::None, defaults: &["operator_picks"] },
    Surface { id: "library.because_you_shelved", label: "Because you shelved", need: DataNeed::Shelves, defaults: &["co_shelving", "tag_jaccard"] },
    Surface { id: "library.fill_the_gap", label: "Fill the gap", need: DataNeed::WorkTags, defaults: &["tag_cooccurrence", "fandom_graph"] },
    Surface { id: "library.authors_updated", label: "Authors you follow, updated", need: DataNeed::None, defaults: &["time_decay"] },
    Surface { id: "tag.default_sort", label: "A tag's works", need: DataNeed::None, defaults: &["time_decay", "popular"] },
    Surface { id: "tag.adjacent", label: "Adjacent tags", need: DataNeed::WorkTags, defaults: &["tag_cooccurrence", "npmi"] },
    Surface { id: "tag.cross_fandom", label: "Cross-fandom tags", need: DataNeed::WorkTags, defaults: &["fandom_graph"] },
    Surface { id: "tag.facets", label: "A tag's facets", need: DataNeed::None, defaults: &["text_relevance"] },
];

/// The algorithms one arm of a surface runs (M64-02).
///
/// The split is by **position in the surface's list**, not by hashing the algorithm name: the list is ordered by how much
/// each algorithm contributes, so arm 0 takes the strongest half and arm 1 the rest — a split that reads as a real
/// comparison rather than an arbitrary one. With `buckets <= 1`, or a bucket out of range, every algorithm runs: an
/// instance that is not experimenting, and a reader whose stored arm no longer exists, both get the full surface rather
/// than a silently narrowed feed.
pub fn algorithms_for_bucket(
    surface: &'static Surface,
    configured: &BTreeMap<String, Vec<String>>,
    bucket: i64,
    buckets: i64,
) -> Vec<String> {
    let all = algorithms_for(surface, configured);
    if buckets <= 1 || bucket < 0 || bucket >= buckets {
        return all;
    }
    all.into_iter()
        .enumerate()
        .filter(|(index, _)| (*index as i64) % buckets == bucket)
        .map(|(_, algorithm)| algorithm)
        .collect()
}

/// The engine reason a registered algorithm runs as, or `None` when no engine implements it yet.
///
/// The blend's engines name themselves with short strings (`"tags"`, `"ladder"`), while the registry names algorithms
/// by what they do (`"tag_jaccard"`, `"rated_by_readers"`). This is the one place the two vocabularies are joined, so a
/// configured algorithm either maps to a running engine or is refused at load — it cannot quietly do nothing.
///
/// `None` is a real answer for algorithms the tree has not built — `npmi` is a query, not a blend engine. A surface
/// configured with only unbuilt algorithms serves no engines, which is exactly the refusal the route reports.
/// (`co_shelving` was on that list until M64-11 built it over the reader's own shelves.)
pub fn engine_reason(algorithm: &str) -> Option<&'static str> {
    Some(match algorithm {
        // Two tag algorithms, one engine: the feed's tag engine blends both signals already.
        "tag_jaccard" | "tag_cooccurrence" | "npmi" => "tags",
        "popular" | "time_decay" => "popular",
        // M64-07: the same popularity signal, summed across every archive a story appears on rather than per copy.
        "crosspost_union" => "crosspost_union",
        "rated_by_readers" | "ladder" => "ladder",
        "media_ref_collab" => "media_ref_collab",
        // M64-11: implemented in `db::discovery::co_shelving_recommendations`, over the reader's own shelves only.
        "co_shelving" => "co_shelving",
        // M64-08: the operator's declared fandom adjacency, read before any reader exists — which is the point of declaring it.
        "fandom_graph" => "fandom_graph",
        "strategy" => "strategy",
        _ => return None,
    })
}

/// Look a surface up by its canonical id.
pub fn surface(id: &str) -> Option<&'static Surface> {
    SURFACES.iter().find(|surface| surface.id == id)
}

/// Every algorithm name this registry knows, in sorted order. The closed set a config may choose from.
pub fn known_algorithms() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = SURFACES.iter().flat_map(|s| s.defaults.iter().copied()).collect();
    names.sort_unstable();
    names.dedup();
    names
}

/// The algorithms a surface actually runs: its configured set when one is given, its defaults otherwise.
pub fn algorithms_for(
    surface: &'static Surface,
    configured: &BTreeMap<String, Vec<String>>,
) -> Vec<String> {
    match configured.get(surface.id) {
        Some(names) => names.clone(),
        None => surface.defaults.iter().map(|name| (*name).to_owned()).collect(),
    }
}

/// Refuse a configuration that cannot be honoured, at load rather than at the first request.
///
/// The four refusals are the four ways a per-surface setting goes wrong: a surface nobody serves, a set of the wrong
/// size, an algorithm that does not exist, and a duplicate that would double a work's score for no reason a reader
/// could name.
pub fn validate_configured(configured: &BTreeMap<String, Vec<String>>) -> Result<(), String> {
    let known = known_algorithms();
    for (id, names) in configured {
        let Some(surface) = surface(id) else {
            return Err(format!(
                "`{id}` is not a recommendation surface; the registry is closed ({})",
                SURFACES.iter().map(|s| s.id).collect::<Vec<_>>().join(", ")
            ));
        };
        if names.len() < MIN_ALGORITHMS || names.len() > MAX_ALGORITHMS {
            return Err(format!(
                "surface `{id}` has {} algorithm(s); a surface runs between {MIN_ALGORITHMS} and {MAX_ALGORITHMS}",
                names.len()
            ));
        }
        let mut seen = std::collections::BTreeSet::new();
        for name in names {
            if !known.contains(&name.as_str()) {
                return Err(format!(
                    "surface `{id}` names `{name}`, which is not a known algorithm; known: {}",
                    known.join(", ")
                ));
            }
            if !seen.insert(name) {
                return Err(format!("surface `{id}` names `{name}` twice"));
            }
        }
        let _ = surface;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn configured(names: &[&str]) -> BTreeMap<String, Vec<String>> {
        let mut map = BTreeMap::new();
        map.insert(
            "discover.feed".to_owned(),
            names.iter().map(|name| (*name).to_owned()).collect(),
        );
        map
    }

    #[test]
    fn an_arm_takes_every_nth_algorithm_by_position() {
        // M64-02: the split is by position in the surface's ordered list, so the arms are complements of each other and
        // together cover the surface — which is what makes the comparison fair. Hashing the algorithm name instead would
        // give arms of different sizes and a lopsided result nobody could read.
        let surface = surface("discover.feed").expect("the feed is registered");
        let set = configured(&["popular", "tag_jaccard", "media_ref_collab", "rated_by_readers"]);

        let first = algorithms_for_bucket(surface, &set, 0, 2);
        let second = algorithms_for_bucket(surface, &set, 1, 2);
        assert_eq!(first, vec!["popular", "media_ref_collab"]);
        assert_eq!(second, vec!["tag_jaccard", "rated_by_readers"]);

        let mut together = first;
        together.extend(second);
        assert_eq!(together.len(), 4, "the arms partition the surface");
    }

    #[test]
    fn an_arm_out_of_range_gets_the_whole_surface() {
        // The operator reduced the arm count after assigning this reader. A moved reader is a broken measurement; a full
        // feed is a working page, so the reader gets everything rather than a silently narrowed surface.
        let surface = surface("discover.feed").expect("the feed is registered");
        let set = configured(&["popular", "tag_jaccard"]);
        assert_eq!(
            algorithms_for_bucket(surface, &set, 5, 2),
            vec!["popular", "tag_jaccard"]
        );
        assert_eq!(
            algorithms_for_bucket(surface, &set, 0, 1),
            vec!["popular", "tag_jaccard"],
            "one arm is not a split"
        );
        assert_eq!(
            algorithms_for_bucket(surface, &set, 0, 0),
            vec!["popular", "tag_jaccard"],
            "an instance that is not experimenting runs the whole surface"
        );
    }

    #[test]
    fn every_surface_is_configurable_and_within_bounds() {
        for entry in SURFACES {
            assert!(
                (MIN_ALGORITHMS..=MAX_ALGORITHMS).contains(&entry.defaults.len()),
                "{} has {} defaults",
                entry.id,
                entry.defaults.len()
            );
            // Looked up through the function, not the const: the const is the list, the function is the contract the
            // route and the config use.
            assert_eq!(
                surface(entry.id).map(|found| found.id),
                Some(entry.id),
                "{} is not findable by its own id",
                entry.id
            );
            assert!(!entry.label.is_empty(), "{} has no label", entry.id);
        }
    }

    #[test]
    fn ids_are_unique_and_dotted() {
        let mut seen = std::collections::BTreeSet::new();
        for entry in SURFACES {
            assert!(seen.insert(entry.id), "duplicate surface id {}", entry.id);
            assert!(entry.id.contains('.'), "{} is not a dotted id", entry.id);
        }
    }

    #[test]
    fn a_configuration_naming_an_unknown_surface_is_refused() {
        let mut configured = BTreeMap::new();
        configured.insert("story.smilar".to_owned(), vec!["tag_jaccard".to_owned()]);
        let error = validate_configured(&configured).expect_err("a typo is refused");
        assert!(error.contains("story.smilar"), "{error}");
    }

    #[test]
    fn five_algorithms_are_refused_and_four_are_accepted() {
        let mut configured = BTreeMap::new();
        let four = vec![
            "tag_jaccard".to_owned(),
            "tag_cooccurrence".to_owned(),
            "popular".to_owned(),
            "time_decay".to_owned(),
        ];
        configured.insert("story.similar".to_owned(), four);
        assert!(validate_configured(&configured).is_ok());

        configured.insert(
            "story.similar".to_owned(),
            vec![
                "tag_jaccard".to_owned(),
                "tag_cooccurrence".to_owned(),
                "popular".to_owned(),
                "time_decay".to_owned(),
                "npmi".to_owned(),
            ],
        );
        let error = validate_configured(&configured).expect_err("five is refused");
        assert!(error.contains("between 1 and 4"), "{error}");
    }

    #[test]
    fn an_unknown_algorithm_is_refused_by_name() {
        let mut configured = BTreeMap::new();
        configured.insert("story.similar".to_owned(), vec!["tag_jaccrad".to_owned()]);
        let error = validate_configured(&configured).expect_err("a typo is refused");
        assert!(error.contains("tag_jaccrad"), "{error}");
        assert!(error.contains("known:"), "the error lists what is known: {error}");
    }

    #[test]
    fn a_repeated_algorithm_is_refused() {
        let mut configured = BTreeMap::new();
        configured.insert(
            "tag.adjacent".to_owned(),
            vec!["npmi".to_owned(), "npmi".to_owned()],
        );
        let error = validate_configured(&configured).expect_err("a duplicate is refused");
        assert!(error.contains("twice"), "{error}");
    }

    #[test]
    fn configured_names_win_and_defaults_fill_in() {
        let surface = surface("tag.adjacent").expect("declared");
        let mut configured = BTreeMap::new();
        configured.insert("tag.adjacent".to_owned(), vec!["npmi".to_owned()]);
        assert_eq!(algorithms_for(surface, &configured), vec!["npmi".to_owned()]);
        assert_eq!(
            algorithms_for(surface, &BTreeMap::new()),
            vec!["tag_cooccurrence".to_owned(), "npmi".to_owned()]
        );
    }
}

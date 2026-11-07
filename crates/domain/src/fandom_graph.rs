//! M64-08 — the operator's fandom adjacency map.
//!
//! Some fandoms are one community under two names: *Worm* and *Ward*, *Naruto* and *Boruto*, the MCU films and the 616 comics, the
//! SpaceBattles and SufficientVelocity cultures. A reader who loves one is not a stranger to the other, and no amount of co-occurrence
//! data will say so on a catalogue where those fandoms have never appeared side by side.
//!
//! So the map is **declared, not inferred**, and this module is deliberately small: it holds edges, refuses malformed ones, and answers
//! "what is next to this fandom, and how strongly". Static by nature, and that is the point — it works before any reader exists, it
//! cannot be poisoned by a bot that imports a thousand crossovers, and an operator can explain every edge they published.
//!
//! Three decisions:
//!
//! * **Edges are undirected.** `Worm <-> Ward` is written once. Asking the operator to write it twice invites the state where the pair
//!   relates one way and not the other, which no reader could ever observe but every ranking would.
//! * **A duplicated pair keeps the strongest weight, not the last one.** Two lines for the same pair is an operator correcting
//!   themselves; taking the later weight would make the result depend on line order in a TOML file.
//! * **A self-loop is refused, a zero weight is refused.** `Worm <-> Worm` is a typo that silently doubles a fandom's own works; a zero
//!   weight is an edge that exists in the file and not in any ranking, which is a claim that says nothing.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One declared relationship between two fandoms.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Edge {
    /// One end, by the name the catalogue uses — the fandom node's canonical name.
    pub from: String,
    /// The other end.
    pub to: String,
    /// How strongly they belong together, `0 < weight <= 1`.
    ///
    /// A scalar rather than a word because the ranking needs to multiply by it: `0.9` for the two halves of one serial, `0.4` for two
    /// shows in the same universe that share a cast and little else.
    pub weight: f64,
}

/// The adjacency map, as an operator publishes it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FandomGraph {
    /// The map's identity, so a response can name the version it ranked with. Free-form: a date, a git tag, an editorial note.
    pub version: String,
    /// The declared edges.
    pub edges: Vec<Edge>,
}

impl FandomGraph {
    /// Refuse a map that cannot answer honestly, and return the normalised adjacency when it can.
    ///
    /// Separated from construction because the error text is the point: each refusal names the edge and the reason, since an operator
    /// editing TOML needs the line, not a rule.
    pub fn validate(&self) -> Result<(), String> {
        if self.version.trim().is_empty() {
            return Err(
                "fandom_graph.version is empty: a published map needs an identity so a ranked page can say which one it used"
                    .to_owned(),
            );
        }
        for edge in &self.edges {
            let from = edge.from.trim();
            let to = edge.to.trim();
            if from.is_empty() || to.is_empty() {
                return Err(format!(
                    "fandom_graph: an edge names an empty fandom ({} <-> {})",
                    edge.from, edge.to
                ));
            }
            if from.eq_ignore_ascii_case(to) {
                return Err(format!(
                    "fandom_graph: `{from}` <-> `{to}` is a self-loop, which would double that fandom's own works"
                ));
            }
            if !edge.weight.is_finite() || edge.weight <= 0.0 || edge.weight > 1.0 {
                return Err(format!(
                    "fandom_graph: `{from}` <-> `{to}` has weight {}, which is outside (0, 1]",
                    edge.weight
                ));
            }
        }
        Ok(())
    }

    /// The adjacency, symmetric, with the strongest weight per pair.
    ///
    /// Keyed by the *lowercased* name on both sides because a fandom is matched against catalogue rows by name, and the catalogue does
    /// not promise that an operator typed the capitalisation it stores. The original spelling is kept in [`Neighbour::name`] so a
    /// response can print what the operator wrote.
    fn adjacency(&self) -> BTreeMap<String, BTreeMap<String, (String, f64)>> {
        let mut map: BTreeMap<String, BTreeMap<String, (String, f64)>> = BTreeMap::new();
        for edge in &self.edges {
            let (from, to) = (edge.from.trim(), edge.to.trim());
            for (a, b) in [(from, to), (to, from)] {
                let entry = map
                    .entry(a.to_lowercase())
                    .or_default()
                    .entry(b.to_lowercase())
                    .or_insert_with(|| (b.to_owned(), edge.weight));
                // Keep the strongest, **including its spelling**. Two lines for one pair is a correction, and the correction should not
                // depend on line order — taking only the weight leaves the neighbour printed with the spelling of whichever line happened
                // to come first, which makes the same map render differently depending on how the file was edited.
                if edge.weight > entry.1 {
                    *entry = (b.to_owned(), edge.weight);
                }
            }
        }
        map
    }

    /// What sits next to one fandom, strongest first.
    ///
    /// Empty for an unknown fandom: this map knows nothing about it, which is a different and honest answer from "nothing is related".
    pub fn neighbours(&self, fandom: &str) -> Vec<Neighbour> {
        self.adjacency()
            .remove(&fandom.trim().to_lowercase())
            .map(|entries| {
                let mut out: Vec<Neighbour> = entries
                    .into_values()
                    .map(|(name, weight)| Neighbour { name, weight })
                    .collect();
                out.sort_by(|a, b| {
                    b.weight
                        .partial_cmp(&a.weight)
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then_with(|| a.name.cmp(&b.name))
                });
                out
            })
            .unwrap_or_default()
    }

    /// Every neighbouring fandom a seed set can reach, with the **highest** weight that reaches it.
    ///
    /// `min_weight` filters here rather than in the caller so the threshold has one definition: an operator who says "only strong
    /// adjacencies" gets the same answer wherever the map is read.
    pub fn expand(&self, seeds: &[String], min_weight: f64) -> Vec<Neighbour> {
        let mut best: BTreeMap<String, Neighbour> = BTreeMap::new();
        for seed in seeds {
            for neighbour in self.neighbours(seed) {
                if neighbour.weight < min_weight {
                    continue;
                }
                let key = neighbour.name.to_lowercase();
                best.entry(key)
                    .and_modify(|existing| {
                        if neighbour.weight > existing.weight {
                            *existing = neighbour.clone();
                        }
                    })
                    .or_insert(neighbour);
            }
        }
        // A seed is never its own neighbour: `Worm <-> Ward` must not send a Worm reader back to Worm as "related".
        for seed in seeds {
            best.remove(&seed.trim().to_lowercase());
        }
        let mut out: Vec<Neighbour> = best.into_values().collect();
        out.sort_by(|a, b| {
            b.weight
                .partial_cmp(&a.weight)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.name.cmp(&b.name))
        });
        out
    }

    /// How many pairs the map declares, counting each once.
    pub fn pair_count(&self) -> usize {
        self.adjacency()
            .values()
            .map(BTreeMap::len)
            .sum::<usize>()
            / 2
    }
}

/// One fandom reachable from another, and how strongly the operator says so.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Neighbour {
    /// The fandom's name, as the operator spelled it.
    pub name: String,
    /// The weight of the edge that reaches it.
    pub weight: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph(edges: Vec<(&str, &str, f64)>) -> FandomGraph {
        FandomGraph {
            version: "2026-11-07".to_owned(),
            edges: edges
                .into_iter()
                .map(|(from, to, weight)| Edge {
                    from: from.to_owned(),
                    to: to.to_owned(),
                    weight,
                })
                .collect(),
        }
    }

    #[test]
    fn an_edge_is_undirected_whichever_way_it_was_written() {
        let map = graph(vec![("Worm", "Ward", 0.9)]);
        // Each end reaches the other, however the reader spelled the one they asked about.
        assert_eq!(map.neighbours("Worm")[0].name, "Ward");
        assert_eq!(map.neighbours("ward")[0].name, "Worm");
        assert_eq!(map.neighbours("Worm")[0].weight, map.neighbours("ward")[0].weight);
        assert_eq!(map.pair_count(), 1, "one pair, not two edges");
        // Case and surrounding space are the operator's typing, not part of the identity.
        assert_eq!(map.neighbours("  WORm  ").len(), 1);
    }

    #[test]
    fn a_duplicate_pair_keeps_the_strongest_weight_not_the_last() {
        let map = graph(vec![("Worm", "Ward", 0.9), ("ward", "worm", 0.2)]);
        assert_eq!(map.neighbours("Worm")[0].weight, 0.9, "the strongest wins");
        // Both orders of the same file give the same map.
        let reversed = graph(vec![("ward", "worm", 0.2), ("Worm", "Ward", 0.9)]);
        assert_eq!(map.neighbours("Worm"), reversed.neighbours("Worm"));
    }

    #[test]
    fn malformed_maps_are_refused_with_the_edge_named() {
        assert!(graph(vec![("Worm", "Worm", 0.5)]).validate().is_err());
        let loop_error = graph(vec![("Worm", "worm", 0.5)]).validate().unwrap_err();
        assert!(loop_error.contains("self-loop"), "{loop_error}");

        let zero = graph(vec![("Worm", "Ward", 0.0)]).validate().unwrap_err();
        assert!(zero.contains("outside (0, 1]"), "{zero}");
        let above = graph(vec![("Worm", "Ward", 1.5)]).validate().unwrap_err();
        assert!(above.contains("outside (0, 1]"), "{above}");
        let nan = graph(vec![("Worm", "Ward", f64::NAN)]).validate().unwrap_err();
        assert!(nan.contains("outside (0, 1]"), "{nan}");
        let blank = graph(vec![(" ", "Ward", 0.5)]).validate().unwrap_err();
        assert!(blank.contains("empty fandom"), "{blank}");

        let mut unversioned = graph(vec![("Worm", "Ward", 0.5)]);
        unversioned.version = "  ".to_owned();
        assert!(unversioned.validate().unwrap_err().contains("version"));
    }

    #[test]
    fn expand_never_returns_a_seed_and_keeps_the_strongest_route() {
        // A three-fandom chain: Ward is both 0.9 from Worm and 0.2 from Naruto.
        let map = graph(vec![
            ("Worm", "Ward", 0.9),
            ("Naruto", "Ward", 0.2),
            ("Naruto", "Boruto", 0.8),
        ]);
        let reached = map.expand(&["Worm".to_owned(), "Naruto".to_owned()], 0.0);
        let names: Vec<&str> = reached.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, vec!["Ward", "Boruto"], "strongest first: {names:?}");
        let ward = reached.iter().find(|n| n.name == "Ward").expect("ward");
        assert_eq!(ward.weight, 0.9, "the strongest route to Ward wins");

        // The threshold is applied here, so every reader of the map agrees on what "strong" means.
        let strong_only = map.expand(&["Worm".to_owned(), "Naruto".to_owned()], 0.5);
        let strong: Vec<&str> = strong_only.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(strong, vec!["Ward", "Boruto"]);

        // A seed never comes back as its own relation.
        let from_ward = graph(vec![("Worm", "Ward", 0.9)]).expand(&["Ward".to_owned()], 0.0);
        assert_eq!(from_ward.len(), 1);
        assert_eq!(from_ward[0].name, "Worm");

        // An unknown fandom knows nothing, which is not the same as "nothing is related".
        assert!(map.neighbours("Some Fandom Nobody Mapped").is_empty());
    }
}

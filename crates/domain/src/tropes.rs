//! M45-68 — trope groups: one name a reader can type, expanding to the tags it means.
//!
//! A subgenre is not one tag. "Dark progression fantasy" is a protagonist who is not reformed, a
//! power curve that is the point, and mechanics that make the curve legible — five or six freeform
//! tags whose synonyms keep growing, and which a reader has no way to guess. So the group is defined
//! **once**, here, and `trope:dark-progression` expands to the tags it names.
//!
//! Two rules this table keeps:
//!
//! * **A group is a disjunction**: `trope:x` means *any* of its members, never all of them. Requiring
//!   all would make the group useless on a catalogue where a work carries two of five.
//! * **A group is small on purpose.** The query language refuses more than 24 terms, and a `tag:`
//!   term costs two, so a group with more than about ten members cannot be combined with anything
//!   else. `crates/domain/src/tropes.rs`'s tests hold every group under that ceiling, so adding an
//!   eleventh member is a build failure rather than a query that stops working for one reader.

/// One trope group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TropeGroup {
    /// What a reader types: `trope:dark-progression`.
    pub slug: &'static str,
    /// What it is called when shown.
    pub label: &'static str,
    /// One sentence, for `/taxonomy`, so the expansion is not a secret.
    pub summary: &'static str,
    /// The tags it means. Any of them satisfies the group.
    pub members: &'static [&'static str],
}

/// The groups that ship. Order is the order `/taxonomy` lists them.
pub const TROPES: &[TropeGroup] = &[
    TropeGroup {
        slug: "dark-progression",
        label: "Dark progression",
        summary:
            "A protagonist who is not reformed, and a power curve that is the point of the story.",
        members: &[
            "dark-protagonist",
            "evil-protagonist",
            "villain-protagonist",
            "morally-grey-protagonist",
            "progression",
        ],
    },
    TropeGroup {
        slug: "system-grind",
        label: "System grind",
        summary: "The numbers are the plot: levels, stats, loot and optimisation.",
        members: &["system", "gamer", "litrpg", "stat-grinding"],
    },
    TropeGroup {
        slug: "multiverse-descent",
        label: "Multiverse descent",
        summary: "Crossing worlds is the mechanic, not a change of setting.",
        members: &[
            "multiverse",
            "dimension-travel",
            "world-hopping",
            "dimensional-travel",
        ],
    },
    TropeGroup {
        slug: "necromancy",
        label: "Necromancy",
        summary: "The dead are the resource: souls, armies, and the cost of both.",
        members: &["necromancy", "soul-magic", "undead", "death-magic"],
    },
    TropeGroup {
        slug: "empire-building",
        label: "Empire building",
        summary: "The protagonist rules something, and the ruling is the plot.",
        members: &[
            "empire-building",
            "kingdom-building",
            "dark-lord",
            "conquest",
        ],
    },
];

/// The most tags a group may hold, and why.
///
/// The parser refuses a query over 24 terms, a `tag:` term costs two, and a group has to leave room
/// for the rest of the query — a fandom clause and a sort-worthy filter at least. Ten members is
/// twenty terms, which leaves four: enough for `AND tag:<fandom>` and one more.
pub const MAX_MEMBERS: usize = 10;

/// The group a slug names.
#[must_use]
pub fn find(slug: &str) -> Option<&'static TropeGroup> {
    let slug = slug.trim();
    TROPES.iter().find(|group| group.slug == slug)
}

/// What `trope:<slug>` means: its member tags, or `None` when the name is not a group.
#[must_use]
pub fn expand(slug: &str) -> Option<&'static [&'static str]> {
    find(slug).map(|group| group.members)
}

/// The slugs, for a refusal that tells the reader what it could have written instead.
#[must_use]
pub fn slugs() -> Vec<&'static str> {
    TROPES.iter().map(|group| group.slug).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_group_expands_to_its_members_and_an_unknown_name_names_the_groups() {
        let members = expand("dark-progression").expect("a group");
        assert!(members.contains(&"dark-protagonist"));
        assert!(
            !members.contains(&"fluff"),
            "a group is what it means, not everything nearby"
        );

        assert!(expand("nonsense").is_none());
        let slugs = slugs();
        assert!(slugs.contains(&"system-grind"), "{slugs:?}");
    }

    #[test]
    fn every_group_stays_inside_the_term_budget() {
        for group in TROPES {
            assert!(
                group.members.len() <= MAX_MEMBERS,
                "{} has {} members: two terms each is more than a query can afford beside anything \
                 else",
                group.slug,
                group.members.len()
            );
            assert!(!group.members.is_empty(), "{} names nothing", group.slug);
            assert!(
                !group.summary.trim().is_empty(),
                "{}: /taxonomy would show a blank explanation",
                group.slug
            );
        }
    }

    #[test]
    fn no_group_duplicates_a_member_and_no_slug_repeats() {
        let mut slugs = std::collections::BTreeSet::new();
        for group in TROPES {
            assert!(slugs.insert(group.slug), "{} is defined twice", group.slug);
            let mut seen = std::collections::BTreeSet::new();
            for member in group.members {
                assert!(
                    seen.insert(*member),
                    "{}: {member} is listed twice, which spends two terms to say one thing",
                    group.slug
                );
            }
        }
    }
}

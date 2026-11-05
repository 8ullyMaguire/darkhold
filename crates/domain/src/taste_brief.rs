//! M45-71 — a taste brief: a shelf turned into a document you can review.
//!
//! The point is that a judgement becomes an artefact. A reader who says "like these five" has said
//! something a machine can act on but a person cannot check; the brief turns it into fandoms, the tropes
//! that recur, the tags that appear once, and one query that expresses the whole thing. Committed to a
//! repo or posted to a forge, it can be argued with.
//!
//! **Deterministic on purpose.** No timestamp, no "generated at", no request id: the same shelf produces
//! the same bytes, so two briefs can be diffed and a change in the markdown means a change in the shelf.
//! A document that churns on every read is a document nobody trusts.
//!
//! **Counts, not scores.** `4 of 5` is a fact about the shelf; a weight of 0.78 would be an opinion about
//! it. The brief says what is there, and leaves the judgement to the reader.

use std::collections::BTreeMap;

/// What one work in the shelf contributes.
#[derive(Debug, Clone, Default)]
pub struct SeedFacts {
    /// The work as the reader named it.
    pub title: String,
    /// Fandom nodes the work carries.
    pub fandoms: Vec<String>,
    /// Every other tag-like node (tropes, characters, relationships, warnings, moods).
    pub tags: Vec<String>,
}

/// A tag with the number of works carrying it and how many works there are.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Counted {
    pub name: String,
    pub works: usize,
    pub total: usize,
}

impl Counted {
    /// `4 of 5`, which is what the markdown prints.
    #[must_use]
    pub fn of(&self) -> String {
        format!("{} of {}", self.works, self.total)
    }
}

/// The whole brief.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Brief {
    pub seeds: usize,
    pub fandoms: Vec<Counted>,
    /// Tags on two or more works.
    pub recurring: Vec<Counted>,
    /// Tags on exactly one work — the list to prune.
    pub one_off: Vec<Counted>,
    /// A query that expresses the shelf.
    pub query: String,
    /// The document.
    pub markdown: String,
}

/// How many tags the generated query names. Beyond a handful the `OR` becomes "the catalogue", and a
/// query that matches everything expresses nothing.
pub const QUERY_TAGS: usize = 8;

/// Build a brief from the shelf.
#[must_use]
pub fn build(shelf: &[SeedFacts]) -> Brief {
    let total = shelf.len();
    let mut fandoms: BTreeMap<&str, usize> = BTreeMap::new();
    let mut tags: BTreeMap<&str, usize> = BTreeMap::new();
    for work in shelf {
        // Dedupe inside a work: a work tagged `dark` twice is one work carrying `dark`.
        let mut seen_f: Vec<&str> = work.fandoms.iter().map(String::as_str).collect();
        seen_f.sort_unstable();
        seen_f.dedup();
        for fandom in seen_f {
            *fandoms.entry(fandom).or_insert(0) += 1;
        }
        let mut seen_t: Vec<&str> = work.tags.iter().map(String::as_str).collect();
        seen_t.sort_unstable();
        seen_t.dedup();
        for tag in seen_t {
            *tags.entry(tag).or_insert(0) += 1;
        }
    }

    let counted = |map: BTreeMap<&str, usize>| -> Vec<Counted> {
        let mut out: Vec<Counted> = map
            .into_iter()
            .map(|(name, works)| Counted {
                name: name.to_owned(),
                works,
                total,
            })
            .collect();
        // Most shared first, then alphabetical, so a diff shows a change in the shelf rather than a
        // change in hash iteration order.
        out.sort_by(|a, b| b.works.cmp(&a.works).then_with(|| a.name.cmp(&b.name)));
        out
    };

    let fandoms = counted(fandoms);
    let recurring = counted(
        tags.iter()
            .filter(|(_, n)| **n >= 2)
            .map(|(k, v)| (*k, *v))
            .collect(),
    );
    let one_off = counted(
        tags.iter()
            .filter(|(_, n)| **n == 1)
            .map(|(k, v)| (*k, *v))
            .collect(),
    );

    // The query: the recurring tropes, or the whole vocabulary when nothing recurs. Fandoms narrow it
    // when the shelf agrees on one, because "dark progression in Harry Potter" is a sharper statement
    // than "dark progression anywhere".
    let named: Vec<&str> = if recurring.is_empty() {
        tags.keys().copied().take(QUERY_TAGS).collect()
    } else {
        recurring
            .iter()
            .take(QUERY_TAGS)
            .map(|c| c.name.as_str())
            .collect()
    };
    let mut query = String::new();
    if let Some(top) = fandoms.first() {
        if top.works > 1 {
            query.push_str(&format!("fandom:\"{}\"", top.name));
        }
    }
    if !named.is_empty() {
        if !query.is_empty() {
            query.push_str(" AND ");
        }
        query.push('(');
        for (index, tag) in named.iter().enumerate() {
            if index > 0 {
                query.push_str(" OR ");
            }
            query.push_str(&format!("tag:\"{tag}\""));
        }
        query.push(')');
    }

    let markdown = render(total, &fandoms, &recurring, &one_off, &query);
    Brief {
        seeds: total,
        fandoms,
        recurring,
        one_off,
        query,
        markdown,
    }
}

/// The document. Plain markdown, no HTML, no tables: it must read correctly in a terminal, in a forge
/// comment and in a note-taking app without any of them agreeing on anything.
fn render(
    total: usize,
    fandoms: &[Counted],
    recurring: &[Counted],
    one_off: &[Counted],
    query: &str,
) -> String {
    let mut out = String::new();
    out.push_str("# Taste brief\n\n");
    out.push_str(&format!(
        "{} work{} named. Fandoms below, then the tags that recur, then the ones that appear once.\n",
        total,
        if total == 1 { "" } else { "s" }
    ));
    out.push_str("\nCounts, not scores: `4 of 5` is how many of the works you named carry it.\n");

    out.push_str("\n## Fandoms\n\n");
    if fandoms.is_empty() {
        out.push_str("_None of these works carries a fandom tag._\n");
    } else {
        for fandom in fandoms {
            out.push_str(&format!("- {} — {}\n", fandom.name, fandom.of()));
        }
    }

    out.push_str("\n## Recurring\n\n");
    if recurring.is_empty() {
        out.push_str(
            "_Nothing recurs: these works share no two tags, which is itself worth knowing._\n",
        );
    } else {
        for tag in recurring {
            out.push_str(&format!("- {} — {}\n", tag.name, tag.of()));
        }
    }

    out.push_str("\n## Appears once\n\n");
    if one_off.is_empty() {
        out.push_str("_Every tag here is shared by at least two works._\n");
    } else {
        for tag in one_off {
            out.push_str(&format!("- {}\n", tag.name));
        }
    }

    out.push_str("\n## The query that expresses this\n\n```\n");
    if query.is_empty() {
        out.push_str("(nothing — the shelf carries no tags to query)\n");
    } else {
        out.push_str(query);
        out.push('\n');
    }
    out.push_str("```\n\n");
    out.push_str("Run it: `dh q '<the query above>'`\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn work(title: &str, fandoms: &[&str], tags: &[&str]) -> SeedFacts {
        SeedFacts {
            title: title.to_owned(),
            fandoms: fandoms.iter().map(|f| (*f).to_owned()).collect(),
            tags: tags.iter().map(|t| (*t).to_owned()).collect(),
        }
    }

    #[test]
    fn recurrence_is_counted_in_works_and_a_duplicate_tag_counts_once() {
        let brief = build(&[
            work("one", &["Harry Potter"], &["dark", "dark", "system"]),
            work("two", &["Harry Potter"], &["dark", "necromancy"]),
        ]);
        assert_eq!(brief.seeds, 2);
        let dark = brief
            .recurring
            .iter()
            .find(|t| t.name == "dark")
            .expect("dark recurs");
        assert_eq!((dark.works, dark.total), (2, 2));
        assert_eq!(dark.of(), "2 of 2");
        assert!(
            brief.one_off.iter().any(|t| t.name == "system"),
            "a tag on one work is a one-off: {:?}",
            brief.one_off
        );
        // A work tagging itself twice must not make `dark` look like it recurred more than it did.
        assert!(!brief.recurring.iter().any(|t| t.works > 2));
    }

    #[test]
    fn the_query_names_the_recurring_tags_and_a_fandom_only_when_the_shelf_agrees() {
        let brief = build(&[
            work("one", &["Harry Potter"], &["dark", "system"]),
            work("two", &["Harry Potter"], &["dark", "necromancy"]),
        ]);
        assert_eq!(
            brief.query, "fandom:\"Harry Potter\" AND (tag:\"dark\")",
            "only `dark` recurs, and the fandom is shared"
        );

        let split = build(&[
            work("one", &["Harry Potter"], &["dark"]),
            work("two", &["Naruto"], &["dark"]),
        ]);
        assert_eq!(
            split.query, "(tag:\"dark\")",
            "a shelf that disagrees on fandom must not pretend it agrees"
        );
    }

    #[test]
    fn the_same_shelf_produces_the_same_bytes() {
        let shelf = [
            work("b", &["X"], &["one", "two"]),
            work("a", &["X"], &["two", "three"]),
        ];
        let first = build(&shelf).markdown;
        let second = build(&shelf).markdown;
        assert_eq!(first, second, "a document that churns cannot be committed");
        assert!(!first.contains("generated"), "no timestamp: {:?}", first);
    }

    #[test]
    fn an_untagged_shelf_says_so_instead_of_printing_an_empty_document() {
        let brief = build(&[work("bare", &[], &[]), work("also bare", &[], &[])]);
        assert!(brief.markdown.contains("_Nothing recurs"));
        assert!(brief
            .markdown
            .contains("(nothing — the shelf carries no tags to query)"));
        assert_eq!(brief.query, "");
    }

    #[test]
    fn the_query_is_capped_because_a_query_that_matches_everything_expresses_nothing() {
        let tags: Vec<String> = (0..12).map(|i| format!("tag{i:02}")).collect();
        let refs: Vec<&str> = tags.iter().map(String::as_str).collect();
        let brief = build(&[work("one", &[], &refs), work("two", &[], &refs)]);
        assert_eq!(brief.recurring.len(), 12);
        let named = brief.query.matches("tag:\"").count();
        assert_eq!(named, QUERY_TAGS, "at most {QUERY_TAGS} names, got {named}");
    }
}

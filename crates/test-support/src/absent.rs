//! The "absent, not empty" contract, as an assertion a page test can call (M71-05, spec §8.11–§8.14).
//!
//! The rule the product states is: **a section that has nothing to say must not be rendered at all.** A heading over an
//! empty list is worse than no heading — it tells the reader there is something here, then shows them the inside of a
//! box. The same rule covers the degradations: a section that cannot be filled must say *why*, in a named, testable
//! string, rather than render blank.
//!
//! Both are assertions about HTML the server produced, so they live here and every document test calls them, instead of
//! each test inventing a substring check that passes on `<h2>Tags</h2><ul></ul>`.
//!
//! # What counts as content
//!
//! After a heading, and before the next heading or the end of the enclosing section/main, there must be at least one
//! character that is not markup. An empty `<ul>`, an empty `<p>`, whitespace, or a comment alone is not content. An
//! element that itself contains text is.

/// One heading, with the byte range of the content that follows it up to the next heading or the end of its section.
struct Heading<'a> {
    level: u8,
    text: &'a str,
    content: &'a str,
}

/// Split `html` into headings and the content that follows each one.
///
/// Deliberately a scanner rather than a parser: the pages this checks are produced by one builder (`crate::html`), so
/// the shapes are known, and a full HTML parser dependency in test-support would be a heavier claim than the check
/// makes. Tags are matched case-insensitively for the four headings that matter.
fn headings(html: &str) -> Vec<Heading<'_>> {
    let mut out = Vec::new();
    let lower = html.to_ascii_lowercase();
    let mut cursor = 0usize;

    while let Some(offset) = lower[cursor..].find("<h") {
        let start = cursor + offset;
        let level = match lower.as_bytes().get(start + 2) {
            Some(b'1'..=b'4') => lower.as_bytes()[start + 2] - b'0',
            // `<hr>`, `<html>`, `<header>` and anything else that starts with "h": not a heading.
            _ => {
                cursor = start + 2;
                continue;
            }
        };
        let Some(open_end) = lower[start..].find('>') else { break };
        let text_start = start + open_end + 1;
        let close = format!("</h{level}>");
        let Some(text_end_rel) = lower[text_start..].find(&close) else {
            cursor = text_start;
            continue;
        };
        let text_end = text_start + text_end_rel;
        let after = text_end + close.len();

        // Content runs to the next heading or to the close of the block that contains this one. Both bounds matter:
        // a list followed by another heading must be judged on its own, and a heading at the end of a section must not
        // borrow the next section's content.
        let next_heading = lower[after..]
            .find("<h")
            .map(|i| after + i)
            .filter(|end| {
                matches!(
                    lower.as_bytes().get(end + 2),
                    Some(b'1'..=b'4')
                )
            })
            .unwrap_or(html.len());
        let block_end = ["</section>", "</main>", "</article>", "</body>"]
            .iter()
            .filter_map(|close| lower[after..].find(close).map(|i| after + i))
            .min()
            .unwrap_or(html.len());
        let content_end = next_heading.min(block_end).max(after);

        out.push(Heading {
            level,
            text: &html[text_start..text_end],
            content: &html[after..content_end],
        });
        cursor = after;
    }
    out
}

/// Whether a fragment holds anything a reader can read.
///
/// Markup does not count. Text does. An `<img alt="…">` counts, because the alt text is what a reader gets.
fn has_content(fragment: &str) -> bool {
    if fragment.contains("alt=\"") {
        return true;
    }
    let mut rest = fragment;
    loop {
        let Some(open) = rest.find('<') else {
            if rest.trim().is_empty() {
                return false;
            }
            return true;
        };
        if !rest[..open].trim().is_empty() {
            return true;
        }
        // Skip the tag itself; comments are skipped whole.
        if rest[open..].starts_with("<!--") {
            match rest[open..].find("-->") {
                Some(end) => {
                    rest = &rest[open + end + 3..];
                    continue;
                }
                None => return false,
            }
        }
        match rest[open..].find('>') {
            Some(end) => rest = &rest[open + end + 1..],
            None => return false,
        }
    }
}

/// Panics when a heading in `html` has nothing under it.
///
/// Call it in every test that fetches a document. It takes the page's own HTML so the failure message can name the
/// heading and the surface, which is what makes a red test diagnosable without a debugger.
pub fn assert_no_empty_section(html: &str, surface: &str) {
    let found = headings(html);
    let empty: Vec<&Heading<'_>> = found.iter().filter(|h| !has_content(h.content)).collect();
    assert!(
        empty.is_empty(),
        "{surface}: {} heading(s) with nothing under them — absent, not empty (§8.11): {}",
        empty.len(),
        empty
            .iter()
            .map(|h| format!("h{} `{}`", h.level, h.text.trim()))
            .collect::<Vec<_>>()
            .join(", ")
    );
}

/// Panics unless `html` contains the named degradation `word` in the section that degrades.
///
/// A degradation that is not named is indistinguishable from a bug: "no recommendations" and "the recommender is off"
/// look the same to a reader unless the page says which. The word is a closed vocabulary — see the M71-05 row — so a
/// test can assert it exactly.
pub fn assert_names_degradation(html: &str, word: &str, surface: &str) {
    assert!(
        html.contains(word),
        "{surface}: a degraded section must name what is missing (`{word}`), not render blank (§8.12)"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_heading_over_an_empty_list_is_caught() {
        let html = "<section><h2>Tags</h2><ul></ul></section>";
        let panic = std::panic::catch_unwind(|| assert_no_empty_section(html, "test"));
        assert!(panic.is_err(), "an empty section must fail the check");
    }

    #[test]
    fn a_heading_over_text_passes() {
        assert_no_empty_section("<section><h2>Tags</h2><ul><li>slow burn</li></ul></section>", "test");
    }

    #[test]
    fn a_heading_followed_by_another_heading_is_caught_even_when_later_text_exists() {
        let html = "<section><h2>Warnings</h2><h2>Tags</h2><ul><li>slow burn</li></ul></section>";
        let panic = std::panic::catch_unwind(|| assert_no_empty_section(html, "test"));
        assert!(
            panic.is_err(),
            "a heading must be judged on its own content, not the next section's"
        );
    }

    #[test]
    fn an_image_with_alt_text_is_content() {
        assert_no_empty_section("<h2>Cover</h2><img alt=\"a cover\">", "test");
    }

    #[test]
    fn a_comment_alone_is_not_content() {
        let panic = std::panic::catch_unwind(|| {
            assert_no_empty_section("<h2>Tags</h2><!-- generated -->", "test")
        });
        assert!(panic.is_err());
    }

    #[test]
    fn hr_and_header_are_not_headings() {
        assert_no_empty_section("<header><p>Darkhold</p></header><hr><main><p>x</p></main>", "test");
    }
}

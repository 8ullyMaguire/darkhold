//! M63-14 — the feed document: three shapes, one item list, and a tag that makes a poll cheap.

use darkhold_domain::feed::{etag, matches, render, Feed, FeedItem, Format};

fn feed() -> Feed {
    Feed {
        title: "grimdark & long".to_owned(),
        self_url: "https://darkhold.example/api/v1/saved-views/v1/feed?format=atom".to_owned(),
        link: "https://darkhold.example/library".to_owned(),
        items: vec![
            FeedItem {
                id: "11111111-1111-1111-1111-111111111111".to_owned(),
                title: "Tom & Jerry <sic>".to_owned(),
                link: "https://example.invalid/w/1".to_owned(),
                updated: Some("2026-11-05T06:00:00Z".to_owned()),
                summary: "a \"summary\" with 'quotes'".to_owned(),
                author: "Shiro".to_owned(),
            },
            FeedItem {
                id: "22222222-2222-2222-2222-222222222222".to_owned(),
                title: "No date".to_owned(),
                link: "https://example.invalid/w/2".to_owned(),
                updated: None,
                summary: String::new(),
                author: String::new(),
            },
        ],
    }
}

#[test]
fn every_shape_carries_every_item_and_escapes_what_would_break_it() {
    let atom = render(&feed(), Format::Atom);
    let rss = render(&feed(), Format::Rss);
    let json = render(&feed(), Format::JsonFeed);

    for document in [&atom, &rss] {
        assert!(
            document.contains("Tom &amp; Jerry"),
            "escaped ampersand: {document}"
        );
        assert!(
            !document.contains("<sic>"),
            "an unescaped tag would parse as markup"
        );
        assert!(
            document.contains("11111111-1111-1111-1111-111111111111"),
            "the id is the guid"
        );
    }
    // JSON needs no XML escaping: the title survives verbatim, which is the point of the third shape.
    assert!(json.contains("Tom & Jerry <sic>"), "{json}");
    assert!(json.contains("11111111-1111-1111-1111-111111111111"));
    assert!(
        rss.contains("<guid isPermaLink=\"false\">"),
        "the id is not a permalink"
    );
    assert!(atom.contains("<feed xmlns=\"http://www.w3.org/2005/Atom\">"));
    assert!(json.contains("\"version\": \"https://jsonfeed.org/version/1.1\""));
}

#[test]
fn an_item_with_no_date_prints_no_date_rather_than_today() {
    let rss = render(&feed(), Format::Rss);
    // One `pubDate` for the dated item, none for the undated one: a feed that stamps "now" on an item
    // reorders itself on every poll.
    assert_eq!(rss.matches("<pubDate>").count(), 1, "{rss}");
    let atom = render(&feed(), Format::Atom);
    // The feed's own `updated` falls back to the epoch only when NOTHING is dated.
    assert!(
        atom.contains("<updated>2026-11-05T06:00:00Z</updated>"),
        "{atom}"
    );
}

#[test]
fn the_tag_changes_when_the_feed_changes_and_not_before() {
    let first = render(&feed(), Format::Atom);
    let tag = etag(&first);
    assert_eq!(
        tag,
        etag(&render(&feed(), Format::Atom)),
        "same document, same tag"
    );
    assert!(
        tag.starts_with('"') && tag.ends_with('"'),
        "quoted for If-None-Match: {tag}"
    );

    let mut changed = feed();
    changed.items[0].title = "Tom & Jerry <sic> (revised)".to_owned();
    assert_ne!(
        tag,
        etag(&render(&changed, Format::Atom)),
        "a changed title must change the tag"
    );
    // A different shape is a different document, so it gets its own tag.
    assert_ne!(tag, etag(&render(&feed(), Format::Rss)));
}

#[test]
fn if_none_match_accepts_a_list_a_weak_tag_and_a_star() {
    let tag = "\"abc123\"";
    assert!(matches("\"abc123\"", tag));
    assert!(matches("\"zzz\", \"abc123\"", tag), "a list names ours");
    assert!(
        matches("W/\"abc123\"", tag),
        "a weak tag still means the reader has the bytes"
    );
    assert!(matches("*", tag), "a star matches anything");
    assert!(!matches("\"zzz\"", tag));
    assert!(!matches("", tag));
}

#[test]
fn the_format_is_parsed_from_the_words_a_client_sends() {
    assert_eq!(Format::parse("atom"), Some(Format::Atom));
    assert_eq!(Format::parse(" RSS "), Some(Format::Rss));
    assert_eq!(Format::parse("jsonfeed"), Some(Format::JsonFeed));
    assert_eq!(Format::parse("json-feed"), Some(Format::JsonFeed));
    assert_eq!(Format::parse("yaml"), None);
    assert_eq!(
        Format::Atom.content_type(),
        "application/atom+xml; charset=utf-8"
    );
    assert_eq!(
        Format::Rss.content_type(),
        "application/rss+xml; charset=utf-8"
    );
    assert_eq!(
        Format::JsonFeed.content_type(),
        "application/feed+json; charset=utf-8"
    );
}

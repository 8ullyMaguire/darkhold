//! M74-01 — Pawchive's public recommendation HTML and chapter-title normalisation.

#[test]
fn recommended_page_links_are_deduped() {
    let html = r#"
      <a href="/patreon/user/5916248"><img src="/icons/patreon/5916248">Patreon Ankit</a>
      <a href="/patreon/user/5916248">Ankit again</a>
      <a href="https://pawchive.pw/patreon/user/3728453">Slayer Anderson</a>
      <a href="/patreon/user/not-a-number">malformed</a>
      <a href="/posts/123">not an author</a>
    "#;
    assert_eq!(
        darkhold_scrapers::sites::pawchive::similar_creator_urls(html),
        vec![
            "https://pawchive.pw/patreon/user/5916248".to_owned(),
            "https://pawchive.pw/patreon/user/3728453".to_owned(),
        ]
    );
}

#[test]
fn story_titles_collapse_chapter_numbers_but_keep_years() {
    use darkhold_scrapers::sites::pawchive::{story_key, story_title};
    assert_eq!(
        story_title("Sailing the Skies with my Endergirl 10"),
        "Sailing the Skies with my Endergirl"
    );
    assert_eq!(
        story_title("Sailing the Skies with my Endergirl - Chapter 11"),
        "Sailing the Skies with my Endergirl"
    );
    assert_eq!(story_title("Gachapunk 2076"), "Gachapunk 2076");
    assert_eq!(
        story_key("26677387", "Sailing the Skies with my Endergirl 10"),
        "pawchive:26677387:sailing-the-skies-with-my-endergirl"
    );
    assert_eq!(
        story_key("26677387", "Sailing the Skies with my Endergirl 11"),
        story_key("26677387", "Sailing the Skies with my Endergirl 10"),
        "two chapters of one story share one work key"
    );
}

#[test]
fn chapter_posts_share_one_story_url() {
    use darkhold_scrapers::sites::pawchive::{distinct_story_slugs, story_url};
    let titles = [
        "Sailing the Skies with my Endergirl 10",
        "Sailing the Skies with my Endergirl - Chapter 11",
        "Weekly Voting: Nov 15, 2026",
        "Gachapunk 2076",
    ];
    assert_eq!(
        distinct_story_slugs(&titles),
        vec![
            polaris_slug::slug(
                "Sailing the Skies with my Endergirl",
                polaris_slug::Policy::NONE
            ),
            "weekly-voting-nov-15-2026".to_owned(),
            "gachapunk-2076".to_owned(),
        ]
    );
    assert_eq!(
        story_url("26677387", "Sailing the Skies with my Endergirl 11"),
        story_url("26677387", "Sailing the Skies with my Endergirl 10")
    );
}

#[test]
fn the_bundled_catalogue_declares_the_pawchive_frontier() {
    use darkhold_scrapers::{Catalogue, Discovery};
    let catalogue = Catalogue::bundled().expect("the bundled catalogue parses");
    let entry = catalogue
        .get("pawchive")
        .expect("Pawchive is a declared source");
    assert_eq!(entry.id, "pawchive");
    assert!(matches!(entry.discovery, Discovery::AuthorFrontier { .. }));
    assert!(entry.is_syncable());
}

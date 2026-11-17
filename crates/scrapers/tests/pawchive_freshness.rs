//! M74-01 — Pawchive's dated discovery method keeps a story's newest post date.

use darkhold_scrapers::safety::FixtureFetcher;
use darkhold_scrapers::sites::pawchive::Pawchive;
use darkhold_scrapers::SourceAdapter;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use url::Url;

#[tokio::test]
async fn dated_author_works_collapse_chapters_and_keep_the_newest_date() {
    let body = r#"[
      {"id":"1","title":"Sailing the Skies with my Endergirl 10","content":"<p>A long enough chapter body for the listing filter to admit it.</p>" ,"user":"26677387","published":"2026-01-01T00:00:00Z","tags":[]},
      {"id":"2","title":"Sailing the Skies with my Endergirl - Chapter 11","content":"<p>Another long enough chapter body for the very same story.</p>" ,"user":"26677387","published":"2026-05-01T00:00:00Z","tags":[]},
      {"id":"3","title":"Gachapunk 2076","content":"<p>A different story entirely, with its own independent publication date.</p>" ,"user":"26677387","published":"2026-06-01T00:00:00Z","tags":[]}
    ]"#;
    let fetch = FixtureFetcher::new().with_page(
        "https://pawchive.pw/api/v1/patreon/user/26677387/posts?limit=50&o=0",
        body,
    );
    let profile = Url::parse("https://pawchive.pw/patreon/user/26677387").unwrap();

    let works = Pawchive::new()
        .list_author_works_dated(&fetch, &profile, None)
        .await
        .expect("dated author works");

    assert_eq!(works.len(), 2, "two distinct stories, not three chapters");
    let sailing = works
        .iter()
        .find(|(url, _)| url.contains("sailing-the-skies-with-my-endergirl"))
        .expect("sailing story");
    assert_eq!(
        sailing.1,
        Some(OffsetDateTime::parse("2026-05-01T00:00:00Z", &Rfc3339).unwrap()),
        "the newest chapter date represents the story"
    );
    let gacha = works
        .iter()
        .find(|(url, _)| url.contains("gachapunk-2076"))
        .expect("other story");
    assert_eq!(
        gacha.1,
        Some(OffsetDateTime::parse("2026-06-01T00:00:00Z", &Rfc3339).unwrap())
    );
}

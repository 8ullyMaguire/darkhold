//! Pawchive — a Patreon fanfiction archive at <https://pawchive.pw>.
//!
//! # Recognised URLs
//!
//! ```text
//! https://pawchive.pw/patreon/user/{user_id}
//! ```
//!
//! # Capabilities
//!
//! Only `bibliography` (listing an author's posts) and `chapters` (reading
//! chapter text when the author pasted it) are available. Most Patreon
//! cross-posts on Pawchive are links-only (Deckreader, Google Docs); those
//! posts are skipped during a full import — they report `content_length: 0`
//! and no chapter text. The adapter cannot log into Patreon, so it reads
//! only what Pawchive's public JSON API returns.
//!
//! The adapter uses two endpoints:
//! * `GET /api/v1/patreon/user/{uid}/posts?limit=50&o={offset}` — list posts
//! * `GET /api/v1/patreon/user/{uid}/post/{pid}` — one post with navigation

use async_trait::async_trait;
use scraper::{Html, Selector};
use url::Url;

use crate::{
    AuthKind, Credentials, Fetcher, SourceAdapter, SourceCapabilities, SourceError, SourceKey,
    SourceResult, SourceUnit, SourceWork, WorkStatus,
};

/// Hosts this adapter serves.
const HOSTS: [&str; 2] = ["pawchive.pw", "www.pawchive.pw"];

/// Every similar-creator link on a `/patreon/user/{id}/recommended` page.
///
/// The live page on 2026-11-17 renders anchors whose href is a relative `/patreon/user/{id}`.
/// Absolute links from the same host are accepted too. Malformed ids are dropped rather than
/// queued: a walk that followed `/patreon/user/not-a-number` would fail at the post API and report a
/// broken source when the page was fine.
#[must_use]
pub fn similar_creator_urls(html: &str) -> Vec<String> {
    let document = Html::parse_fragment(html);
    let Ok(selector) = Selector::parse("a[href]") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for anchor in document.select(&selector) {
        let Some(href) = anchor.value().attr("href") else {
            continue;
        };
        let path = href
            .strip_prefix("https://pawchive.pw")
            .or_else(|| href.strip_prefix("http://pawchive.pw"))
            .or_else(|| href.strip_prefix("https://www.pawchive.pw"))
            .unwrap_or(href);
        let Some(id) = path.strip_prefix("/patreon/user/") else {
            continue;
        };
        let id = id.trim_end_matches('/');
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let url = format!("https://pawchive.pw/patreon/user/{id}");
        if !out.contains(&url) {
            out.push(url);
        }
    }
    out
}

/// Collapse a Pawchive post title to the story it belongs to.
///
/// Witnessed live: "Sailing the Skies with my Endergirl 10" and "... 11" are chapters of one story.
/// A four-digit bare number is part of a name ("Gachapunk 2076") and is kept; an explicit
/// "Chapter 2076" is still a chapter and is removed.
#[must_use]
pub fn story_title(post_title: &str) -> String {
    let title = post_title.trim();
    let lower = title.to_lowercase();
    if let Some(position) = lower.rfind("chapter ") {
        let suffix = title[position + "chapter ".len()..].trim();
        let number = suffix.trim_matches(|c: char| !c.is_ascii_digit());
        if !number.is_empty() {
            let before = title[..position].trim_end_matches(|c: char| "-–—: ".contains(c));
            if !before.trim().is_empty() {
                return before.trim().to_owned();
            }
        }
    }
    let digits = title
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit())
        .count();
    if digits == 0 || digits > 3 {
        return title.to_owned();
    }
    let before = title[..title.len() - digits].trim_end_matches(|c: char| "-–—: ".contains(c));
    if before.trim().is_empty() {
        title.to_owned()
    } else {
        before.trim().to_owned()
    }
}

/// The natural key of one story at one author: `pawchive:{author_id}:{slug}`.
#[must_use]
pub fn story_key(author_id: &str, post_title: &str) -> String {
    let slug = polaris_slug::slug(&story_title(post_title), polaris_slug::Policy::NONE);
    format!("pawchive:{author_id}:{slug}")
}

/// The stable slug of the story a post belongs to.
#[must_use]
pub fn story_slug(post_title: &str) -> String {
    polaris_slug::slug(&story_title(post_title), polaris_slug::Policy::NONE)
}

/// One canonical import URL per story, independent of the post that revealed it.
#[must_use]
pub fn story_url(author_id: &str, post_title: &str) -> String {
    format!(
        "https://pawchive.pw/patreon/user/{author_id}?story={}",
        story_slug(post_title)
    )
}

/// Distinct story slugs in first-seen order. Chapter posts collapse to one slug.
#[must_use]
pub fn distinct_story_slugs(titles: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    for title in titles {
        let slug = story_slug(title);
        if !slug.is_empty() && !out.contains(&slug) {
            out.push(slug);
        }
    }
    out
}

/// The Pawchive adapter.
#[derive(Debug, Clone)]
pub struct Pawchive {
    key: SourceKey,
    hosts: Vec<String>,
}

impl Default for Pawchive {
    fn default() -> Self {
        Self::new()
    }
}

impl Pawchive {
    /// The adapter and its hosts.
    #[must_use]
    pub fn new() -> Self {
        Self {
            key: SourceKey::new("pawchive"),
            hosts: HOSTS.iter().map(|h| (*h).to_owned()).collect(),
        }
    }

    /// The hosts, for the fetcher's allow-list.
    #[must_use]
    pub fn hosts(&self) -> Vec<String> {
        self.hosts.clone()
    }

    /// Whether a URL is one of this adapter's hosts.
    fn host_matches(&self, host: &str) -> bool {
        let host = host.trim_start_matches("www.");
        self.hosts.iter().any(|ours| host == ours)
    }

    /// The numeric user id in a `/patreon/user/{id}` path.
    fn user_id(url: &Url) -> Option<String> {
        let mut segments = url.path_segments()?;
        if segments.next()? != "patreon" {
            return None;
        }
        if segments.next()? != "user" {
            return None;
        }
        let id = segments.next()?;
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        Some(id.to_owned())
    }

    /// Build the JSON API URL for a user's posts page.
    fn api_user_posts_url(uid: &str, offset: u32) -> String {
        format!("https://pawchive.pw/api/v1/patreon/user/{uid}/posts?limit=50&o={offset}")
    }
}

/// One post from Pawchive's JSON API.
#[derive(Debug, serde::Deserialize)]
#[allow(dead_code)]
struct ApiPost {
    id: String,
    user: String,
    title: String,
    content: String,
    /// Pawchive serialises the tag set as a *string* shaped like a set
    /// literal: `"{BIS,Commissioned,battletech}"` or
    /// `'{"Free Write","No Sex",Marvel}'`. A real JSON array and `null` also
    /// occur. All shapes are accepted and split into individual tags.
    #[serde(default, deserialize_with = "deserialize_tags")]
    tags: Vec<String>,
    #[serde(default)]
    published: Option<String>,
    #[serde(default)]
    attachments: Option<Vec<serde_json::Value>>,
}

/// Split Pawchive's set-literal tag string into individual tags.
///
/// Pawchive emits a mixed set literal: `{"Free Write","No Sex",Marvel}` — some
/// entries quoted (those containing a comma or space), others bare. So the
/// body is walked token by token: a quote starts a tag that runs to the next
/// quote, and otherwise a tag runs to the next comma or closing brace. Only
/// when the body has no quotes at all is a plain comma split used.
///
/// The previous behaviour — stripping braces and keeping the whole string —
/// turned `{BIS,Commissioned,battletech,warhammer40k}` into one nonsense tag.
fn split_tag_string(raw: &str) -> Vec<String> {
    let body = raw
        .trim()
        .trim_start_matches(['{', '['])
        .trim_end_matches(['}', ']']);

    let mut parts: Vec<String> = Vec::new();
    if body.contains('"') {
        let mut current = String::new();
        let mut in_quotes = false;
        for ch in body.chars() {
            match ch {
                '"' => {
                    if in_quotes {
                        parts.push(std::mem::take(&mut current));
                    }
                    in_quotes = !in_quotes;
                }
                ',' if !in_quotes => {
                    let trimmed = current.trim();
                    if !trimmed.is_empty() {
                        parts.push(trimmed.to_owned());
                    }
                    current.clear();
                }
                _ => current.push(ch),
            }
        }
        let trimmed = current.trim();
        if !trimmed.is_empty() {
            parts.push(trimmed.to_owned());
        }
    } else if body.contains(',') {
        parts = body.split(',').map(ToOwned::to_owned).collect();
    } else {
        parts.push(body.to_owned());
    }

    let mut out: Vec<String> = Vec::with_capacity(parts.len());
    for part in parts {
        let tag = part.trim().trim_matches('"').trim_matches('\'').trim();
        if tag.is_empty() || tag.len() > 80 {
            continue;
        }
        if !out
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(tag))
        {
            out.push(tag.to_owned());
        }
    }
    out
}

/// Accept `["a", "b"]`, `"{a,b}"` (set-literal form), `null` and absence.
fn deserialize_tags<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize as _;
    use serde_json::Value;

    let value = Value::deserialize(deserializer)?;
    match value {
        Value::Null => Ok(Vec::new()),
        Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    Value::String(s) => out.extend(split_tag_string(&s)),
                    other => {
                        use serde::de::Error as _;
                        return Err(D::Error::custom(format!(
                            "a tag list entry must be a string, got {}",
                            Value::to_string(&other)
                        )));
                    }
                }
            }
            Ok(out)
        }
        Value::String(s) => Ok(split_tag_string(&s)),
        other => {
            use serde::de::Error as _;
            Err(D::Error::custom(format!(
                "tags must be an array or a string, got {}",
                Value::to_string(&other)
            )))
        }
    }
}

/// The JSON list response is just an array of posts.
type ApiPostList = Vec<ApiPost>;

/// Every post for one author, paginated. The natural story identity lives in the post titles,
/// so a story cannot be found from the first page alone.
async fn all_posts(fetch: &dyn Fetcher, uid: &str) -> SourceResult<ApiPostList> {
    let mut posts = Vec::new();
    let mut offset = 0u32;
    loop {
        let page = fetch
            .get(&Pawchive::api_user_posts_url(uid, offset))
            .await
            .map_err(|error| SourceError::Network(format!("listing posts: {error}")))?;
        let page_posts: ApiPostList = serde_json::from_str(&page.body).map_err(|error| {
            SourceError::Parse(format!("Pawchive API returned invalid JSON: {error}"))
        })?;
        let exhausted = page_posts.len() < 50;
        posts.extend(page_posts);
        if exhausted {
            break;
        }
        offset += 50;
    }
    Ok(posts)
}

/// The `?story=` slug on a canonical story URL, when one is present.
fn requested_story(url: &Url) -> Option<String> {
    url.query_pairs()
        .find(|(key, _)| key == "story")
        .map(|(_, value)| value.into_owned())
}

/// `/api/v1/patreon/user/{uid}/profile` — the author's display name.
#[derive(Debug, serde::Deserialize)]
struct ApiProfile {
    name: String,
}

#[async_trait]
impl SourceAdapter for Pawchive {
    fn key(&self) -> SourceKey {
        self.key.clone()
    }

    fn display_name(&self) -> &'static str {
        "Pawchive"
    }

    fn capabilities(&self) -> SourceCapabilities {
        SourceCapabilities {
            metadata: true,
            units: true,
            bibliography: true,
            per_unit_fetch: false,
            incremental: false,
            authentication: AuthKind::None,
            min_interval_millis: None,
        }
    }

    fn can_handle(&self, url: &Url) -> bool {
        url.host_str()
            .map(|h| self.host_matches(h))
            .unwrap_or(false)
    }

    fn hosts(&self) -> Vec<String> {
        self.hosts.clone()
    }

    async fn preview(
        &self,
        fetch: &dyn Fetcher,
        url: &Url,
        _creds: Option<&Credentials>,
    ) -> SourceResult<SourceWork> {
        let uid = Self::user_id(url)
            .ok_or_else(|| SourceError::Unsupported(format!("{url} is not a Pawchive user URL")))?;
        let wanted = requested_story(url);
        let posts = all_posts(fetch, &uid).await?;
        if posts.is_empty() {
            return Err(SourceError::NotFound);
        }

        let selected: Vec<&ApiPost> = posts
            .iter()
            .filter(|post| match &wanted {
                Some(slug) => story_slug(&post.title) == *slug,
                None => true,
            })
            .collect();
        if selected.is_empty() {
            return Err(SourceError::NotFound);
        }

        let mut tag_set: Vec<String> = Vec::new();
        let mut units = Vec::new();
        for post in &selected {
            for tag in &post.tags {
                if !tag.is_empty()
                    && !tag_set
                        .iter()
                        .any(|existing| existing.eq_ignore_ascii_case(tag))
                {
                    tag_set.push(tag.clone());
                }
            }
            let text = strip_tags(&post.content).trim().to_owned();
            if text.len() > 50 {
                units.push(crate::UnitRef {
                    ordinal: units.len() as u32 + 1,
                    source_unit_key: post.id.clone(),
                    title: post.title.clone(),
                });
            }
        }

        let author_name = fetch
            .get(&format!(
                "https://pawchive.pw/api/v1/patreon/user/{uid}/profile"
            ))
            .await
            .ok()
            .and_then(|page| serde_json::from_str::<ApiProfile>(&page.body).ok())
            .map(|profile| profile.name)
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| format!("Pawchive user {uid}"));

        let (source_work_key, title, summary) = match &wanted {
            Some(slug) => {
                let story = story_title(&selected[0].title);
                (
                    format!("pawchive:{uid}:{slug}"),
                    story.clone(),
                    format!("{story} by {author_name} on Pawchive"),
                )
            }
            None => (
                uid.clone(),
                author_name.clone(),
                format!("{} posts on Pawchive", selected.len()),
            ),
        };

        Ok(SourceWork {
            source_key: self.key.clone(),
            source_work_key,
            source_url: if wanted.is_some() {
                url.to_string()
            } else {
                format!("https://pawchive.pw/patreon/user/{uid}")
            },
            title,
            author_text: author_name,
            author_url: Some(format!("https://pawchive.pw/patreon/user/{uid}")),
            summary,
            word_count: None,
            language: Some("en".to_owned()),
            status: WorkStatus::Unknown,
            published_at: None,
            updated_at: None,
            units,
            rating_text: None,
            warning_texts: Vec::new(),
            tags: tag_set,
        })
    }

    async fn fetch_units(
        &self,
        fetch: &dyn Fetcher,
        work: &SourceWork,
        _creds: Option<&Credentials>,
    ) -> SourceResult<Vec<SourceUnit>> {
        let url = Url::parse(&work.source_url)
            .map_err(|error| SourceError::Unsupported(format!("invalid source url: {error}")))?;
        let uid = Self::user_id(&url).ok_or_else(|| {
            SourceError::Unsupported(format!("{} is not a Pawchive URL", work.source_url))
        })?;
        let wanted = requested_story(&url);
        let posts = all_posts(fetch, &uid).await?;
        let mut units = Vec::new();
        for post in posts {
            if let Some(slug) = &wanted {
                if story_slug(&post.title) != *slug {
                    continue;
                }
            }
            let text = collapse_whitespace(&strip_tags(&post.content));
            if text.len() <= 50 {
                continue;
            }
            units.push(SourceUnit {
                ordinal: units.len() as u32 + 1,
                source_unit_key: post.id,
                title: post.title,
                content_html: text,
                image_urls: Vec::new(),
            });
        }
        Ok(units)
    }

    async fn list_author_works(
        &self,
        fetch: &dyn Fetcher,
        profile_url: &Url,
        _creds: Option<&Credentials>,
    ) -> SourceResult<Vec<String>> {
        let uid = Self::user_id(profile_url).ok_or_else(|| {
            SourceError::Unsupported(format!("{profile_url} is not a Pawchive user URL"))
        })?;
        let posts = all_posts(fetch, &uid).await?;
        let titles: Vec<&str> = posts
            .iter()
            .filter(|post| strip_tags(&post.content).trim().len() > 50)
            .map(|post| post.title.as_str())
            .collect();
        Ok(distinct_story_slugs(&titles)
            .into_iter()
            .map(|slug| format!("https://pawchive.pw/patreon/user/{uid}?story={slug}"))
            .collect())
    }

    fn preview_from_html(&self, html: &str, url: &Url) -> SourceResult<SourceWork> {
        let _ = (html, url);
        Err(SourceError::Unsupported(
            "Pawchive has no HTML preview entry point".to_owned(),
        ))
    }

    fn units_from_html(&self, html: &str, work: &SourceWork) -> SourceResult<Vec<SourceUnit>> {
        let _ = (html, work);
        Err(SourceError::Unsupported(
            "Pawchive has no HTML chapters entry point".to_owned(),
        ))
    }
}

/// Strip HTML tags from a fragment, returning plain text.
fn strip_tags(html: &str) -> String {
    let document = Html::parse_fragment(html);
    document.root_element().text().collect::<Vec<_>>().join(" ")
}

/// Collapse internal whitespace to a single string.
fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tag_tests {
    use super::{ApiPost, ApiProfile};

    fn parse(json: &str) -> Result<ApiPost, serde_json::Error> {
        serde_json::from_str(json)
    }

    #[test]
    fn tags_split_a_comma_separated_set_literal() {
        // The shape pawchive actually returns for user 26677387. Before the
        // comma split this produced one tag: "BIS,Commissioned,battletech,warhammer40k".
        let post = parse(
            r#"{"id":"1","user":"7","title":"t","content":"c",
                "tags":"{BIS,Commissioned,battletech,warhammer40k}"}"#,
        )
        .expect("comma set-literal parses");
        assert_eq!(
            post.tags,
            vec!["BIS", "Commissioned", "battletech", "warhammer40k"]
        );
    }

    #[test]
    fn tags_prefer_quoted_runs_so_a_comma_inside_a_tag_survives() {
        // "Highschool CxC" carries no comma, but 'No Sex, Plot' style tags do.
        let post = parse(
            r#"{"id":"1","user":"7","title":"t","content":"c",
                "tags":"{\"Free Write\",\"No Sex\",Marvel,\"Time Travel\"}"}"#,
        )
        .expect("quoted set-literal parses");
        assert_eq!(
            post.tags,
            vec!["Free Write", "No Sex", "Marvel", "Time Travel"]
        );
    }

    #[test]
    fn tags_drop_duplicates_case_insensitively() {
        let post =
            parse(r#"{"id":"1","user":"7","title":"t","content":"c","tags":"{WIP,wip,WIP}"}"#)
                .expect("duplicate tags parse");
        assert_eq!(post.tags, vec!["WIP"]);
    }

    #[test]
    fn tags_accept_the_array_form() {
        let post = parse(r#"{"id":"1","user":"7","title":"t","content":"c","tags":["a","b"]}"#)
            .expect("array tags parse");
        assert_eq!(post.tags, vec!["a", "b"]);
    }

    #[test]
    fn tags_accept_a_bare_string_without_braces() {
        let post = parse(r#"{"id":"1","user":"7","title":"t","content":"c","tags":"solo"}"#)
            .expect("bare string tags parse");
        assert_eq!(post.tags, vec!["solo"]);
    }

    #[test]
    fn tags_accept_the_string_form() {
        // Pawchive returns "{Magiscape}" for single-tag posts (user 77053790).
        let post = parse(r#"{"id":"1","user":"7","title":"t","content":"c","tags":"{Magiscape}"}"#)
            .expect("string tags parse");
        assert_eq!(post.tags, vec!["Magiscape"]);
    }

    #[test]
    fn tags_accept_absence_null_and_empty_strings() {
        let base = r#"{"id":"1","user":"7","title":"t","content":"c""#;
        assert!(parse(&format!("{base}}}")).is_ok());
        assert!(parse(&format!("{base},\"tags\":null}}")).is_ok());
        let post = parse(&format!("{base},\"tags\":\"{{}}\"}}")).expect("empty braces parse");
        assert!(post.tags.is_empty());
    }

    #[test]
    fn tags_reject_other_shapes() {
        let post = parse(r#"{"id":"1","user":"7","title":"t","content":"c","tags":42}"#);
        assert!(post.is_err());
    }

    #[test]
    fn the_author_profile_carries_a_display_name() {
        // The shape of /api/v1/patreon/user/5149963/profile.
        let profile: ApiProfile =
            serde_json::from_str(r#"{"id":"5149963","name":"Cambrian","service":"patreon"}"#)
                .expect("profile parses");
        assert_eq!(profile.name, "Cambrian");
    }
}

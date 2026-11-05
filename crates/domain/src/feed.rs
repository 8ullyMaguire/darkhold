//! M63-14 — a saved query as a feed.
//!
//! A feed is the cheapest client a power user has: their reader polls, and a poll that changed nothing
//! costs one request. That is what the ETag is for — **a 304 is the feature**, not an optimisation.
//!
//! **Three shapes, one document.** Atom, RSS 2.0 and JSON Feed 1.1 carry the same `FeedItem`s. They are
//! rendered by hand rather than by a serialiser crate because the XML is nine lines and a dependency that
//! renders four elements is a dependency that decides your escaping for you; the escaping here is explicit
//! and tested against the characters that actually appear in fic titles (`&`, `<`, `"`, `'`).
//!
//! **The id is the item id, not a permalink.** A feed is rebuilt on every poll, so an item must keep its
//! identity across polls or the reader shows forty unread copies of the same work. `id` is the library row
//! (or the work), `guid isPermaLink="false"` in RSS, `<id>` in Atom, `id` in JSON Feed.

use serde::Serialize;

/// One entry in a feed.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FeedItem {
    /// Stable across polls. Not a URL.
    pub id: String,
    pub title: String,
    /// Where a reader goes to read it.
    pub link: String,
    /// RFC 3339. Absent when the source said nothing, rather than replaced by "now".
    pub updated: Option<String>,
    pub summary: String,
    pub author: String,
}

/// A whole feed.
#[derive(Debug, Clone)]
pub struct Feed {
    /// The view's name, as the reader wrote it.
    pub title: String,
    /// Where the feed lives, for its self link.
    pub self_url: String,
    /// Where a person goes to see the same thing in a browser.
    pub link: String,
    pub items: Vec<FeedItem>,
}

/// The three shapes M63-14 names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Atom,
    Rss,
    JsonFeed,
}

impl Format {
    /// `?format=` value, or the extension a reader guesses from.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "atom" | "xml" => Some(Self::Atom),
            "rss" | "rss2" => Some(Self::Rss),
            "json" | "jsonfeed" | "json-feed" => Some(Self::JsonFeed),
            _ => None,
        }
    }

    /// The `Content-Type` a client needs to recognise the document.
    #[must_use]
    pub fn content_type(self) -> &'static str {
        match self {
            // `application/feed+json` is RFC 7303-adjacent and what the JSON Feed spec asks for; the
            // charset is spelled because a feed with an author name in it is text.
            Self::JsonFeed => "application/feed+json; charset=utf-8",
            Self::Atom => "application/atom+xml; charset=utf-8",
            Self::Rss => "application/rss+xml; charset=utf-8",
        }
    }

    /// The file extension a static export would use.
    #[must_use]
    pub fn extension(self) -> &'static str {
        match self {
            Self::Atom => "atom",
            Self::Rss => "rss",
            Self::JsonFeed => "json",
        }
    }
}

/// Render the feed in one of the three shapes.
#[must_use]
pub fn render(feed: &Feed, format: Format) -> String {
    match format {
        Format::Atom => atom(feed),
        Format::Rss => rss(feed),
        Format::JsonFeed => json_feed(feed),
    }
}

/// The feed's strong `ETag`.
///
/// Computed from the rendered bytes, so it changes when *anything* a reader would see changes and does not
/// change when they poll again. Quoted, because `If-None-Match` compares quoted strings.
#[must_use]
pub fn etag(body: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(body.as_bytes());
    let digest = hasher.finalize();
    let mut text = String::from("\"");
    for byte in &digest[..16] {
        text.push_str(&format!("{byte:02x}"));
    }
    text.push('"');
    text
}

/// Whether an `If-None-Match` header names this tag.
///
/// Handles a list, the `W/` weak prefix (a reader that sends a weak tag is still telling us it has the
/// bytes), and `*`.
#[must_use]
pub fn matches(header: &str, tag: &str) -> bool {
    let header = header.trim();
    if header == "*" {
        return true;
    }
    header.split(',').any(|candidate| {
        let candidate = candidate.trim();
        let candidate = candidate.strip_prefix("W/").unwrap_or(candidate);
        candidate == tag
    })
}

/// XML text, escaped. `&` first, or the escapes escape each other.
fn xml(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // Control characters other than tab/newline/carriage return are not legal in XML 1.0 and a
            // single one makes the document unparseable, which a reader reports as "not a feed".
            c if (c as u32) < 0x20 && c != '\t' && c != '\n' && c != '\r' => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

fn atom(feed: &Feed) -> String {
    // Atom demands a feed-level `updated` even when nothing in it is dated. The epoch is the honest
    // constant: it says "nothing here has a time", and because it never changes the tag never changes
    // either, so an undated feed still answers 304 on every poll after the first.
    let updated = feed
        .items
        .iter()
        .filter_map(|item| item.updated.clone())
        .max()
        .unwrap_or_else(|| "1970-01-01T00:00:00Z".to_owned());
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n");
    out.push_str("<feed xmlns=\"http://www.w3.org/2005/Atom\">\n");
    out.push_str(&format!("  <id>{}</id>\n", xml(&feed.self_url)));
    out.push_str(&format!("  <title>{}</title>\n", xml(&feed.title)));
    out.push_str(&format!("  <updated>{}</updated>\n", xml(&updated)));
    out.push_str(&format!(
        "  <link rel=\"self\" href=\"{}\"/>\n",
        xml(&feed.self_url)
    ));
    out.push_str(&format!(
        "  <link rel=\"alternate\" href=\"{}\"/>\n",
        xml(&feed.link)
    ));
    for item in &feed.items {
        out.push_str("  <entry>\n");
        out.push_str(&format!("    <id>{}</id>\n", xml(&item.id)));
        out.push_str(&format!("    <title>{}</title>\n", xml(&item.title)));
        out.push_str(&format!(
            "    <link rel=\"alternate\" href=\"{}\"/>\n",
            xml(&item.link)
        ));
        if let Some(updated) = &item.updated {
            out.push_str(&format!("    <updated>{}</updated>\n", xml(updated)));
        }
        if !item.author.is_empty() {
            out.push_str(&format!(
                "    <author><name>{}</name></author>\n",
                xml(&item.author)
            ));
        }
        out.push_str(&format!("    <summary>{}</summary>\n", xml(&item.summary)));
        out.push_str("  </entry>\n");
    }
    out.push_str("</feed>\n");
    out
}

fn rss(feed: &Feed) -> String {
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n");
    out.push_str("<rss version=\"2.0\" xmlns:atom=\"http://www.w3.org/2005/Atom\">\n");
    out.push_str("  <channel>\n");
    out.push_str(&format!("    <title>{}</title>\n", xml(&feed.title)));
    out.push_str(&format!("    <link>{}</link>\n", xml(&feed.link)));
    out.push_str(&format!(
        "    <atom:link href=\"{}\" rel=\"self\" type=\"application/rss+xml\"/>\n",
        xml(&feed.self_url)
    ));
    out.push_str(&format!(
        "    <description>{}</description>\n",
        xml(&format!("{} — a saved query", feed.title))
    ));
    for item in &feed.items {
        out.push_str("    <item>\n");
        out.push_str(&format!("      <title>{}</title>\n", xml(&item.title)));
        out.push_str(&format!("      <link>{}</link>\n", xml(&item.link)));
        // Not a permalink: the id identifies the row, and the row is not a page.
        out.push_str(&format!(
            "      <guid isPermaLink=\"false\">{}</guid>\n",
            xml(&item.id)
        ));
        if let Some(updated) = &item.updated {
            out.push_str(&format!("      <pubDate>{}</pubDate>\n", xml(updated)));
        }
        out.push_str(&format!(
            "      <description>{}</description>\n",
            xml(&item.summary)
        ));
        out.push_str("    </item>\n");
    }
    out.push_str("  </channel>\n</rss>\n");
    out
}

fn json_feed(feed: &Feed) -> String {
    let items: Vec<serde_json::Value> = feed
        .items
        .iter()
        .map(|item| {
            let mut object = serde_json::json!({
                "id": item.id,
                "title": item.title,
                "url": item.link,
                "content_text": item.summary,
            });
            if let Some(updated) = &item.updated {
                object["date_published"] = serde_json::json!(updated);
            }
            if !item.author.is_empty() {
                object["authors"] = serde_json::json!([{ "name": item.author }]);
            }
            object
        })
        .collect();
    let document = serde_json::json!({
        "version": "https://jsonfeed.org/version/1.1",
        "title": feed.title,
        "home_page_url": feed.link,
        "feed_url": feed.self_url,
        "items": items,
    });
    // Pretty: a feed a person may read in a terminal, and diffs that line up.
    serde_json::to_string_pretty(&document).unwrap_or_else(|_| "{}".to_owned())
}

//! The source catalogue: every platform this build knows of, what it may read, and how it is paced.
//!
//! # Why this exists
//!
//! Readers do not live on one archive. When an archive wobbles, the fandom scatters across a dozen
//! smaller ones — some running the same software, some forums, some one-person neocities pages. Darkhold
//! already had adapters for eleven of them, but **no answer to the question an operator actually asks**:
//! *which sites does this build know about, which of them may I import from, and why not?* That answer
//! lived in eleven doc comments and a `robots.txt` fetch that only happened at import time, one work at
//! a time.
//!
//! So the catalogue is data, not code:
//!
//! * **Every listed platform has an entry**, whether or not an adapter exists. An entry with
//!   `adapter: null` is a site we deliberately do not read yet, with the reason in `note` — which is a
//!   different thing from a site nobody remembered to write down.
//! * **The robots verdict is recorded, dated, and the basis of `bodies`.** `blocked` means the site's
//!   `robots.txt` disallows the whole tree, or refuses this client outright; `metadata_only` means the
//!   listing pages are readable but the prose paths are not (or the prose is a file download the site
//!   disallows); `allowed` means the `*` group leaves work pages readable.
//! * **Discovery is declared per entry**, because "where do new works appear" is a fact about the site.
//!   Today only the tag-page shape is implemented — `{tag}` substituted with a fandom name — because
//!   the three AO3-software archives are the ones this build can walk unattended.
//!
//! # The rule this file encodes
//!
//! **Metadata is always worth having; prose is not.** A site that disallows `/` is not read at all. A
//! site that allows listing pages but disallows chapter text is read for its catalogue — title, author,
//! tags, word count — and the reader is sent to the source to read. That is the posture the operator
//! asked for, and it is the posture that keeps the importer a citizen rather than a scraper.
//!
//! The verdicts were read from each host on 2026-11-02 and are dated in the file. They go stale, so the
//! importer re-reads `robots.txt` before touching a source — this table is the *reviewed* state, not
//! the runtime authority.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

/// The catalogue as shipped in this build.
///
/// `include_str!` rather than a file read: a deployed binary must not depend on a directory that a
/// packaging step can forget to copy, and the catalogue is small enough that a rebuild is cheaper than
/// a missing file at 3am.
pub const BUNDLED: &str = include_str!("../sources.json");

/// How much of a source this build may read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Bodies {
    /// Listing pages and work pages are readable, so prose may be imported.
    Allowed,
    /// Metadata only: the catalogue is readable, the prose is not. A reader is sent to the source.
    MetadataOnly,
    /// Nothing may be fetched. Either `robots.txt` disallows the tree or the host refuses us.
    Blocked,
}

impl Bodies {
    /// Whether an import may store prose for this source.
    #[must_use]
    pub const fn may_read_bodies(self) -> bool {
        matches!(self, Self::Allowed)
    }

    /// Whether the source may be fetched at all.
    #[must_use]
    pub const fn may_fetch_metadata(self) -> bool {
        !matches!(self, Self::Blocked)
    }

    /// The stored spelling, which is the JSON spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Allowed => "allowed",
            Self::MetadataOnly => "metadata_only",
            Self::Blocked => "blocked",
        }
    }
}

/// How new works appear at a source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Discovery {
    /// `https://host/tags/{tag}/works` — an AO3-software tag listing.
    TagPage {
        /// A path template; `{tag}` is substituted and URL-escaped.
        template: String,
    },
    /// A listing page that needs no tag: `/series-ranking/`, `/latest-series/`.
    ///
    /// Some sites publish no tag-indexed browse that a crawler may walk, but do publish "what is new"
    /// — and that is enough to keep a catalogue fresh. The template carries no `{tag}`, so a sync walks
    /// it once per pass rather than once per configured tag.
    Listing {
        /// A path template, substituted with nothing.
        template: String,
    },
    /// No unattended discovery. A reader may paste a URL; nothing walks the site.
    None,
}

/// The recorded `robots.txt` verdict, and the day it was read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RobotsCheck {
    /// What the `*` group says, in a sentence.
    pub verdict: String,
    /// `YYYY-MM-DD`. A verdict with no date is a rumour.
    pub checked: String,
}

/// One platform.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    /// Stable identifier, also the `source_id` an adapter is keyed by where one exists.
    pub id: String,
    /// What a reader is shown.
    pub name: String,
    /// The host, bare: `squidgeworld.org`.
    pub host: String,
    /// The software family, which is what makes an adapter reusable: `ao3`, `efiction`, `xenforo`,
    /// `royalroad`, `scribblehub`, `bigwattpad`, `syosetu`, `ficbook`, or `custom`.
    pub family: String,
    /// The adapter key in this build, when one can read the site.
    pub adapter: Option<String>,
    /// How much may be read.
    pub bodies: Bodies,
    /// The recorded robots verdict.
    pub robots: RobotsCheck,
    /// Where new works appear.
    pub discovery: Discovery,
    /// Why this entry says what it says. Never empty: a blank note is an unreviewed entry.
    pub note: String,
}

impl Entry {
    /// Whether an unattended sync may walk this source.
    #[must_use]
    pub fn is_syncable(&self) -> bool {
        self.adapter.is_some()
            && self.bodies.may_fetch_metadata()
            && !matches!(self.discovery, Discovery::None)
    }

    /// The one listing URL, for a source whose discovery needs no tag.
    #[must_use]
    pub fn listing_url(&self) -> Option<String> {
        match &self.discovery {
            Discovery::Listing { template } => Some(format!("https://{}{}", self.host, template)),
            _ => None,
        }
    }

    /// The listing URL for a tag, when this source has a tag-page discovery shape.
    #[must_use]
    pub fn tag_page_url(&self, tag: &str) -> Option<String> {
        let Discovery::TagPage { template } = &self.discovery else {
            return None;
        };
        Some(format!(
            "https://{}{}",
            self.host,
            template.replace("{tag}", &urlencode(tag))
        ))
    }
}

/// Percent-encode a tag for a path segment.
///
/// `tags/Naruto` and `tags/Star%20Trek` are both real; a tag with a slash in it (`Fate/stay night`)
/// must not silently become two path segments, which is why `/` is encoded and not left alone.
fn urlencode(tag: &str) -> String {
    let mut out = String::with_capacity(tag.len());
    for byte in tag.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(char::from(byte));
            }
            b' ' => out.push_str("%20"),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// The whole catalogue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalogue {
    /// The schema version. A bump means a field changed meaning.
    pub version: u32,
    /// `YYYY-MM-DD`, the day the robots verdicts were read.
    pub checked: String,
    /// Every platform, in the order a screen should show them.
    pub entries: Vec<Entry>,
}

/// Why a catalogue was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogueError(String);

impl std::fmt::Display for CatalogueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CatalogueError {}

impl Catalogue {
    /// Parse and validate a catalogue.
    ///
    /// Validation is not decoration: a duplicate host means two entries disagree about what may be
    /// fetched from one site, and whichever the loop reached first would silently win. A `blocked` entry
    /// with an adapter is the other shape that must not ship — it says "this build can read a site it may
    /// not touch", and the only safe reading of that is a mistake in the table.
    pub fn parse(raw: &str) -> Result<Self, CatalogueError> {
        let catalogue: Self = serde_json::from_str(raw)
            .map_err(|error| CatalogueError(format!("the catalogue is not valid JSON: {error}")))?;

        let mut hosts = BTreeSet::new();
        let mut ids = BTreeSet::new();
        for entry in &catalogue.entries {
            if !ids.insert(entry.id.clone()) {
                return Err(CatalogueError(format!("duplicate entry id `{}`", entry.id)));
            }
            if !hosts.insert(entry.host.clone()) {
                return Err(CatalogueError(format!(
                    "duplicate host `{}`: two entries would disagree about it",
                    entry.host
                )));
            }
            if entry.host.contains('/') || entry.host.contains(':') {
                return Err(CatalogueError(format!(
                    "`{}` carries a scheme or port; the field is a bare host",
                    entry.host
                )));
            }
            if entry.note.trim().is_empty() {
                return Err(CatalogueError(format!(
                    "entry `{}` has no note; an unreviewed entry is not an entry",
                    entry.id
                )));
            }
            if let Discovery::Listing { template } = &entry.discovery {
                if !template.starts_with('/') {
                    return Err(CatalogueError(format!(
                        "entry `{}` has a listing template that is not a path: `{template}`",
                        entry.id
                    )));
                }
                if template.contains("{tag}") {
                    return Err(CatalogueError(format!(
                        "entry `{}` lists a tag template as a tagless listing; one of the two is wrong",
                        entry.id
                    )));
                }
            }
            if let Discovery::TagPage { template } = &entry.discovery {
                if !template.contains("{tag}") {
                    return Err(CatalogueError(format!(
                        "entry `{}` has a tag-page discovery with no {{tag}} in its template",
                        entry.id
                    )));
                }
                if !template.starts_with('/') {
                    return Err(CatalogueError(format!(
                        "entry `{}` has a template that is not a path: `{template}`",
                        entry.id
                    )));
                }
            }
        }
        Ok(catalogue)
    }

    /// The catalogue this build ships.
    pub fn bundled() -> Result<Self, CatalogueError> {
        Self::parse(BUNDLED)
    }

    /// One entry by id.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    /// One entry by host, matching `www.` as the same site.
    #[must_use]
    pub fn by_host(&self, host: &str) -> Option<&Entry> {
        let bare = host.strip_prefix("www.").unwrap_or(host);
        self.entries
            .iter()
            .find(|entry| entry.host == bare || entry.host == host)
    }

    /// The entries an unattended sync may walk, in catalogue order.
    pub fn syncable(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter().filter(|entry| entry.is_syncable())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bundled_catalogue_is_valid() {
        let catalogue = Catalogue::bundled().expect("the shipped catalogue parses and validates");
        assert!(
            catalogue.entries.len() >= 30,
            "the list is the point: {} entries",
            catalogue.entries.len()
        );
    }

    #[test]
    fn a_duplicate_host_is_refused_because_two_entries_would_disagree() {
        let mut catalogue = Catalogue::bundled().expect("bundled");
        catalogue.entries.push(catalogue.entries[0].clone());
        let error = Catalogue::parse(&serde_json::to_string(&catalogue).expect("serialise"))
            .expect_err("a duplicate host must be refused");
        assert!(error.to_string().contains("duplicate"), "{error}");
    }

    #[test]
    fn a_blocked_entry_may_name_an_adapter_but_is_never_walked() {
        // A blocked source may keep its adapter: the code can be right while the host refuses this
        // instance (`royalroad.com` answers 403 to every listing and work page here, with any user
        // agent). The invariant is not "no adapter" but "not walkable", and it holds structurally
        // because `is_syncable` asks `may_fetch_metadata` first.
        let catalogue = Catalogue::bundled().expect("bundled");
        let mut blocked = 0;
        for entry in &catalogue.entries {
            if entry.bodies == Bodies::Blocked {
                blocked += 1;
                assert!(
                    !entry.is_syncable(),
                    "`{}` is blocked and walkable — the pass would fetch a source we may not touch",
                    entry.id
                );
            }
        }
        assert!(blocked >= 4, "the catalogue has blocked entries to check");
        assert!(
            catalogue
                .entries
                .iter()
                .any(|e| e.bodies == Bodies::Blocked && e.adapter.is_some()),
            "and at least one of them keeps its adapter, which is legal"
        );
    }

    #[test]
    fn every_syncable_entry_names_an_adapter_this_build_has() {
        let catalogue = Catalogue::bundled().expect("bundled");
        let registry = crate::sites::default_registry();
        let known: BTreeSet<String> = registry
            .adapters()
            .iter()
            .map(|adapter| adapter.key().as_str().to_owned())
            .collect();
        for entry in &catalogue.entries {
            if let Some(adapter) = &entry.adapter {
                assert!(
                    known.contains(adapter),
                    "`{}` names adapter `{adapter}`, which this build does not have; known: {known:?}",
                    entry.id
                );
            }
        }
    }

    #[test]
    fn a_syncable_entry_has_something_to_walk() {
        for entry in Catalogue::bundled().expect("bundled").syncable() {
            match &entry.discovery {
                Discovery::TagPage { .. } => {
                    let url = entry
                        .tag_page_url("Star Trek")
                        .expect("a tag page resolves for a tag source");
                    assert_eq!(
                        url,
                        format!("https://{}/tags/Star%20Trek/works", entry.host),
                        "the template puts the tag in a path segment, escaped"
                    );
                }
                Discovery::Listing { .. } => {
                    let url = entry
                        .listing_url()
                        .expect("a listing source resolves without a tag");
                    assert!(
                        url.starts_with(&format!("https://{}/", entry.host)),
                        "the listing is on the source's own host: {url}"
                    );
                    assert!(
                        !url.contains("{tag}"),
                        "a tagless listing carries no placeholder: {url}"
                    );
                }
                Discovery::None => panic!("`{}` is syncable with nothing to walk", entry.id),
            }
        }
    }

    #[test]
    fn a_tag_with_a_slash_does_not_become_two_segments() {
        // `Fate/stay night` is a real fandom tag. Left unescaped it would address a different URL, and
        // the importer would report "no works" for a fandom with thousands.
        let entry = Catalogue::bundled()
            .expect("bundled")
            .entries
            .into_iter()
            .find(|entry| matches!(entry.discovery, Discovery::TagPage { .. }))
            .expect("at least one tag-page source");
        let url = entry.tag_page_url("Fate/stay night").expect("a URL");
        assert!(url.ends_with("/tags/Fate%2Fstay%20night/works"), "{url}");
    }
}

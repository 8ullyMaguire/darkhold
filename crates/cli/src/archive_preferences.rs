//! Private taste preferences. Loaded from a 0600 local file, never committed.
//!
//! The explicit JSON is the authority. The operator's local reading list and
//! browser bookmarks are read-only sources of additional low-weight signals;
//! their text is never printed, committed or sent over the network.

use anyhow::{bail, Result};
use darkhold_taste::curation::Preference;
use serde::Deserialize;
use std::fs;
use std::path::Path;

const READ_LIST: &str = "/home/alvaro/mnt/thinkcentre/personal/literature/fanfiction/read.txt";
const BOOKMARKS: &str = "/home/alvaro/mnt/thinkcentre/personal/downloads/bookmarks.html";

#[derive(Debug, Deserialize)]
struct LocalFile {
    version: u32,
    positive: Vec<LocalTerm>,
    negative: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct LocalTerm {
    term: String,
    weight: f64,
}

pub struct LoadedPreferences {
    pub preference: Preference,
    pub source_terms: usize,
}

pub fn load(path: &Path) -> Result<LoadedPreferences> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let mode = fs::metadata(path)?.mode() & 0o777;
        if mode != 0o600 {
            bail!("{} must be mode 0600, found {mode:04o}", path.display());
        }
    }
    let raw = fs::read_to_string(path)?;
    let file: LocalFile = serde_json::from_str(&raw)?;
    if file.version != 1 {
        bail!("unsupported preference version {}", file.version);
    }
    if file.positive.is_empty() {
        bail!("at least one positive preference is required");
    }
    let mut positive: Vec<(String, f64)> = file
        .positive
        .into_iter()
        .map(|term| (term.term, term.weight))
        .collect();
    let mut source_terms = 0;
    for source in [READ_LIST, BOOKMARKS] {
        let Ok(text) = fs::read_to_string(source) else {
            continue;
        };
        let derived = if source == READ_LIST {
            derive_from_read_list(&text)
        } else {
            derive_from_bookmarks(&text)
        };
        for term in derived {
            if push_low_weight(&mut positive, &term, 0.25) {
                source_terms += 1;
            }
        }
    }
    Ok(LoadedPreferences {
        preference: Preference {
            positive,
            negative: file.negative,
        },
        source_terms,
    })
}

fn push_low_weight(terms: &mut Vec<(String, f64)>, term: &str, weight: f64) -> bool {
    let needle = term.trim().to_ascii_lowercase();
    if needle.is_empty() || needle.len() > 60 {
        return false;
    }
    if terms
        .iter()
        .any(|(existing, _)| existing.eq_ignore_ascii_case(&needle))
    {
        return false;
    }
    terms.push((needle, weight));
    true
}

fn derive_from_read_list(raw: &str) -> Vec<String> {
    const STOP: &[&str] = &[
        "the", "and", "with", "from", "this", "that", "for", "you", "his", "her", "into",
        "being", "after", "before", "they", "their", "she", "her", "him", "not", "but",
    ];
    let mut out: Vec<(String, f64)> = Vec::new();
    for line in raw.lines() {
        let mut tokens = line.split_whitespace();
        let Some(id) = tokens.next() else { continue };
        let Some(rating) = tokens.next() else { continue };
        if id.parse::<u64>().is_err() || rating.parse::<u8>().unwrap_or(0) < 8 {
            continue;
        }
        for token in tokens {
            let clean = token
                .trim_matches(|c: char| !c.is_ascii_alphanumeric())
                .to_ascii_lowercase();
            if clean.len() > 3
                && clean.chars().any(|c| c.is_ascii_alphabetic())
                && !STOP.contains(&clean.as_str())
                && push_low_weight(&mut out, &clean, 0.25)
            {
            }
        }
    }
    out.truncate(120);
    out.into_iter().map(|(term, _)| term).collect()
}

fn fragments(raw: &str, open: &str, close: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = raw;
    while let Some(start) = rest.find(open) {
        let after = &rest[start + open.len()..];
        let Some(end) = after.find(close) else { break };
        out.push(after[..end].to_owned());
        rest = &after[end + close.len()..];
    }
    out
}

fn strip_tags(raw: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in raw.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn derive_from_bookmarks(raw: &str) -> Vec<String> {
    const SKIP: &[&str] = &[
        "programming", "utility", "forums", "news", "misc", "other", "tools", "search",
        "bookmarks toolbar", "mozilla firefox", "manjaro linux", "mozillafirefox",
    ];
    let mut out: Vec<(String, f64)> = Vec::new();
    for chunk in fragments(raw, "<H3", "</H3>") {
        let name = strip_tags(&chunk).to_ascii_lowercase();
        if name.len() > 3
            && name.len() <= 40
            && !SKIP.contains(&name.as_str())
        {
            push_low_weight(&mut out, &name, 0.25);
        }
    }
    for chunk in fragments(raw, "<A ", "</A>") {
        let title = strip_tags(&chunk);
        if title.len() > 3 && title.len() <= 60 && !title.starts_with("http") {
            push_low_weight(&mut out, &title, 0.25);
        }
    }
    out.truncate(160);
    out.into_iter().map(|(term, _)| term).collect()
}

//! Pure, private taste scoring. No I/O and no database access.

#[derive(Debug, Clone, PartialEq)]
pub struct Preference {
    pub positive: Vec<(String, f64)>,
    pub negative: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub title: String,
    pub summary: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Score {
    pub value: f64,
    pub reason: String,
    pub excluded: bool,
    pub confidence: f64,
}

fn normalise(raw: &str) -> String {
    raw.to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn haystack(candidate: &Candidate) -> String {
    let mut parts = vec![
        normalise(&candidate.title),
        normalise(&candidate.summary),
    ];
    parts.extend(candidate.tags.iter().map(|tag| normalise(tag)));
    parts.join(" ")
}

pub fn score(preference: &Preference, candidate: &Candidate) -> Score {
    let hay = haystack(candidate);
    let mut matched: Vec<(&str, f64)> = Vec::new();
    for (term, weight) in &preference.positive {
        let needle = normalise(term);
        if !needle.is_empty() && hay.contains(&needle) {
            matched.push((term.as_str(), *weight));
        }
    }
    for term in &preference.negative {
        let needle = normalise(term);
        if !needle.is_empty() && hay.contains(&needle) {
            return Score {
                value: 0.0,
                reason: format!("excluded:{term}"),
                excluded: true,
                confidence: 1.0,
            };
        }
    }
    let value: f64 = matched.iter().map(|(_, weight)| *weight).sum();
    let reason = if matched.is_empty() {
        "no private taste match".to_owned()
    } else {
        matched
            .iter()
            .map(|(term, weight)| format!("{term}:{weight}"))
            .collect::<Vec<_>>()
            .join("; ")
    };
    let confidence = if preference.positive.is_empty() {
        0.0
    } else {
        (matched.len() as f64 / preference.positive.len() as f64).min(1.0)
    };
    Score {
        value,
        reason,
        excluded: false,
        confidence,
    }
}

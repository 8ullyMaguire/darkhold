//! M63-26: `dh stats` — a private, wrapped-style report assembled from two private server surfaces.
//!
//! The streak numbers come from `GET /me/streak`. The contribution counts come from the origin's signed passport at
//! `GET /me/passport`, which already counts works, revisions and comments. There is no per-event history route, so the
//! period is a label and the report says which signed snapshot it came from; inventing week buckets from totals would be
//! a number with no source.

use base64::Engine;
use clap::{Args, ValueEnum};
use darkhold_domain::passport::{Contributions, Passport};
use serde::Serialize;
use serde_json::Value;

use crate::rec::{auth_required, bad_request, environment, failure_for, Failure};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Period {
    Week,
    Year,
}

impl Period {
    fn as_str(self) -> &'static str {
        match self {
            Self::Week => "week",
            Self::Year => "year",
        }
    }
}

/// `dh stats --by week --year 2026 --share`
#[derive(Debug, Clone, Args)]
pub struct StatsArgs {
    /// Which period this report labels.
    #[arg(long, value_enum, default_value = "week")]
    pub by: Period,

    /// The year a yearly report is about. A weekly report accepts it as context only.
    #[arg(long, value_name = "YYYY")]
    pub year: Option<i32>,

    /// Remove the handle and origin so the text can be pasted.
    #[arg(long)]
    pub share: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    pub period: String,
    pub year: Option<i32>,
    pub subject: String,
    pub origin: String,
    pub issued_at: String,
    pub current_streak: i64,
    pub longest_streak: i64,
    pub streak_freezes_used: i64,
    pub contributions: Contributions,
}

/// The two shapes can be rendered without a server, which is how the share rule is tested.
#[must_use]
pub fn render(report: &Report, share: bool) -> String {
    let mut lines = Vec::new();
    if !share {
        lines.push(format!("{} — {}", report.subject, report.origin));
    }
    lines.push(format!(
        "Your darkhold {} in review",
        if report.year.is_some() {
            format!("{} {}", report.year.unwrap_or(0), report.period)
        } else {
            report.period.clone()
        }
    ));
    lines.push(format!(
        "  sign-in streak        {} days (longest {})",
        report.current_streak, report.longest_streak
    ));
    lines.push(format!(
        "  works published       {}",
        report.contributions.works
    ));
    lines.push(format!(
        "  revisions made        {}",
        report.contributions.revisions
    ));
    lines.push(format!(
        "  comments written      {}",
        report.contributions.comments
    ));
    if !share {
        lines.push(format!(
            "  streak freezes used   {}",
            report.streak_freezes_used
        ));
        lines.push(format!("  signed snapshot       {}", report.issued_at));
    } else {
        lines.push("  a private snapshot from your own instance".to_owned());
    }
    lines.join("\n")
}

/// Decode the payload segment of the signed passport. The signature belongs to the receiving instance's verifier;
/// here the document came back over an authenticated session from its own origin.
fn passport_from_signed(signed: &str) -> Result<Passport, Failure> {
    let payload = signed.split('.').nth(1).ok_or_else(|| {
        bad_request("the passport response is not a signed document (no payload segment)")
    })?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|error| bad_request(format!("the passport payload is not base64url: {error}")))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| bad_request(format!("the passport payload is unreadable: {error}")))
}

fn number(body: &Value, key: &str) -> Result<i64, Failure> {
    body.get(key).and_then(Value::as_i64).ok_or_else(|| {
        bad_request(format!(
            "the server's stats response has no integer `{key}`"
        ))
    })
}

/// Dispatch. The global `--json` flag prints the same report as structured data.
pub async fn run(cli: &crate::Cli, args: &StatsArgs) -> Result<(), Failure> {
    if args.year.is_some_and(|year| !(2000..=2100).contains(&year)) {
        return Err(bad_request("--year must be between 2000 and 2100"));
    }
    let credential = if let Some(token) = &cli.token {
        Credential::Bearer(token.clone())
    } else if let Some(cookie) = &cli.cookie {
        Credential::Cookie(cookie.clone())
    } else {
        return Err(auth_required(
            "dh stats reads your own private report: set DARKHOLD_TOKEN or DARKHOLD_COOKIE",
        ));
    };
    let base = cli.base_url.trim_end_matches('/');
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|error| environment(format!("http client: {error}")))?;

    let streak_url = format!("{base}/api/v1/me/streak");
    let (status, bytes) =
        crate::http::send(|| credential.apply(client.get(&streak_url)), base).await?;
    let streak: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    if !(200..300).contains(&status) {
        return Err(failure_for(status, &streak, base));
    }

    let passport_url = format!("{base}/api/v1/me/passport");
    let (status, bytes) =
        crate::http::send(|| credential.apply(client.get(&passport_url)), base).await?;
    let envelope: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    if !(200..300).contains(&status) {
        return Err(failure_for(status, &envelope, base));
    }
    let signed = envelope
        .get("passport")
        .and_then(Value::as_str)
        .ok_or_else(|| bad_request("the server did not return a signed passport"))?;
    let passport = passport_from_signed(signed)?;

    let report = Report {
        period: args.by.as_str().to_owned(),
        year: args.year,
        subject: passport.subject,
        origin: passport.origin,
        issued_at: passport.issued_at,
        current_streak: number(&streak, "current_streak")?,
        longest_streak: number(&streak, "longest_streak")?,
        streak_freezes_used: number(&streak, "streak_freezes_used")?,
        contributions: passport.contributions,
    };

    if cli.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).unwrap_or_default()
        );
    } else {
        println!("{}", render(&report, args.share));
    }
    Ok(())
}

#[derive(Debug, Clone)]
enum Credential {
    Bearer(String),
    Cookie(String),
}

impl Credential {
    fn apply(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match self {
            Self::Bearer(token) => request.bearer_auth(token),
            Self::Cookie(cookie) => request.header(reqwest::header::COOKIE, cookie),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn report() -> Report {
        Report {
            period: "week".to_owned(),
            year: Some(2026),
            subject: "reader-handle".to_owned(),
            origin: "https://example.test".to_owned(),
            issued_at: "2026-11-18T00:00:00Z".to_owned(),
            current_streak: 12,
            longest_streak: 40,
            streak_freezes_used: 1,
            contributions: Contributions {
                works: 3,
                revisions: 27,
                comments: 11,
            },
        }
    }

    #[test]
    fn the_private_report_names_the_reader_and_the_snapshot() {
        let text = render(&report(), false);
        assert!(text.contains("reader-handle"));
        assert!(text.contains("2026-11-18T00:00:00Z"));
        assert!(text.contains("revisions made        27"));
    }

    #[test]
    fn the_shared_report_carries_only_aggregates() {
        let text = render(&report(), true);
        assert!(!text.contains("reader-handle"));
        assert!(!text.contains("example.test"));
        assert!(!text.contains("2026-11-18T00:00:00Z"));
        assert!(text.contains("revisions made        27"));
    }

    #[test]
    fn a_signed_payload_is_decoded_without_a_verification_claim() {
        let passport = Passport {
            version: darkhold_domain::passport::VERSION,
            origin: "https://example.test".to_owned(),
            subject: "reader-handle".to_owned(),
            issued_at: "2026-11-18T00:00:00Z".to_owned(),
            taste: BTreeMap::new(),
            favourite_sources: Vec::new(),
            contributions: Contributions {
                works: 1,
                revisions: 2,
                comments: 3,
            },
            affinity_bp: None,
        };
        let json = serde_json::to_vec(&passport).unwrap();
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json);
        let signed = format!("header.{payload}.signature");
        let decoded = passport_from_signed(&signed).expect("decoded");
        assert_eq!(decoded.subject, "reader-handle");
        assert_eq!(decoded.contributions.revisions, 2);
    }
}

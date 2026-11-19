//! M63-25: `dh audit --since 7d` — a private view of what the account did.
//!
//! The server accepts only RFC 3339, so a relative window has to be resolved here before it reaches the URL. The output says
//! "what your account did", not "what your tokens did": `audit_log` records the actor but not the credential, and claiming more
//! would be attribution the schema cannot support.

use clap::Args;
use serde_json::Value;

use crate::rec::{auth_required, bad_request, environment, failure_for, Failure};

/// `dh audit --since 7d --limit 50`
#[derive(Debug, Clone, Args)]
pub struct AuditArgs {
    /// Lower bound: `7d`, `12h`, `30m`, `60s`, or an RFC 3339 timestamp.
    #[arg(long, default_value = "7d")]
    pub since: String,

    /// At most this many entries; clamped to 1..=200 like the server door.
    #[arg(long, default_value_t = 50)]
    pub limit: i64,
}

/// Resolve a client window. The server's grammar is deliberately narrower, so a relative value never reaches it unresolved.
pub fn since_rfc3339(raw: &str, now: time::OffsetDateTime) -> Result<String, Failure> {
    if raw.len() > 1 && raw.ends_with(['s', 'm', 'h', 'd']) {
        let duration = crate::watch::parse_duration(raw, "--since")?;
        let seconds =
            i64::try_from(duration.as_secs()).map_err(|_| bad_request("--since is too large"))?;
        let at = now - time::Duration::seconds(seconds);
        return at
            .format(&time::format_description::well_known::Rfc3339)
            .map_err(|error| environment(format!("timestamp formatting: {error}")));
    }
    time::OffsetDateTime::parse(raw, &time::format_description::well_known::Rfc3339)
        .map_err(|_| bad_request("--since must be a duration like 7d or an RFC 3339 timestamp"))?;
    Ok(raw.to_owned())
}

/// Render the server's existing `items` array. `document` is an opaque server record, so it is not reinterpreted here.
pub fn render(body: &Value) -> String {
    let mut lines = vec!["What your account did — audit log".to_owned()];
    if let Some(since) = body.get("since").and_then(Value::as_str) {
        lines.push(format!("since {since}"));
    }
    let Some(items) = body.get("items").and_then(Value::as_array) else {
        lines.push("  (no entries)".to_owned());
        return lines.join("\n");
    };
    if items.is_empty() {
        lines.push("  (no entries)".to_owned());
    }
    for item in items {
        let at = item
            .get("created_at")
            .and_then(Value::as_str)
            .unwrap_or("unknown time");
        let action = item
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or("unknown action");
        let subject_type = item
            .get("subject_type")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let subject_id = item
            .get("subject_id")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        lines.push(format!("  {at}  {action}  {subject_type}/{subject_id}"));
    }
    lines.push("  audit_log records the account actor, not the token used".to_owned());
    lines.join("\n")
}

pub async fn run(cli: &crate::Cli, args: &AuditArgs) -> Result<(), Failure> {
    let limit = args.limit.clamp(1, 200);
    let since = since_rfc3339(&args.since, time::OffsetDateTime::now_utc())?;
    let credential = if let Some(token) = &cli.token {
        Credential::Bearer(token.clone())
    } else if let Some(cookie) = &cli.cookie {
        Credential::Cookie(cookie.clone())
    } else {
        return Err(auth_required(
            "dh audit reads your own private log: set DARKHOLD_TOKEN or DARKHOLD_COOKIE",
        ));
    };
    let base = cli.base_url.trim_end_matches('/');
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|error| environment(format!("http client: {error}")))?;
    let url = format!(
        "{base}/api/v1/me/audit-log?since={}&limit={limit}",
        percent_encode(&since)
    );
    let (status, bytes) = crate::http::send(|| credential.apply(client.get(&url)), base).await?;
    let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    if !(200..300).contains(&status) {
        return Err(failure_for(status, &body, base));
    }
    if cli.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&body).unwrap_or_default()
        );
    } else {
        println!("{}", render(&body));
    }
    Ok(())
}

fn percent_encode(value: &str) -> String {
    value.replace(':', "%3A").replace('+', "%2B")
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

    #[test]
    fn relative_windows_are_resolved_before_they_reach_the_server() {
        let now = time::OffsetDateTime::parse(
            "2026-11-18T12:00:00Z",
            &time::format_description::well_known::Rfc3339,
        )
        .expect("fixed now");
        assert_eq!(
            since_rfc3339("7d", now).expect("seven days"),
            "2026-11-11T12:00:00Z"
        );
        assert_eq!(
            since_rfc3339("2026-11-01T00:00:00Z", now).expect("absolute"),
            "2026-11-01T00:00:00Z"
        );
        assert!(since_rfc3339("7w", now).is_err());
    }
}

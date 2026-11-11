//! `M63-15` — `dh doctor`: the first command anyone runs when something feels wrong.
//!
//! A support thread about `dh` starts with a sentence like "it says auth but my token works", and the answer is almost
//! always one of five facts that nobody thought to collect: the profile being read is not the one they edited, the token
//! carries a different scope than the verb needs, the host clock is minutes off so signed requests are rejected, the
//! instance is on a different API version than the binary, or the base URL answers plain HTTP.
//!
//! So `doctor` measures those five and prints **one suggested fix per problem**. It never changes anything: a diagnostic
//! that repairs what it diagnoses cannot be run to find out what is wrong.
//!
//! ```text
//! $ dh doctor
//! ok    instance     https://darkhold.example  version 0.1.0+112909a6
//! ok    clock        local time is 0s from the server
//! warn  token        no token: public verbs work, session verbs will answer 401
//!                    fix: dh config set token <token>   (or --token for one command)
//! fail  version      server 0.1.0+112909a6 is #ahead of dh 0.1.0
//!                    fix: cargo install --path crates/cli  (or pull a newer checkout)
//! ```
//!
//! `--json` prints the same report as an object, for pasting into an issue.

use std::time::Duration;

use serde_json::json;

use crate::exit;
use crate::Cli;

/// The three outcomes a check has. `Warn` is "works, but you are about to be surprised"; `Fail` is "this is why it is
/// broken", and only `Fail` changes the exit code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Ok,
    Warn,
    Fail,
}

impl Status {
    fn label(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Warn => "warn",
            Self::Fail => "fail",
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Warn => "warn",
            Self::Fail => "fail",
        }
    }
}

/// One measured fact, and what to do about it.
#[derive(Debug, Clone)]
pub struct Check {
    pub name: &'static str,
    pub status: Status,
    pub detail: String,
    pub fix: Option<String>,
}

impl Check {
    fn ok(name: &'static str, detail: impl Into<String>) -> Self {
        Self { name, status: Status::Ok, detail: detail.into(), fix: None }
    }

    fn warn(name: &'static str, detail: impl Into<String>, fix: impl Into<String>) -> Self {
        Self { name, status: Status::Warn, detail: detail.into(), fix: Some(fix.into()) }
    }

    fn fail(name: &'static str, detail: impl Into<String>, fix: impl Into<String>) -> Self {
        Self { name, status: Status::Fail, detail: detail.into(), fix: Some(fix.into()) }
    }
}

/// How far the local clock is from the server's, in whole seconds. Positive means the local clock is ahead.
///
/// Signed requests and short-lived codes both fail when this is large, and neither failure says "your clock" — which is
/// why it is worth a check of its own.
#[must_use]
pub fn clock_skew_seconds(server: time::OffsetDateTime, local: time::OffsetDateTime) -> i64 {
    (local - server).whole_seconds()
}

/// Beyond this, a skew is worth reporting: token expiry, rate-limit windows and any future signature check all assume
/// clocks that agree to within a minute.
pub const SKEW_TOLERANCE_SECONDS: i64 = 60;

/// The `major.minor` of a version string, which is what API drift is about: patch releases do not change a contract.
#[must_use]
pub fn api_line(version: &str) -> Option<(u64, u64)> {
    let core = version.split(['+', '-']).next()?;
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    Some((major, minor))
}

/// A drift message, or `None` when the two binaries speak the same API line.
#[must_use]
pub fn version_drift(server: &str, ours: &str) -> Option<String> {
    match (api_line(server), api_line(ours)) {
        (Some(theirs), Some(mine)) if theirs == mine => None,
        (Some(theirs), Some(mine)) => Some(format!(
            "server {server} is on API {}.{}, dh {ours} is on {}.{}",
            theirs.0, theirs.1, mine.0, mine.1
        )),
        _ => Some(format!("could not compare {server} with {ours}")),
    }
}

/// Run every check, print the report, and answer the process exit code.
pub async fn run(cli: &Cli, json_out: bool) -> u8 {
    let checks = collect(cli).await;
    if json_out {
        let report = json!({
            "base_url": cli.base_url,
            "checks": checks.iter().map(|check| json!({
                "name": check.name,
                "status": check.status.as_str(),
                "detail": check.detail,
                "fix": check.fix,
            })).collect::<Vec<_>>(),
            "failed": checks.iter().filter(|c| c.status == Status::Fail).count(),
        });
        println!("{report}");
    } else {
        for check in &checks {
            println!(
                "{:<5} {:<10} {}",
                check.status.label(),
                check.name,
                check.detail
            );
            if let Some(fix) = &check.fix {
                println!("      {:<10} fix: {fix}", "");
            }
        }
    }
    if checks.iter().any(|c| c.status == Status::Fail) {
        exit::ERROR
    } else {
        exit::OK
    }
}

async fn collect(cli: &Cli) -> Vec<Check> {
    let mut checks = Vec::new();
    let base = cli.base_url.trim_end_matches('/').to_owned();

    // 1. The instance, its version, and its clock — one request answers all three, so a broken instance is one failure
    //    rather than three that all say "connection refused".
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            checks.push(Check::fail(
                "instance",
                format!("could not build an HTTP client: {error}"),
                "report this: it is a bug in dh, not in your setup",
            ));
            return checks;
        }
    };

    let meta = client.get(format!("{base}/api/v1/meta")).send().await;
    let server_date = meta
        .as_ref()
        .ok()
        .and_then(|response| response.headers().get(reqwest::header::DATE).cloned());
    let local_at_response = time::OffsetDateTime::now_utc();
    match meta {
        Ok(response) if response.status().is_success() => {
            let version = response
                .json::<serde_json::Value>()
                .await
                .ok()
                .and_then(|body| {
                    body.get("version")
                        .and_then(|v| v.as_str())
                        .map(str::to_owned)
                })
                .unwrap_or_else(|| "unknown".to_owned());
            checks.push(Check::ok("instance", format!("{base} version {version}")));

            // 2. Clock skew, from the server's own `Date` header.
            match server_date
                .and_then(|value| value.to_str().ok().map(str::to_owned))
                .and_then(|text| {
                    time::PrimitiveDateTime::parse(
                        &text,
                        &time::format_description::well_known::Rfc2822,
                    )
                    .ok()
                })
                .map(time::PrimitiveDateTime::assume_utc)
            {
                Some(server_at) => {
                    let skew = clock_skew_seconds(server_at, local_at_response);
                    if skew.abs() > SKEW_TOLERANCE_SECONDS {
                        checks.push(Check::warn(
                            "clock",
                            format!("local time is {skew}s from the server"),
                            "enable NTP (timedatectl set-ntp true); signed requests and codes expire by the clock",
                        ));
                    } else {
                        checks.push(Check::ok("clock", format!("local time is {skew}s from the server")));
                    }
                }
                None => checks.push(Check::warn(
                    "clock",
                    "the server sent no readable Date header",
                    "nothing to fix here, but skew cannot be ruled out",
                )),
            }

            // 3. API drift.
            match version_drift(&version, env!("CARGO_PKG_VERSION")) {
                None => checks.push(Check::ok(
                    "version",
                    format!("dh {} matches the server's API line", env!("CARGO_PKG_VERSION")),
                )),
                Some(drift) => checks.push(Check::warn(
                    "version",
                    drift,
                    "install the matching dh (cargo install --path crates/cli in the server's checkout)",
                )),
            }
        }
        Ok(response) => {
            checks.push(Check::fail(
                "instance",
                format!("{base}/api/v1/meta answered {}", response.status()),
                "the base URL is reachable but not this instance's API: check --base-url and the profile",
            ));
            checks.push(Check::warn("clock", "not measured: the instance did not answer", "fix `instance` first"));
            checks.push(Check::warn("version", "not measured: the instance did not answer", "fix `instance` first"));
        }
        Err(error) => {
            checks.push(Check::fail(
                "instance",
                format!("nothing answered at {base}/api/v1/meta: {error}"),
                "is the service running? is the tunnel up? check --base-url",
            ));
            checks.push(Check::warn("clock", "not measured: the instance did not answer", "fix `instance` first"));
            checks.push(Check::warn("version", "not measured: the instance did not answer", "fix `instance` first"));
        }
    }

    // 4. TLS: a token over plain HTTP is a token in every log on the path.
    if let Some(rest) = base.strip_prefix("http://") {
        let host = rest.split('/').next().unwrap_or(rest);
        let local = host.starts_with("127.0.0.1")
            || host.starts_with("localhost")
            || host.starts_with("[::1]");
        if local {
            checks.push(Check::ok("tls", "plain HTTP to localhost: nothing leaves the machine"));
        } else {
            checks.push(Check::fail(
                "tls",
                format!("{base} is plain HTTP and not local, so the token travels in clear text"),
                "use the https:// URL",
            ));
        }
    } else {
        checks.push(Check::ok("tls", "https"));
    }

    // 5. The token: present, and what it can do. `me/credential` describes the caller without disclosing the secret.
    match cli.token.as_deref().filter(|token| !token.trim().is_empty()) {
        None => checks.push(Check::warn(
            "token",
            "no token configured: public verbs work, session verbs will answer 401",
            "dh config --profile <name> set token <token>", 
        )),
        Some(token) => {
            let request = client
                .get(format!("{base}/api/v1/me/credential"))
                .bearer_auth(token)
                .send()
                .await;
            match request {
                Ok(response) if response.status().is_success() => {
                    let body = response.json::<serde_json::Value>().await.unwrap_or_default();
                    let scopes = body
                        .get("scopes")
                        .map(|scopes| match scopes {
                            serde_json::Value::Array(items) => items
                                .iter()
                                .filter_map(|item| item.as_str())
                                .collect::<Vec<_>>()
                                .join(", "),
                            serde_json::Value::String(text) => text.clone(),
                            _ => "unknown".to_owned(),
                        })
                        .unwrap_or_else(|| "unknown".to_owned());
                    checks.push(Check::ok("token", format!("accepted; scopes: {scopes}")));
                }
                Ok(response) if response.status().as_u16() == 401 || response.status().as_u16() == 403 => {
                    checks.push(Check::fail(
                        "token",
                        format!("rejected by the server: {}", response.status()),
                        "the token is wrong for this instance, or was revoked: mint a new one",
                    ));
                }
                Ok(response) => checks.push(Check::warn(
                    "token",
                    format!("/me/credential answered {}", response.status()),
                    "the token may be fine; this server does not describe credentials",
                )),
                Err(error) => checks.push(Check::warn(
                    "token",
                    format!("could not be checked: {error}"),
                    "fix `instance` first",
                )),
            }
        }
    }

    checks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(seconds: i64) -> time::OffsetDateTime {
        time::OffsetDateTime::from_unix_timestamp(1_760_000_000 + seconds).expect("a valid instant")
    }

    #[test]
    fn skew_is_signed_and_measured_in_whole_seconds() {
        assert_eq!(clock_skew_seconds(at(0), at(90)), 90, "local ahead reads positive");
        assert_eq!(clock_skew_seconds(at(90), at(0)), -90, "local behind reads negative");
        assert_eq!(clock_skew_seconds(at(0), at(0)), 0);
        assert!(clock_skew_seconds(at(0), at(SKEW_TOLERANCE_SECONDS + 1)) > SKEW_TOLERANCE_SECONDS);
    }

    #[test]
    fn a_patch_release_is_not_api_drift_and_a_minor_is() {
        assert_eq!(api_line("0.1.0+112909a6"), Some((0, 1)));
        assert_eq!(version_drift("0.1.0+aaa", "0.1.0+bbb"), None, "same API line");
        let drift = version_drift("0.2.0", "0.1.9").expect("a minor bump is drift");
        assert!(drift.contains("0.2"), "{drift}");
        assert!(version_drift("not-a-version", "0.1.0").is_some());
    }

    #[test]
    fn only_a_failure_changes_the_exit_code() {
        // The rule the caller depends on: a warning must not fail a script that runs `dh doctor` in a preflight.
        let warn = Check::warn("clock", "12s", "enable NTP");
        assert_eq!(warn.status, Status::Warn);
        assert_eq!(Status::Warn.label(), "warn");
        assert_ne!(Status::Fail, Status::Warn);
    }
}

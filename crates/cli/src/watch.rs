//! M63-12: `dh watch` — a debounced polling daemon for a saved query.
//!
//! The server owns the query: this program only knows the saved view's name and the query string the server stored.
//! A poll is the set of work ids the server returned; an added or removed id is a change, a reordered response is not.
//! A change is held until it has survived `--debounce`; then one `--exec` command and one notification are emitted.
//!
//! Starting a daemon and running a shell command are side effects, so the global default applies: without `--yes`
//! this prints a plan and sends nothing. `--once` is the operator's single-poll form.

use std::collections::BTreeSet;
use std::process::Command as ProcessCommand;
use std::time::{Duration, Instant};

use clap::{Args, ValueEnum};
use serde_json::Value;

use crate::rec::{bad_request, environment, failure_for, Failure};

/// Where one change is sent. The sinks are names, not a provider SDK: the two HTTP ones use the CLI's existing
/// `reqwest` dependency and the desktop one uses the desktop's own `notify-send`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Sink {
    Ntfy,
    Desktop,
    Webhook,
}

impl Sink {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ntfy => "ntfy",
            Self::Desktop => "desktop",
            Self::Webhook => "webhook",
        }
    }
}

/// `dh watch <saved> --exec … --notify …`
#[derive(Debug, Clone, Args)]
pub struct WatchArgs {
    /// The saved query to poll. `dh rec --queries` lists the names.
    pub saved: String,

    /// Command to run once per debounced change. Executed by the shell; no command is run without `--yes`.
    #[arg(long, value_name = "COMMAND")]
    pub exec: String,

    /// Where to send one short change message.
    #[arg(long, value_enum)]
    pub notify: Sink,

    /// Poll interval: `30s`, `10m`, `6h`, `1d`.
    #[arg(long, value_name = "DURATION", default_value = "10m")]
    pub every: String,

    /// Hold a change this long before acting on it, so a flapping query does not fire repeatedly.
    #[arg(long, value_name = "DURATION", default_value = "5m")]
    pub debounce: String,

    /// Poll once and exit. Useful for a test or a cron service that supplies its own schedule.
    #[arg(long)]
    pub once: bool,
}

/// A validated plan. Nothing here holds a credential or a server response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub saved: String,
    pub exec: String,
    pub notify: Sink,
    pub every: Duration,
    pub debounce: Duration,
    pub once: bool,
}

impl Plan {
    /// Print the plan in the same shape for dry-run and for the first line of a live run.
    pub fn show(&self) {
        println!(
            "would watch \"{}\" every {}s; a change held {}s runs `{}` and notifies {}",
            self.saved,
            self.every.as_secs(),
            self.debounce.as_secs(),
            self.exec,
            self.notify.as_str()
        );
        if self.once {
            println!("one poll only (--once)");
        }
    }
}

/// Parse a positive `Nh/Nm/Ns/Nd` duration. Zero and unknown units are refused by name.
pub fn parse_duration(raw: &str, flag: &str) -> Result<Duration, Failure> {
    if raw.is_empty() {
        return Err(bad_request(format!("{flag} is empty")));
    }
    let (digits, unit) = raw.split_at(raw.len() - 1);
    let amount: u64 = digits.parse().map_err(|_| {
        bad_request(format!(
            "{flag} must be a positive number followed by s, m, h or d: {raw}"
        ))
    })?;
    if amount == 0 {
        return Err(bad_request(format!("{flag} must be greater than zero")));
    }
    let seconds = match unit {
        "s" => amount,
        "m" => amount * 60,
        "h" => amount * 3_600,
        "d" => amount * 86_400,
        _ => {
            return Err(bad_request(format!(
                "{flag} uses an unknown unit `{unit}`; use s, m, h or d"
            )))
        }
    };
    Ok(Duration::from_secs(seconds))
}

/// Trailing-edge debounce: a first change is held; a later *different* set restarts the clock; the same
/// change repeated does not. Once the window has elapsed, the caller may act.
#[must_use]
pub fn debounce(pending_since: Option<Instant>, now: Instant, window: Duration) -> bool {
    pending_since.is_some_and(|started| now.duration_since(started) >= window)
}

/// Build a plan without touching the server or the shell.
pub fn plan(args: &WatchArgs) -> Result<Plan, Failure> {
    Ok(Plan {
        saved: args.saved.clone(),
        exec: args.exec.clone(),
        notify: args.notify,
        every: parse_duration(&args.every, "--every")?,
        debounce: parse_duration(&args.debounce, "--debounce")?,
        once: args.once,
    })
}

/// Dispatch. The global `--dry-run` and missing `--yes` both stop before a poll.
pub async fn run(cli: &crate::Cli, args: &WatchArgs) -> Result<(), Failure> {
    let plan = plan(args)?;
    if cli.dry_run || !cli.yes {
        plan.show();
        println!("start the daemon by adding --yes");
        return Ok(());
    }
    daemon(cli, &plan).await
}

async fn daemon(cli: &crate::Cli, plan: &Plan) -> Result<(), Failure> {
    let token = std::env::var("DARKHOLD_TOKEN").map_err(|_| {
        bad_request("DARKHOLD_TOKEN is not set; a saved view belongs to an account")
    })?;
    if token.trim().is_empty() {
        return Err(bad_request("DARKHOLD_TOKEN is empty"));
    }
    let base = cli.base_url.trim_end_matches('/');
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|error| environment(format!("http client: {error}")))?;

    let mut last: Option<BTreeSet<String>> = None;
    let mut pending: Option<(Instant, BTreeSet<String>)> = None;
    loop {
        let current = poll(&client, base, token.trim(), &plan.saved).await?;
        if last.as_ref() == Some(&current) {
            pending = None;
        } else {
            match &pending {
                None => pending = Some((Instant::now(), current.clone())),
                Some((_, seen)) if *seen != current => {
                    pending = Some((Instant::now(), current.clone()));
                }
                Some((started, _)) if debounce(Some(*started), Instant::now(), plan.debounce) => {
                    run_exec(&plan.exec)?;
                    deliver(&client, plan.notify, &plan.saved, current.len()).await?;
                    last = Some(current);
                    pending = None;
                }
                Some(_) => {}
            }
        }
        if plan.once {
            return Ok(());
        }
        tokio::time::sleep(plan.every).await;
    }
}

async fn poll(
    client: &reqwest::Client,
    base: &str,
    token: &str,
    name: &str,
) -> Result<BTreeSet<String>, Failure> {
    let views_url = format!("{base}/api/v1/saved-views");
    let (status, bytes) =
        crate::http::send(|| client.get(&views_url).bearer_auth(token), base).await?;
    let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    if !(200..300).contains(&status) {
        return Err(failure_for(status, &body, base));
    }
    let view = body
        .get("items")
        .and_then(Value::as_array)
        .and_then(|items| {
            items
                .iter()
                .find(|item| item.get("name").and_then(Value::as_str) == Some(name))
        })
        .ok_or_else(|| bad_request(format!("no saved view named \"{name}\"")))?;
    let params = view
        .get("params")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let query_url = format!("{base}/api/v1/catalogue?{params}");
    let (status, bytes) =
        crate::http::send(|| client.get(&query_url).bearer_auth(token), base).await?;
    let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    if !(200..300).contains(&status) {
        return Err(failure_for(status, &body, base));
    }
    let rows = body
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Ok(rows
        .iter()
        .filter_map(|row| row.get("id").and_then(Value::as_str))
        .map(str::to_owned)
        .collect())
}

fn run_exec(command: &str) -> Result<(), Failure> {
    let result = if cfg!(windows) {
        ProcessCommand::new("cmd").args(["/C", command]).status()
    } else {
        ProcessCommand::new("sh").args(["-c", command]).status()
    };
    match result {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(environment(format!("--exec exited with {status}"))),
        Err(error) => Err(environment(format!("--exec could not start: {error}"))),
    }
}

async fn deliver(
    client: &reqwest::Client,
    sink: Sink,
    saved: &str,
    rows: usize,
) -> Result<(), Failure> {
    let message = format!("dh watch: \"{saved}\" changed ({rows} works)");
    match sink {
        Sink::Desktop => {
            let status = ProcessCommand::new("notify-send")
                .arg("dh watch")
                .arg(&message)
                .status();
            match status {
                Ok(status) if status.success() => Ok(()),
                Ok(status) => Err(environment(format!("notify-send exited with {status}"))),
                Err(error) => Err(environment(format!("notify-send is unavailable: {error}"))),
            }
        }
        Sink::Ntfy => {
            let topic = std::env::var("DARKHOLD_NTFY_TOPIC")
                .map_err(|_| bad_request("DARKHOLD_NTFY_TOPIC is not set; ntfy needs a topic"))?;
            let root =
                std::env::var("DARKHOLD_NTFY_URL").unwrap_or_else(|_| "https://ntfy.sh".to_owned());
            let url = format!("{}/{}", root.trim_end_matches('/'), topic.trim());
            let response = client.post(&url).body(message).send().await;
            match response {
                Ok(response) if response.status().is_success() => Ok(()),
                Ok(response) => Err(environment(format!(
                    "ntfy returned {}",
                    response.status().as_u16()
                ))),
                Err(_) => Err(environment("ntfy delivery failed")),
            }
        }
        Sink::Webhook => {
            let url = std::env::var("DARKHOLD_WATCH_WEBHOOK_URL").map_err(|_| {
                bad_request("DARKHOLD_WATCH_WEBHOOK_URL is not set; webhook needs a destination")
            })?;
            let response = client
                .post(url.trim())
                .json(&serde_json::json!({ "text": message }))
                .send()
                .await;
            match response {
                Ok(response) if response.status().is_success() => Ok(()),
                Ok(response) => Err(environment(format!(
                    "webhook returned {}",
                    response.status().as_u16()
                ))),
                Err(_) => Err(environment("webhook delivery failed")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_unit_is_parsed_and_zero_is_refused() {
        assert_eq!(parse_duration("30s", "--every").unwrap().as_secs(), 30);
        assert_eq!(parse_duration("2m", "--every").unwrap().as_secs(), 120);
        assert_eq!(parse_duration("6h", "--every").unwrap().as_secs(), 21_600);
        assert_eq!(parse_duration("1d", "--every").unwrap().as_secs(), 86_400);
        for bad in ["0s", "", "5x", "m"] {
            assert!(parse_duration(bad, "--every").is_err(), "{bad}");
        }
    }

    #[test]
    fn a_change_is_held_until_the_debounce_window_elapses() {
        let now = Instant::now();
        let started = now - Duration::from_secs(3);
        assert!(!debounce(None, now, Duration::from_secs(5)));
        assert!(!debounce(Some(started), now, Duration::from_secs(5)));
        assert!(debounce(Some(started), now, Duration::from_secs(2)));
    }
}

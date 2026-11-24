//! The automatic work-lifecycle sweep at the terminal (M45-76).
//!
//! ```text
//! dh lifecycle sweep                        inspect one page, sends no write
//! dh lifecycle sweep --apply --all          apply every page, following cursors
//! dh lifecycle sweep --apply --limit 10     apply one bounded page
//! ```
//!
//! The route decides freshness, trust and provenance. This module sends the
//! typed flags and prints the report, using the same cookie and exit codes as
//! the other operator doors.

use clap::{Args, Subcommand};
use serde_json::{json, Value};

use crate::rec::{auth_required, bad_request, failure_for, Failure};
use crate::Cli;

#[derive(Debug, Clone, Subcommand)]
pub enum LifecycleArgs {
    /// Inspect or apply the paged automatic sweep.
    Sweep(SweepArgs),
}

#[derive(Debug, Clone, Args)]
pub struct SweepArgs {
    /// Send a write instead of the default dry run.
    #[arg(long)]
    pub apply: bool,
    /// Keep following `next_cursor` until the server reports `done`.
    #[arg(long)]
    pub all: bool,
    /// Page size; the server clamps to 1..=500.
    #[arg(long, default_value_t = 500)]
    pub limit: i64,
}

fn field(value: &Value, name: &str) -> i64 {
    value.get(name).and_then(Value::as_i64).unwrap_or(0)
}

fn flag(value: &Value, name: &str) -> bool {
    value.get(name).and_then(Value::as_bool).unwrap_or(false)
}

async fn post(
    cli: &Cli,
    client: &reqwest::Client,
    cookie: &str,
    body: &Value,
) -> Result<Value, Failure> {
    let url = format!("{}/api/v1/lifecycle/sweep", cli.base_url);
    let (status, bytes) = crate::http::send(
        || {
            client
                .post(&url)
                .header(reqwest::header::COOKIE, cookie)
                .json(body)
        },
        &cli.base_url,
    )
    .await?;
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    if !(200..300).contains(&status) {
        return Err(failure_for(status, &value, &cli.base_url));
    }
    Ok(value)
}

pub async fn run(cli: &Cli, action: &LifecycleArgs) -> Result<(), Failure> {
    match action {
        LifecycleArgs::Sweep(args) => sweep(cli, args).await,
    }
}

async fn sweep(cli: &Cli, args: &SweepArgs) -> Result<(), Failure> {
    if !(1..=500).contains(&args.limit) {
        return Err(bad_request(
            "--limit must be between 1 and 500; the server clamps larger pages",
        ));
    }
    if args.apply && cli.dry_run {
        return Err(bad_request(
            "--apply and --dry-run contradict each other; use --apply alone to write",
        ));
    }
    let cookie = cli.cookie.as_deref().ok_or_else(|| {
        auth_required(
            "dh lifecycle sweep needs an operator session: pass --cookie or set DARKHOLD_COOKIE from a signed-in browser",
        )
    })?;
    let client = reqwest::Client::new();
    let mut cursor: Option<String> = None;

    loop {
        let body = json!({
            "dry_run": !args.apply,
            "limit": args.limit,
            "cursor": cursor,
        });
        let value = post(cli, &client, cookie, &body).await?;
        if cli.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string())
            );
        } else {
            println!(
                "scanned={} abandoned={} revived={} concluded={} raced={} unmeasured={} done={} dry_run={}",
                field(&value, "scanned"),
                field(&value, "abandoned"),
                field(&value, "revived"),
                field(&value, "concluded"),
                field(&value, "raced"),
                field(&value, "unmeasured"),
                flag(&value, "done"),
                flag(&value, "dry_run"),
            );
        }
        let done = flag(&value, "done");
        if done || !args.all {
            break;
        }
        match value.get("next_cursor").and_then(Value::as_str) {
            Some(next) => cursor = Some(next.to_owned()),
            None => break,
        }
    }
    Ok(())
}

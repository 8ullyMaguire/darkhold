//! `dh admin` — the operator's microscope (M63-22).
//!
//! The CLI repeats no ranking logic. It asks `/api/v1/admin/discovery/explain-rec` with a session cookie and prints
//! what came back: a bearer is a different principal and would be refused by a session door, and a locally computed
//! table would drift from the blend the server actually used.

use clap::{Args, Subcommand};
use serde_json::Value;

use crate::rec::{auth_required, bad_request, environment, failure_for, Failure};
use crate::Cli;

/// What an operator may ask about.
#[derive(Debug, Clone, Subcommand)]
pub enum AdminAction {
    /// One work, through every registered strategy, for one account.
    ExplainRec {
        /// The work id, as the server's API spells it.
        work: String,
        /// The account whose taste the trace is for.
        #[arg(long)]
        account: String,
    },
}

#[derive(Debug, Args)]
pub struct AdminArgs {
    #[command(subcommand)]
    pub action: AdminAction,
}

pub async fn run(cli: &Cli, args: AdminArgs) -> Result<(), Failure> {
    match args.action {
        AdminAction::ExplainRec { work, account } => explain_rec(cli, &work, &account).await,
    }
}

async fn explain_rec(cli: &Cli, work: &str, account: &str) -> Result<(), Failure> {
    // The route is session-gated and this is an operator's view of one account's taste. A bearer token is a
    // different principal with no browser session; sending one produces the 401 that sends people looking at
    // the wrong layer.
    let cookie = cli.cookie.as_deref().ok_or_else(|| {
        auth_required(
            "explain-rec reads an account's taste: set DARKHOLD_COOKIE to an operator session",
        )
    })?;
    if work.trim().is_empty() {
        return Err(bad_request("work must not be empty"));
    }
    if account.trim().is_empty() {
        return Err(bad_request("account must not be empty"));
    }
    let base = cli.base_url.trim_end_matches('/');
    // Build the query with the URL parser rather than string concatenation: an id typed with a space or an ampersand
    // must not become a second parameter.
    let mut url = reqwest::Url::parse(&format!("{base}/api/v1/admin/discovery/explain-rec"))
        .map_err(|error| environment(format!("bad base URL: {error}")))?;
    url.query_pairs_mut()
        .append_pair("account", account)
        .append_pair("work", work);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|error| environment(format!("http client: {error}")))?;
    let (status, bytes) = crate::http::send(
        || {
            client
                .get(url.clone())
                .header(reqwest::header::COOKIE, cookie)
        },
        base,
    )
    .await?;
    if !status.to_string().starts_with('2') {
        let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        return Err(failure_for(status, &body, base));
    }
    if cli.json {
        // Verbatim: `--json` is a pipe into jq, not a reformatting.
        use std::io::Write;
        let mut out = std::io::stdout().lock();
        out.write_all(&bytes)
            .map_err(|e| environment(format!("writing output: {e}")))?;
        let _ = out.write_all(b"\n");
        return Ok(());
    }
    let body: Value = serde_json::from_slice(&bytes)
        .map_err(|error| environment(format!("the server did not send JSON: {error}")))?;
    println!("work {}   account {}", body["work"], body["account"]);
    println!(
        "limit {}   blended position {}",
        body["limit"], body["blended_position"]
    );
    println!();
    println!(
        "{:<22} {:>10} {:>7} {:>12}",
        "STRATEGY", "PRODUCED", "RANK", "RRF"
    );
    let empty: Vec<Value> = Vec::new();
    for strategy in body["strategies"].as_array().unwrap_or(&empty) {
        // A dash, never a zero: a strategy that never returned the work did not score it as worthless.
        let rank = match &strategy["rank"] {
            Value::Number(n) => n.to_string(),
            _ => "-".to_owned(),
        };
        let rrf = match &strategy["rrf"].as_f64() {
            Some(n) => format!("{n:.4}"),
            None => "-".to_owned(),
        };
        println!(
            "{:<22} {:>10} {:>7} {:>12}",
            strategy["name"].as_str().unwrap_or("?"),
            strategy["produced"].as_u64().unwrap_or(0),
            rank,
            rrf
        );
    }
    println!();
    let f = &body["features"];
    println!(
        "features: words={} chapters={} kudos={} hits={} bookmarks={} comments={} rating={} complete={} age_days={:.1}",
        f["words"], f["chapters"], f["kudos"], f["hits"], f["bookmarks"], f["comments"],
        f["rating"], f["complete"], f["age_days"].as_f64().unwrap_or(0.0)
    );
    println!(
        "tags: {}",
        f["tags"].as_array().map_or_else(
            || "-".to_owned(),
            |t| t
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        )
    );
    Ok(())
}

//! `dh` has one job so far: take a question a power user asks at a terminal
//! ("naruto fics updated last week, by likes, using the rrf engine, and show me
//! why") and answer it without a browser.
//!
//! # The one design rule
//!
//! **Every rule lives on the server, and this binary repeats none of them.** The
//! window grammar, the metric vocabulary, the enabled engines, the suggestion for
//! a misspelt fandom — all of it is resolved by `/api/v1`. The CLI sends what was
//! typed and prints what came back, so a window typed here cannot mean something
//! different from the same window in a URL. That is also why `--sort wilson`
//! fails today with the server's own message rather than being mapped to `likes`:
//! a client-side alias table is a second definition waiting to disagree.
//!
//! **The exit codes are part of the API** (`M63-16`): `0` ok, `1` environment, `2` usage, `3` not
//! found, `4` auth, `5` rate-limited — defined once in [`exit`] and documented in
//! `docs/cli-exit-codes.md`, because a wrapper script decides what to do from that number alone.

mod exit;
mod gen;
mod http;
mod module;
mod output;
mod rec;
mod redact;
mod safety;
mod supply;

use clap::{Parser, Subcommand};

/// Query a Darkhold catalogue from a terminal.
#[derive(Debug, Parser)]
#[command(
    name = "dh",
    version,
    about = "Query a Darkhold catalogue from a terminal",
    long_about = None,
    propagate_version = true
)]
pub struct Cli {
    /// Base URL of the instance to talk to.
    #[arg(
        long,
        global = true,
        env = "DARKHOLD_BASE_URL",
        default_value = "http://127.0.0.1:8081",
        value_name = "URL"
    )]
    pub base_url: String,

    /// Session token, for the surfaces that read your own history.
    ///
    /// Only `--engine` needs it: the recommendation engines rank against what
    /// this account has already read, so they are not a public route. Without it
    /// a plain catalogue query still works.
    #[arg(long, global = true, env = "DARKHOLD_TOKEN", value_name = "TOKEN")]
    pub token: Option<String>,

    /// A session cookie, for the doors that act for an account.
    ///
    /// A bearer token is a different principal (`resolve_bearer`): it carries its
    /// own scopes and no browser session, so a route that decides per account —
    /// a module override is one — cannot be driven by it. Copy the
    /// `darkhold_session=…` pair from a signed-in browser, or from a `curl -c` jar.
    #[arg(long, global = true, env = "DARKHOLD_COOKIE", value_name = "COOKIE")]
    pub cookie: Option<String>,

    /// Print the server's JSON verbatim instead of a table.
    #[arg(long, global = true)]
    pub json: bool,

    /// Apply a change. Mutating verbs preview their request and exit 0 without it (`M63-21`).
    ///
    /// ```text
    /// dh module off arena            → would PATCH …/modules/arena {"enabled":false}
    /// dh module off arena --yes      → the request is sent
    /// ```
    #[arg(long, global = true)]
    pub yes: bool,

    /// Print what would happen and send nothing. The default for mutating verbs; say it explicitly
    /// in a generated script so the intent is visible.
    #[arg(long, global = true, conflicts_with = "yes")]
    pub dry_run: bool,

    #[command(subcommand)]
    pub command: Command,
}

/// What to do.
#[derive(Debug, Clone, Subcommand)]
pub enum Command {
    /// Recommend from the catalogue: a fandom, a window, a metric, an engine.
    ///
    /// The window grammar is the server's, not this program's:
    ///
    /// ```text
    ///   7d                        the last seven days
    ///   >30d                      older than thirty days
    ///   @2026-10-27               that whole day, UTC
    ///   2026-01-01..2026-10-15    between two days
    /// ```
    Rec(Box<rec::RecArgs>),
    /// §58 supply: keystone authors, droughts, and the routing you opt into.
    ///
    /// The operator doors (`keystones`, `droughts`, `recompute`) read a role the
    /// server holds; the reader doors (`route`, `routing`, `complete`) are keyed
    /// by your account. Every one of them needs a session.
    Supply {
        #[command(subcommand)]
        action: Box<supply::SupplyArgs>,
    },
    /// §59: what this instance runs, and what it could. `list` and `show` read the
    /// server; `check` validates a config file without one.
    Module {
        #[command(subcommand)]
        action: module::ModuleAction,
    },
    /// A completion script for your shell, generated from this parser (`M63-17`).
    ///
    /// ```text
    /// dh completions bash > ~/.local/share/bash-completion/completions/dh
    /// dh completions zsh  > ~/.zfunc/_dh
    /// dh completions fish > ~/.config/fish/completions/dh.fish
    /// ```
    Completions {
        /// Which shell: `bash`, `zsh`, `fish`, `elvish` or `powershell`.
        shell: clap_complete::Shell,
    },
    /// The man page, generated from this parser. `dh man | man -l -`
    Man,
    /// Strip ids, emails, session cookies and tokens from stdin, so a transcript can be pasted
    /// (`M63-21`). `dh rec … --json | dh redact`
    ///
    /// UUIDs become `uuid#1`, emails `email#1`, and so on, numbered by first appearance: two pastes
    /// of the same transcript redact identically, and a reader can still see that two lines name the
    /// same value without learning which. Fandom names, handles and titles are left alone — a
    /// redactor that guesses corrupts the output that makes a paste useful.
    Redact,
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    match &cli.command {
        Command::Rec(args) => match rec::run(&cli, args).await {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("{error}");
                std::process::ExitCode::from(error.code)
            }
        },
        Command::Redact => {
            use std::io::Read as _;
            let mut input = String::new();
            if let Err(error) = std::io::stdin().read_to_string(&mut input) {
                eprintln!("reading stdin: {error}");
                return std::process::ExitCode::from(exit::ERROR);
            }
            print!("{}", redact::redact(&input));
            std::process::ExitCode::SUCCESS
        }
        Command::Completions { shell } => {
            gen::completions(*shell);
            std::process::ExitCode::SUCCESS
        }
        Command::Man => {
            gen::man();
            std::process::ExitCode::SUCCESS
        }
        Command::Module { action } => match module::run(
            &cli,
            module::ModuleArgs {
                action: action.clone(),
            },
        )
        .await
        {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("{error}");
                std::process::ExitCode::from(error.code)
            }
        },
        Command::Supply { action } => match supply::run(&cli, action).await {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("{error}");
                std::process::ExitCode::from(error.code)
            }
        },
    }
}

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

mod bench;
mod cron;
mod doctor;
mod exit;
mod gen;
mod http;
mod module;
mod output;
mod profiles;
mod q;
mod rec;
mod redact;
mod safety;
mod supply;

use clap::{Parser, Subcommand};

use crate::cron::CronArgs;

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
    ///
    /// The default is empty on purpose: an empty value means "not given", which lets the selected
    /// profile supply the URL. `--base-url` (or `DARKHOLD_BASE_URL`) still wins over the profile, and
    /// the built-in `http://127.0.0.1:8081` is the last resort when neither names one.
    #[arg(
        long,
        global = true,
        env = "DARKHOLD_BASE_URL",
        default_value = "",
        value_name = "URL"
    )]
    pub base_url: String,

    /// The config profile to use: a named `(url, token, cookie)` triple in `config.toml`.
    ///
    /// ```text
    /// dh config set profiles.work.url https://work.example
    /// dh config set profiles.work.token <token>
    /// dh --profile work rec --engine personal
    /// ```
    ///
    /// Falls back to `default_profile` in the file, then to the built-in defaults. An unknown name is
    /// refused by name rather than silently using another instance's token.
    #[arg(long, global = true, env = "DARKHOLD_PROFILE", value_name = "NAME")]
    pub profile: Option<String>,

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
    /// Time the twenty canonical queries against an instance (M63-24).
    ///
    /// The shapes are fixed and public (`bench::CANONICAL`): one tag, a numeric floor, a conjunction, a negation, two
    /// recency windows, a phrase. What the command prints is a table of client-side milliseconds with a warm-up pass
    /// before the measurements, and the footer names the target and the load, because the same twenty queries against
    /// a quiet laptop and a busy box are different facts.
    Bench(bench::BenchArgs),
    /// Search with the shared query language: `tag:naruto AND kudos:>=100`.
    ///
    /// The same parser the site's search box uses, at `/api/v1/search`: one expression, the same
    /// meaning in a terminal as in a URL. The grammar is the server's —
    ///
    /// ```text
    ///   tag:naruto AND NOT tag:gore        fields, AND / OR / NOT, parentheses
    ///   kudos:>=100   words:>10000         numeric comparisons
    ///   updated>=-7d  published:>=2026-08-01   dates, absolute or as an age
    ///   completion:complete  status:published  the stored vocabulary
    /// ```
    ///
    /// A query the parser rejects is the server's `422` and exit code 2, naming the token.
    Q(Box<q::QArgs>),
    /// §58 supply: keystone authors, droughts, and the routing you opt into.
    ///
    /// The operator doors (`keystones`, `droughts`, `recompute`) read a role the
    /// server holds; the reader doors (`route`, `routing`, `complete`) are keyed
    /// by your account. Every one of them needs a session.
    Supply {
        #[command(subcommand)]
        action: Box<supply::SupplyArgs>,
    },
    /// Install a systemd user timer that runs a saved query on a schedule.
    ///
    /// `dh cron install --saved weekly --every 6h --jitter 10m` writes three files under
    /// `~/.config/darkhold/cron/`: the timer, the service, and a `0600` environment file holding
    /// a token that carries `library.read`. It does not run `systemctl`; it prints the two
    /// commands. `--dry-run` prints the units and writes nothing.
    Cron(CronArgs),
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
    /// Diagnose the connection before filing a bug: profile, token scope, clock skew, TLS and API version,
    /// with one suggested fix per problem.
    ///
    /// Never changes anything — a diagnostic that repairs what it diagnoses cannot be run to find out what is
    /// wrong. Exit code is `1` when a check fails and `0` when everything only warns.
    Doctor {
        /// Print the report as one JSON object, for pasting into an issue.
        #[arg(long)]
        json: bool,
    },
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
    /// Read and write the config file: profiles, their URLs, and which one is the default.
    ///
    /// ```text
    /// dh config path                          where the file is
    /// dh config get profiles                 the profile names
    /// dh config set profiles.work.url https://work.example
    /// dh config get profiles.work.token      `set (hidden, N chars)` — never the secret
    /// ```
    ///
    /// A token written here is stored `0600` and can never be read back in clear: the length and the
    /// profile name are what tell two entries apart, and a printed prefix is what leaks.
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
}

/// `dh config …`
#[derive(clap::Subcommand, Debug, Clone)]
pub enum ConfigAction {
    /// Print the config file's path.
    Path,
    /// Read one dotted key: `default_profile`, `profiles`, `profiles.<name>.<url|token|cookie>`.
    Get {
        /// The dotted key.
        key: String,
    },
    /// Write one dotted key, creating the profile if it does not exist.
    Set {
        /// The dotted key.
        key: String,
        /// The value.
        value: String,
    },
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let mut cli = Cli::parse();

    // M63-18: fold flags, environment and profile into the two values every request uses. `config`
    // is exempt because it edits the file that resolution reads — resolving first would refuse a
    // command that only wanted to fix the file the resolution complained about.
    let editing_config = matches!(cli.command, Command::Config { .. });
    if editing_config {
        let path = profiles::path();
        return match run_config(&path, &cli.config_action(), cli.profile.as_deref()) {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("{error}");
                std::process::ExitCode::from(2)
            }
        };
    }
    match profiles::resolve(
        &cli.base_url,
        cli.token.as_deref(),
        cli.cookie.as_deref(),
        cli.profile.as_deref(),
        &profiles::path(),
    ) {
        Ok(resolved) => {
            cli.base_url = resolved.url;
            cli.token = resolved.token;
            cli.cookie = resolved.cookie;
        }
        Err(error) => {
            // A named profile that does not exist is a refusal, not a reason to install a default:
            // the next request would carry the wrong instance's token.
            eprintln!("{error}");
            return std::process::ExitCode::from(2);
        }
    }

    match &cli.command {
        Command::Rec(args) => match rec::run(&cli, args).await {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("{error}");
                std::process::ExitCode::from(error.code)
            }
        },
        Command::Bench(args) => match bench::run(&cli, &args).await {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("{error}");
                std::process::ExitCode::from(error.code)
            }
        },
        Command::Q(args) => match q::run(&cli, args).await {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("{error}");
                std::process::ExitCode::from(error.code)
            }
        },
        // Handled before this match, because `config` edits the file that resolution reads.
        Command::Config { .. } => unreachable!("config is dispatched before resolution"),
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
        Command::Doctor { json } => std::process::ExitCode::from(doctor::run(&cli, *json).await),
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
        Command::Cron(args) => match cron::run(&cli, args).await {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("{error}");
                std::process::ExitCode::from(error.code)
            }
        },
    }
}

impl Cli {
    /// The `config` subcommand, cloned out so `run_config` does not have to borrow the whole CLI.
    fn config_action(&self) -> ConfigAction {
        match &self.command {
            Command::Config { action } => action.clone(),
            _ => ConfigAction::Path,
        }
    }
}

/// `dh config path|get|set` — the only command that edits the file resolution reads.
fn run_config(
    path: &std::path::Path,
    action: &ConfigAction,
    profile: Option<&str>,
) -> anyhow::Result<()> {
    match action {
        ConfigAction::Path => {
            println!("{}", path.display());
            // Which profile will actually be used is the other half of "where is my config": a file
            // with four profiles and a `default_profile` answers nothing on its own.
            let resolved = profiles::resolve("", None, None, profile, path)?;
            match resolved.profile {
                Some(name) => println!("profile: {name}"),
                None => println!("profile: (none; using the built-in default)"),
            }
            Ok(())
        }
        ConfigAction::Get { key } => {
            let config = profiles::load(path)?;
            println!("{}", profiles::get(&config, key)?);
            Ok(())
        }
        ConfigAction::Set { key, value } => {
            let mut config = profiles::load(path)?;
            profiles::set(&mut config, key, value)?;
            profiles::save(path, &config)?;
            // Name what was written, and for a secret name only that it was written: an echo of the
            // value would put it in the shell's scrollback, which is the leak `get` is careful about.
            if key.ends_with(".token") || key.ends_with(".cookie") {
                println!("{key}: set (hidden)");
            } else {
                println!("{key}: {value}");
            }
            Ok(())
        }
    }
}

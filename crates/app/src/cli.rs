//! Command-line surface.
//!
//! Spec §5 names four commands — `serve`, `migrate`, `seed --development`,
//! `doctor` — and requires the configuration precedence
//! **argument → environment → file → default**.
//!
//! That precedence falls out of clap for free: every global option declares an
//! `env`, so clap resolves "argument, else environment" into one `Option`. The
//! configuration file then only fills the gaps that remain, and the defaults
//! fill what is left. Both higher layers therefore always win, which is exactly
//! the documented order.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

/// Darkhold — a self-hosted fanfiction archive and community platform.
#[derive(Debug, Parser)]
#[command(
    name = "darkhold",
    version,
    about = "Darkhold — a self-hosted fanfiction archive and community platform",
    long_about = None,
    propagate_version = true
)]
pub struct Cli {
    /// Options shared by every subcommand.
    #[command(flatten)]
    pub global: GlobalArgs,

    /// What to do.
    #[command(subcommand)]
    pub command: Command,
}

/// Options accepted by every subcommand, in precedence order.
#[derive(Debug, Clone, Default, Args)]
pub struct GlobalArgs {
    /// Path to the TOML configuration file.
    #[arg(
        long,
        short = 'c',
        global = true,
        env = "DARKHOLD_CONFIG",
        value_name = "PATH"
    )]
    pub config: Option<PathBuf>,

    /// Runtime environment: development, test or production.
    #[arg(long, global = true, env = "DARKHOLD_ENV", value_name = "ENV")]
    pub environment: Option<String>,

    /// Address to bind the HTTP listener to.
    #[arg(long, global = true, env = "DARKHOLD_BIND", value_name = "ADDR")]
    pub bind: Option<String>,

    /// TCP port to listen on.
    #[arg(long, global = true, env = "DARKHOLD_PORT", value_name = "PORT")]
    pub port: Option<u16>,

    /// Database URL, e.g. `sqlite://./data/darkhold.sqlite?mode=rwc`
    /// or `postgres://user:pass@host/db`.
    #[arg(long, global = true, env = "DARKHOLD_DATABASE_URL", value_name = "URL")]
    pub database_url: Option<String>,

    /// Root directory for uploaded and generated files.
    #[arg(
        long,
        global = true,
        env = "DARKHOLD_STORAGE_ROOT",
        value_name = "PATH"
    )]
    pub storage_root: Option<PathBuf>,

    /// Public base URL of the instance, used for links and feeds.
    #[arg(long, global = true, env = "DARKHOLD_BASE_URL", value_name = "URL")]
    pub base_url: Option<String>,

    /// Tracing filter, e.g. `info,darkhold_db=debug`.
    #[arg(long, global = true, env = "DARKHOLD_LOG", value_name = "FILTER")]
    pub log: Option<String>,

    /// Account allowed to reach the `/admin` routes, until Milestone 13 builds
    /// the trust model that replaces this.
    #[arg(
        long,
        global = true,
        env = "DARKHOLD_OPERATOR_ACCOUNT_ID",
        value_name = "ACCOUNT_ID"
    )]
    pub operator_account_id: Option<String>,

    /// Log output format: `pretty` or `json`.
    #[arg(
        long,
        global = true,
        env = "DARKHOLD_LOG_FORMAT",
        value_name = "FORMAT"
    )]
    pub log_format: Option<String>,
}

/// The available subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Start the HTTP server.
    Serve(ServeArgs),

    /// Apply or inspect database migrations.
    Migrate(MigrateArgs),

    /// Populate a development instance with sample data, or seed from a list.
    Seed(SeedArgs),

    /// Check configuration, database, storage and converters.
    Doctor(DoctorArgs),

    /// Run the background worker: jobs and outbox delivery.
    Worker(WorkerArgs),

    /// Queue the recurring maintenance work and exit. For cron.
    ///
    /// The tasks are idempotent and each reads only what is due, so running this
    /// more often than needed costs a query and nothing else. It exists because
    /// retention is a promise — an export is kept seven days and then removed —
    /// and a promise nothing enqueues is not kept.
    Maintain(MaintainArgs),

    /// Local library tools: offline mirrors, CSV tag loads and diffs.
    Lib(LibArgs),
}

/// Local library tools.
#[derive(Debug, Args)]
pub struct LibArgs {
    #[command(subcommand)]
    pub command: LibCommand,
}

/// One local-library verb.
#[derive(Debug, Subcommand)]
pub enum LibCommand {
    /// Tag works from a two-column `work_id,tag` CSV.
    Tag(TagArgs),

    /// Pull changed library and catalogue rows into a local SQLite mirror.
    Sync(SyncArgs),
}

/// Options for `lib sync`.
#[derive(Debug, Args)]
pub struct SyncArgs {
    /// Path to the derived SQLite mirror file.
    #[arg(long)]
    pub to: PathBuf,

    /// Only rows changed at or after this RFC 3339 timestamp.
    #[arg(long)]
    pub since: Option<String>,

    /// Restrict the library pull to one account.
    #[arg(long)]
    pub account: Option<String>,
}

/// Options for `lib tag`.
#[derive(Debug, Args)]
pub struct TagArgs {
    /// CSV with a `work_id,tag` header and one pair per row.
    #[arg(long)]
    pub from_csv: PathBuf,

    /// Where to write the SQL that undoes rows this run applied.
    #[arg(long)]
    pub rollback: Option<PathBuf>,
}

/// Options for `maintain`.
#[derive(Debug, Args)]
pub struct MaintainArgs {
    /// List the tasks that would be queued, and queue nothing.
    #[arg(long)]
    pub dry_run: bool,
}

/// Options for `serve`.
#[derive(Debug, Args)]
pub struct ServeArgs {
    /// Never apply migrations on startup, even outside production.
    #[arg(long)]
    pub no_migrate: bool,

    /// Run the background worker in this process as well as the HTTP server.
    ///
    /// One process is the easier deployment for a self-hosted instance; a
    /// separate `worker` process is the better one when the queue is busy.
    #[arg(long)]
    pub with_worker: bool,
}

/// Options for `worker`.
#[derive(Debug, Args)]
pub struct WorkerArgs {
    /// Drain what is waiting and exit, rather than looping. For cron, for tests,
    /// and for an operator who wants to see the queue move once.
    #[arg(long)]
    pub once: bool,

    /// How many pending outbox events to deliver per pass.
    #[arg(long, value_name = "N", default_value_t = 50)]
    pub batch: i64,
}

/// Options for `migrate`.
#[derive(Debug, Args)]
pub struct MigrateArgs {
    /// Only report which migrations are pending; change nothing.
    #[arg(long)]
    pub status: bool,
}

/// Options for `seed`.
#[derive(Debug, Args)]
pub struct SeedArgs {
    /// Which seed to run. The development fixture stays the default when no
    /// subcommand is given, so `darkhold seed --development` keeps working.
    #[command(subcommand)]
    pub source: Option<SeedSource>,

    /// Required acknowledgement that this is a development instance.
    #[arg(long)]
    pub development: bool,

    /// Delete existing development data before seeding.
    #[arg(long)]
    pub reset: bool,

    /// Email address for the generated development account.
    #[arg(long, value_name = "EMAIL", default_value = "dev@darkhold.local")]
    pub email: String,

    /// Password for the generated development account.
    #[arg(long, value_name = "PASSWORD", default_value = "darkhold-dev")]
    pub password: String,
}

/// The data a `seed` command can import.
#[derive(Debug, Clone, Subcommand)]
pub enum SeedSource {
    /// Seed private ratings from the operator's rated reading list.
    ReadList(ReadListArgs),
}

/// Options for `seed read-list`.
#[derive(Debug, Clone, Args)]
pub struct ReadListArgs {
    /// The Calibre reading-list export to read.
    #[arg(long, value_name = "PATH")]
    pub file: PathBuf,

    /// Print the report without writing. The default; kept explicit for scripts.
    #[arg(long)]
    pub dry_run: bool,

    /// Write the matched ratings. Requires --account and --pseud.
    #[arg(long, conflicts_with = "dry_run")]
    pub apply: bool,

    /// The operator's account id, required with --apply.
    #[arg(long, value_name = "UUID")]
    pub account: Option<String>,

    /// The operator's pseud id, required with --apply. A rating belongs to a
    /// pseud, so --account alone is not enough.
    #[arg(long, value_name = "UUID")]
    pub pseud: Option<String>,
}

/// Options for `doctor`.
#[derive(Debug, Args)]
pub struct DoctorArgs {
    /// Treat warnings as failures.
    #[arg(long)]
    pub strict: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn the_cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn serve_is_the_only_command_that_needs_no_flags() {
        let cli = Cli::try_parse_from(["darkhold", "serve"]).expect("parses");
        assert!(matches!(cli.command, Command::Serve(_)));
        assert!(cli.global.config.is_none());
    }

    #[test]
    fn arguments_beat_environment_variables() {
        // clap resolves the pair for us; this pins the documented precedence.
        std::env::set_var("DARKHOLD_PORT", "9999");
        let cli = Cli::try_parse_from(["darkhold", "serve", "--port", "1234"]).expect("parses");
        assert_eq!(cli.global.port, Some(1234));
        let cli = Cli::try_parse_from(["darkhold", "serve"]).expect("parses");
        assert_eq!(cli.global.port, Some(9999));
        std::env::remove_var("DARKHOLD_PORT");
    }

    #[test]
    fn seed_requires_the_development_flag_to_be_explicit() {
        let cli = Cli::try_parse_from(["darkhold", "seed"]).expect("parses");
        match cli.command {
            Command::Seed(args) => assert!(!args.development),
            _ => panic!("expected seed"),
        }
    }
}

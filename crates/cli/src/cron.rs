//! M63-13: `dh cron install` — a systemd **user** timer for a saved query.
//!
//! The instance already runs timers for its own ingestion; this hands the same mechanism to a
//! reader without asking them to write a unit file. Three files per query, all under
//! `~/.config/darkhold/cron/`:
//!
//! ```text
//!   dh-weekly.timer     when it runs
//!   dh-weekly.service   what it runs
//!   dh-weekly.env       the token, 0600 — the only file that holds a secret
//! ```
//!
//! **This program does not run `systemctl`.** It prints the two commands and leaves them to the
//! reader: shelling out to `systemctl` means a CLI that fails differently on every distro, and a
//! timer an operator has to discover in a journal is a worse answer than a line they can read.
//!
//! The token is a scoped `api_tokens` credential carrying `library.read` — not a session cookie.
//! A cookie expires in days, so a timer built on one works until it silently stops, which is the
//! worst failure mode a scheduled job can have. `M63-13` widened `GET /saved-views`,
//! `GET /saved-views/{id}` and `POST /saved-views/{id}/runs` to accept exactly that token; the
//! verbs that create, rename and delete a view are still session-only.

use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::{Args, Subcommand};

use crate::rec::{auth_required, bad_request, environment, Failure};

/// The scope a timer's token has to carry.
const REQUIRED_SCOPE: &str = "library.read";

/// `dh cron …`
#[derive(Debug, Clone, Args)]
pub struct CronArgs {
    #[command(subcommand)]
    pub action: CronCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum CronCommand {
    /// Write a systemd user timer that runs a saved query on a schedule.
    ///
    /// The service runs `dh rec --saved <NAME> --digest-diff`, so each run prints only the works
    /// that are new since the previous one. `DARKHOLD_TOKEN` must be set: the token is written to
    /// `<name>.env` with mode 0600 and never printed.
    Install(Box<InstallArgs>),
    /// List the timers this program wrote, and how often each one runs.
    List,
}

#[derive(Debug, Clone, Args)]
pub struct InstallArgs {
    /// The saved query to run. It must already exist (`dh rec --save <NAME>`).
    #[arg(long, value_name = "NAME")]
    pub saved: String,

    /// How often to run: a number and one of `s`, `m`, `h`, `d` — `6h`, `90m`, `1d`.
    #[arg(long, value_name = "DURATION")]
    pub every: String,

    /// Spread the start by up to this much, so every timer on a host does not fire at once.
    #[arg(long, value_name = "DURATION")]
    pub jitter: Option<String>,

    /// Overwrite unit files that already exist.
    #[arg(long)]
    pub force: bool,

    /// Print the files that would be written, and write nothing.
    #[arg(long)]
    pub dry_run: bool,
}

/// The files one saved query needs, and the text of each.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub timer_path: PathBuf,
    pub timer_text: String,
    pub service_path: PathBuf,
    pub service_text: String,
    pub env_path: PathBuf,
}

impl Plan {
    /// The three paths, in the order they are written.
    pub fn paths(&self) -> [&Path; 3] {
        [&self.timer_path, &self.service_path, &self.env_path]
    }
}

pub async fn run(cli: &crate::Cli, args: &CronArgs) -> Result<(), Failure> {
    let _ = cli;
    match &args.action {
        CronCommand::Install(install) => install_now(install),
        CronCommand::List => list(),
    }
}

/// Where the units live: `~/.config/darkhold/cron`, or `DARKHOLD_CRON_DIR` for a test or a
/// reader who keeps their dotfiles elsewhere.
fn cron_dir() -> Result<PathBuf, Failure> {
    if let Some(dir) = std::env::var_os("DARKHOLD_CRON_DIR") {
        return Ok(PathBuf::from(dir));
    }
    let home = std::env::var_os("HOME").ok_or_else(|| {
        environment("HOME is not set, so there is nowhere to write a user unit ($HOME/.config/darkhold/cron)")
    })?;
    Ok(PathBuf::from(home).join(".config/darkhold/cron"))
}

fn install_now(args: &InstallArgs) -> Result<(), Failure> {
    let every = parse_span(&args.every, "--every")?;
    let jitter = match &args.jitter {
        Some(raw) => Some(parse_span(raw, "--jitter")?),
        None => None,
    };
    let dir = cron_dir()?;
    let token = std::env::var("DARKHOLD_TOKEN").map_err(|_| {
        auth_required(format!(
            "DARKHOLD_TOKEN is not set. A timer needs a token carrying `{REQUIRED_SCOPE}`: mint one \
             with `POST /api/v1/me/tokens` (scopes: [\"{REQUIRED_SCOPE}\"]), and keep a session \
             cookie out of the unit — it expires, and the timer then stops silently"
        ))
    })?;
    if token.trim().is_empty() {
        return Err(auth_required("DARKHOLD_TOKEN is empty"));
    }
    let base_url =
        std::env::var("DARKHOLD_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:8081".to_owned());
    let dh = std::env::current_exe()
        .map_err(|error| environment(format!("cannot find my own path for ExecStart: {error}")))?;
    let plan = plan(&args.saved, &every, jitter.as_deref(), &dir, &base_url, &dh)?;

    if args.dry_run {
        println!("would write:");
        for path in plan.paths() {
            println!("  {}", path.display());
        }
        println!(
            "\n--- {}\n{}",
            plan.timer_path.display(),
            plan.timer_text.trim_end()
        );
        println!(
            "\n--- {}\n{}",
            plan.service_path.display(),
            plan.service_text.trim_end()
        );
        println!(
            "\n{} holds DARKHOLD_TOKEN (mode 0600) and is not printed.",
            plan.env_path.display()
        );
        return Ok(());
    }

    std::fs::create_dir_all(&dir)
        .map_err(|error| environment(format!("{}: {error}", dir.display())))?;
    if !args.force {
        for path in plan.paths() {
            if path.exists() {
                return Err(bad_request(format!(
                    "{} already exists — a unit somebody may have edited by hand is data; pass \
                     --force to replace it",
                    path.display()
                )));
            }
        }
    }
    std::fs::write(&plan.env_path, format!("DARKHOLD_TOKEN={}\n", token.trim()))
        .map_err(|error| environment(format!("{}: {error}", plan.env_path.display())))?;
    // Set before anything else can read it: the file holds a credential.
    set_private(&plan.env_path)?;
    std::fs::write(&plan.service_path, &plan.service_text)
        .map_err(|error| environment(format!("{}: {error}", plan.service_path.display())))?;
    std::fs::write(&plan.timer_path, &plan.timer_text)
        .map_err(|error| environment(format!("{}: {error}", plan.timer_path.display())))?;

    let stem = unit_stem(&args.saved);
    println!(
        "wrote {} (0600), {} and {}",
        plan.env_path.display(),
        plan.service_path.display(),
        plan.timer_path.display()
    );
    println!("\nthe two commands this program will not run for you:");
    println!("  systemctl --user daemon-reload");
    println!("  systemctl --user enable --now {stem}.timer");
    Ok(())
}

/// `0600`, and refuse to continue if the file is reachable by anybody else.
#[cfg(unix)]
fn set_private(path: &Path) -> Result<(), Failure> {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|error| environment(format!("{}: {error}", path.display())))?;
    let mode = std::fs::metadata(path)
        .map_err(|error| environment(format!("{}: {error}", path.display())))?
        .permissions()
        .mode()
        & 0o777;
    if mode != 0o600 {
        return Err(environment(format!(
            "{} is mode {mode:o} after chmod; refusing to leave a token readable by others",
            path.display()
        )));
    }
    Ok(())
}

#[cfg(not(unix))]
fn set_private(_path: &Path) -> Result<(), Failure> {
    Err(environment(
        "a timer's environment file holds a token, and this build cannot set 0600 on this platform",
    ))
}

/// Validate an interval the way `systemd` will read it.
///
/// `systemd` accepts `6` as six seconds, which is why this refuses a bare number: a reader writing
/// `--every 6` means six hours, and a timer that fires every six seconds is worse than a refusal.
fn parse_span(raw: &str, flag: &str) -> Result<String, Failure> {
    let trimmed = raw.trim();
    let (digits, unit) = trimmed.split_at(trimmed.len().saturating_sub(1));
    let ok_unit = matches!(unit, "s" | "m" | "h" | "d");
    let ok_digits = !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit());
    if !ok_unit || !ok_digits {
        return Err(bad_request(format!(
            "{flag} {raw}: expected a number and one of s, m, h, d — `6h`, `90m`, `1d`"
        )));
    }
    let amount: u64 = digits.parse().map_err(|_| {
        bad_request(format!(
            "{flag} {raw}: the number is too large to be an interval"
        ))
    })?;
    if amount == 0 {
        return Err(bad_request(format!(
            "{flag} {raw}: an interval of zero would fire continuously"
        )));
    }
    Ok(format!("{amount}{unit}"))
}

/// The stem shared by the three files, derived from the saved query's name.
///
/// A unit name is not a place for arbitrary text: it is a filename, an argument to `systemctl`, and
/// a word in `ExecStart`. Everything outside `[a-z0-9_-]` becomes `-`, runs collapse, and the result
/// is prefixed so a query called `list` cannot shadow a system unit.
fn unit_stem(saved: &str) -> String {
    let mut stem = String::new();
    for ch in saved.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            stem.push(ch);
        } else if !stem.ends_with('-') {
            stem.push('-');
        }
    }
    let stem = stem.trim_matches('-').to_owned();
    format!("dh-{}", if stem.is_empty() { "query" } else { &stem })
}

/// Describe the files for one saved query. Pure, so the unit text can be pinned by a test.
fn plan(
    saved: &str,
    every: &str,
    jitter: Option<&str>,
    dir: &Path,
    base_url: &str,
    dh: &Path,
) -> Result<Plan, Failure> {
    if saved.trim().is_empty() {
        return Err(bad_request("--saved needs the name of a saved query"));
    }
    // `ExecStart` is split on whitespace by systemd; a quoted argument would need its own escaping
    // rules, and a saved query whose name cannot be written in a unit is a name to change.
    if saved.chars().any(char::is_whitespace) || saved.contains(['"', '\'']) {
        return Err(bad_request(format!(
            "--saved {saved:?}: systemd splits ExecStart on whitespace, so a name with a space or \
             a quote cannot be written as one argument. Rename the view."
        )));
    }
    let stem = unit_stem(saved);
    let env_path = dir.join(format!("{stem}.env"));
    let service_path = dir.join(format!("{stem}.service"));
    let timer_path = dir.join(format!("{stem}.timer"));

    let timer_text = format!(
        "[Unit]\n\
         Description=darkhold saved query \"{saved}\"\n\
         \n\
         [Timer]\n\
         OnBootSec=2min\n\
         OnUnitActiveSec={every}\n\
         RandomizedDelaySec={}\n\
         Persistent=true\n\
         \n\
         [Install]\n\
         WantedBy=timers.target\n",
        jitter.unwrap_or("0"),
    );
    let service_text = format!(
        "[Unit]\n\
         Description=darkhold saved query \"{saved}\"\n\
         Wants=network-online.target\n\
         After=network-online.target\n\
         \n\
         [Service]\n\
         Type=oneshot\n\
         EnvironmentFile={}\n\
         Environment=DARKHOLD_BASE_URL={base_url}\n\
         ExecStart={} rec --saved {saved} --digest-diff\n",
        env_path.display(),
        dh.display(),
    );
    Ok(Plan {
        timer_path,
        timer_text,
        service_path,
        service_text,
        env_path,
    })
}

/// What `dh cron list` prints, read back from the timer files themselves — so it describes what is
/// on disk rather than what this program last intended to write.
fn list() -> Result<(), Failure> {
    let dir = cron_dir()?;
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            println!("no timers: {} does not exist", dir.display());
            return Ok(());
        }
        Err(error) => return Err(environment(format!("{}: {error}", dir.display()))),
    };
    let mut timers: Vec<(String, String, String)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "timer") {
            continue;
        }
        let text = std::fs::read_to_string(&path)
            .map_err(|error| environment(format!("{}: {error}", path.display())))?;
        let every = field(&text, "OnUnitActiveSec=").unwrap_or_else(|| "?".to_owned());
        let what = field(&text, "Description=").unwrap_or_else(|| "?".to_owned());
        let name = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        timers.push((name, every, what));
    }
    timers.sort();
    if timers.is_empty() {
        println!("no timers in {}", dir.display());
        return Ok(());
    }
    for (name, every, what) in timers {
        println!("{name:<24} {every:<8} {what}");
    }
    Ok(())
}

fn field(text: &str, key: &str) -> Option<String> {
    text.lines()
        .find_map(|line| line.strip_prefix(key))
        .map(|value| value.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_span_is_a_number_and_a_unit() {
        assert_eq!(parse_span("6h", "--every").expect("6h"), "6h");
        assert_eq!(parse_span("90m", "--every").expect("90m"), "90m");
        assert_eq!(parse_span("1d", "--every").expect("1d"), "1d");
        assert_eq!(parse_span("45s", "--every").expect("45s"), "45s");
    }

    #[test]
    fn a_bare_number_is_refused_because_systemd_would_read_it_as_seconds() {
        let failure = parse_span("6", "--every").expect_err("refused");
        assert_eq!(failure.code, 2);
        assert!(
            failure
                .message
                .contains("expected a number and one of s, m, h, d"),
            "{}",
            failure.message
        );
        // And the shapes around it that a reader might write.
        for raw in ["6 hours", "1w", "h", "6H", "-6h", ""] {
            assert!(
                parse_span(raw, "--every").is_err(),
                "{raw:?} should be refused"
            );
        }
    }

    #[test]
    fn zero_is_refused_and_the_reason_says_why() {
        let failure = parse_span("0h", "--every").expect_err("refused");
        assert!(
            failure.message.contains("continuously"),
            "{}",
            failure.message
        );
    }

    #[test]
    fn a_name_becomes_a_unit_stem_that_cannot_shadow_a_system_unit() {
        assert_eq!(unit_stem("weekly"), "dh-weekly");
        assert_eq!(unit_stem("Weekly naruto"), "dh-weekly-naruto");
        assert_eq!(unit_stem("a/b"), "dh-a-b");
        assert_eq!(unit_stem("---"), "dh-query");
        assert_eq!(unit_stem(""), "dh-query");
    }

    #[test]
    fn the_unit_text_is_the_one_systemd_is_asked_to_read() {
        let dir = Path::new("/home/reader/.config/darkhold/cron");
        let plan = plan(
            "weekly",
            "6h",
            Some("10m"),
            dir,
            "http://127.0.0.1:8081",
            Path::new("/home/reader/.local/bin/dh"),
        )
        .expect("plans");

        assert_eq!(plan.timer_path, dir.join("dh-weekly.timer"));
        assert_eq!(plan.env_path, dir.join("dh-weekly.env"));
        assert!(
            plan.timer_text.contains("OnUnitActiveSec=6h\n"),
            "{}",
            plan.timer_text
        );
        assert!(
            plan.timer_text.contains("RandomizedDelaySec=10m\n"),
            "{}",
            plan.timer_text
        );
        assert!(
            plan.timer_text.contains("WantedBy=timers.target"),
            "{}",
            plan.timer_text
        );
        assert!(
            plan.service_text
                .contains("EnvironmentFile=/home/reader/.config/darkhold/cron/dh-weekly.env\n"),
            "{}",
            plan.service_text
        );
        assert!(
            plan.service_text.contains(
                "ExecStart=/home/reader/.local/bin/dh rec --saved weekly --digest-diff\n"
            ),
            "{}",
            plan.service_text
        );
        // No token in either unit: it lives in the env file, which is 0600.
        assert!(!plan.service_text.contains("DARKHOLD_TOKEN"));
        assert!(!plan.timer_text.contains("DARKHOLD_TOKEN"));
    }

    #[test]
    fn no_jitter_writes_zero_rather_than_leaving_the_key_out() {
        let plan = plan(
            "weekly",
            "6h",
            None,
            Path::new("/tmp/x"),
            "http://127.0.0.1:8081",
            Path::new("/usr/bin/dh"),
        )
        .expect("plans");
        assert!(
            plan.timer_text.contains("RandomizedDelaySec=0\n"),
            "an explicit zero, so the unit says what it means: {}",
            plan.timer_text
        );
    }

    #[test]
    fn a_name_that_cannot_survive_execstart_is_refused() {
        let failure = plan(
            "weekly naruto",
            "6h",
            None,
            Path::new("/tmp/x"),
            "http://127.0.0.1:8081",
            Path::new("/usr/bin/dh"),
        )
        .expect_err("refused");
        assert_eq!(failure.code, 2);
        assert!(
            failure.message.contains("splits ExecStart on whitespace"),
            "{}",
            failure.message
        );
    }

    #[test]
    fn the_listing_reads_the_units_it_wrote() {
        let text =
            "[Unit]\nDescription=darkhold saved query \"weekly\"\n\n[Timer]\nOnUnitActiveSec=6h\n";
        assert_eq!(field(text, "OnUnitActiveSec=").as_deref(), Some("6h"));
        assert_eq!(
            field(text, "Description=").as_deref(),
            Some("darkhold saved query \"weekly\"")
        );
        assert_eq!(field(text, "Absent="), None);
    }
}

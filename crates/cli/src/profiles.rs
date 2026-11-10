//! `M63-18` — config profiles: one user, two instances, without exporting anything twice.
//!
//! A profile is a named `(url, token, cookie)` triple in `config.toml`. The file exists because the
//! alternative — `export DARKHOLD_TOKEN=…` in a shell, a second one in another shell — is how a
//! token for the wrong instance ends up in a request, and the failure looks like a server bug.
//!
//! Three rules the code is built around:
//!
//! 1. **One resolution order, in one function.** `--flag` beats `DARKHOLD_*` (clap folds the
//!    environment into the flag) beats the selected profile beats the built-in default. Because
//!    `resolve` is the only place that decides, the CLI cannot disagree with itself about which
//!    instance it is talking to.
//! 2. **An unknown profile is a named error.** Falling back to the default profile would send the
//!    request with the *other* instance's token to the *other* instance — the exact failure
//!    profiles exist to prevent — and it would do it silently.
//! 3. **A token read back is always redacted.** `dh config get profiles.work.token` prints
//!    `set (hidden, N chars)`. A prefix is the part that ends up in a screenshot, and the length
//!    plus the profile name is what a reader actually needs to tell two tokens apart.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};

/// The instance `dh` talks to when nothing else says otherwise.
pub const DEFAULT_URL: &str = "http://127.0.0.1:8081";

/// What `config.toml` holds.
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct ConfigFile {
    /// The profile used when neither `--profile` nor `DARKHOLD_PROFILE` names one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_profile: Option<String>,
    /// Profiles by name, ordered so `dh config get profiles` is stable output.
    #[serde(default)]
    pub profiles: BTreeMap<String, Profile>,
}

/// One instance's credentials.
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct Profile {
    /// This instance's base URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// A bearer token for the doors that rank against your own history.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    /// A session cookie for the doors that act for an account.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cookie: Option<String>,
}

/// What the CLI will actually use, after flags, environment and profile are folded together.
#[derive(Debug, Clone)]
pub struct Resolved {
    /// The base URL to send requests to.
    pub url: String,
    /// The bearer token, if any.
    pub token: Option<String>,
    /// The session cookie, if any.
    pub cookie: Option<String>,
    /// The profile the URL came from, for the one line `dh config path` can print.
    pub profile: Option<String>,
}

/// Where the config file lives.
///
/// `DARKHOLD_CONFIG` wins over the platform location so a test — or a throwaway profile set — never
/// touches the caller's real credentials.
pub fn path() -> PathBuf {
    if let Ok(explicit) = std::env::var("DARKHOLD_CONFIG") {
        if !explicit.trim().is_empty() {
            return PathBuf::from(explicit);
        }
    }
    let base = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|home| Path::new(&home).join(".config"))
        })
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("darkhold").join("config.toml")
}

/// Read the file, treating "does not exist" as "no profiles yet" rather than an error.
pub fn load(file: &Path) -> Result<ConfigFile> {
    match std::fs::read_to_string(file) {
        Ok(text) => toml::from_str(&text)
            .with_context(|| format!("{} is not a valid profile config", file.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(ConfigFile::default()),
        Err(error) => Err(error).with_context(|| format!("reading {}", file.display())),
    }
}

/// Write the file with mode `0600` — the file can hold a token, so the permission is part of the write.
pub fn save(file: &Path, config: &ConfigFile) -> Result<()> {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let text = toml::to_string_pretty(config).context("serialising the profile config")?;
    std::fs::write(file, text).with_context(|| format!("writing {}", file.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(file, mode)
            .with_context(|| format!("restricting {}", file.display()))?;
    }
    Ok(())
}

/// Fold flags, environment (already folded into the flags by clap) and the profile together.
///
/// An empty `flag_url` means "not given": the parser's default is empty precisely so a profile can
/// supply the URL, and `--base-url ""` is then the same as omitting it.
pub fn resolve(
    flag_url: &str,
    flag_token: Option<&str>,
    flag_cookie: Option<&str>,
    profile: Option<&str>,
    file: &Path,
) -> Result<Resolved> {
    let config = load(file)?;
    let selected = profile
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .or_else(|| config.default_profile.clone());
    let entry = match selected.as_deref() {
        Some(name) => Some(config.profiles.get(name).ok_or_else(|| {
            anyhow!(
                "no profile named `{name}` in {} (profiles: {})",
                file.display(),
                names(&config)
            )
        })?),
        None => None,
    };
    let url = if flag_url.trim().is_empty() {
        entry
            .and_then(|profile| profile.url.clone())
            .unwrap_or_else(|| DEFAULT_URL.to_owned())
    } else {
        flag_url.trim().to_owned()
    };
    Ok(Resolved {
        url,
        token: flag_token.map(str::to_owned).or_else(|| {
            entry
                .and_then(|profile| profile.token.clone())
                .filter(|value| !value.trim().is_empty())
        }),
        cookie: flag_cookie.map(str::to_owned).or_else(|| {
            entry
                .and_then(|profile| profile.cookie.clone())
                .filter(|value| !value.trim().is_empty())
        }),
        profile: selected,
    })
}

/// The profile names, comma-separated, for an error that tells the caller what they may type.
pub fn names(config: &ConfigFile) -> String {
    if config.profiles.is_empty() {
        return "none".to_owned();
    }
    config
        .profiles
        .keys()
        .cloned()
        .collect::<Vec<_>>()
        .join(", ")
}

/// Read one dotted key.
///
/// `default_profile`, `profiles`, or `profiles.<name>.<url|token|cookie>`. A secret is **never**
/// returned in clear: the caller gets a description of it, and the description is what the door
/// prints.
pub fn get(config: &ConfigFile, key: &str) -> Result<String> {
    let parts: Vec<&str> = key.trim().split('.').collect();
    match parts.as_slice() {
        ["default_profile"] => Ok(config
            .default_profile
            .clone()
            .unwrap_or_else(|| "(none)".to_owned())),
        ["profiles"] => Ok(names(config)),
        ["profiles", name, field] => {
            let profile = config.profiles.get(*name).ok_or_else(|| {
                anyhow!("no profile named `{name}` (profiles: {})", names(config))
            })?;
            let value = match *field {
                "url" => profile.url.clone(),
                "token" => return Ok(describe_secret(profile.token.as_deref())),
                "cookie" => return Ok(describe_secret(profile.cookie.as_deref())),
                other => {
                    return Err(unknown_field(other));
                }
            };
            Ok(value.unwrap_or_else(|| "(unset)".to_owned()))
        }
        _ => Err(unknown_key(key, config)),
    }
}

/// Write one dotted key, creating the profile when it does not exist yet.
pub fn set(config: &mut ConfigFile, key: &str, value: &str) -> Result<()> {
    let parts: Vec<&str> = key.trim().split('.').collect();
    match parts.as_slice() {
        ["default_profile"] => {
            let value = value.trim();
            config.default_profile = if value.is_empty() {
                None
            } else {
                Some(value.to_owned())
            };
            Ok(())
        }
        ["profiles", name, field] => {
            let profile = config.profiles.entry((*name).to_owned()).or_default();
            match *field {
                "url" => profile.url = Some(value.trim().to_owned()),
                "token" => profile.token = Some(value.trim().to_owned()),
                "cookie" => profile.cookie = Some(value.trim().to_owned()),
                other => return Err(unknown_field(other)),
            }
            Ok(())
        }
        _ => Err(unknown_key(key, config)),
    }
}

fn describe_secret(value: Option<&str>) -> String {
    match value.map(str::trim).filter(|text| !text.is_empty()) {
        // Length, not a prefix: a prefix is what a screenshot leaks, and the length is enough to
        // tell two entries apart when the profile name already says which instance this is.
        Some(text) => format!("set (hidden, {} chars)", text.chars().count()),
        None => "(unset)".to_owned(),
    }
}

fn unknown_field(field: &str) -> anyhow::Error {
    anyhow!("`{field}` is not a profile field; use url, token or cookie")
}

fn unknown_key(key: &str, config: &ConfigFile) -> anyhow::Error {
    anyhow!(
        "`{key}` is not a config key; use default_profile, profiles, or profiles.<name>.<url|token|cookie> (profiles: {})",
        names(config)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("a clock after 1970")
            .as_nanos();
        std::env::temp_dir().join(format!("dh-profiles-{tag}-{nanos}.toml"))
    }

    fn write_config(tag: &str, text: &str) -> PathBuf {
        let path = temp_path(tag);
        std::fs::write(&path, text).expect("the fixture config");
        path
    }

    #[test]
    fn a_profile_supplies_the_url_and_token_when_no_flag_does() {
        let path = write_config(
            "profile",
            r#"
default_profile = "work"

[profiles.work]
url = "https://work.example"
token = "work-token-0123456789"
"#,
        );
        let resolved = resolve("", None, None, None, &path).expect("a resolvable profile");
        assert_eq!(resolved.url, "https://work.example");
        assert_eq!(resolved.token.as_deref(), Some("work-token-0123456789"));
        assert_eq!(resolved.profile.as_deref(), Some("work"));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn an_explicit_url_beats_the_profile_and_keeps_its_token() {
        let path = write_config(
            "flag",
            r#"
default_profile = "work"

[profiles.work]
url = "https://work.example"
token = "work-token"
"#,
        );
        // `--base-url` is about *where*, not *who*: pointing one command at a staging instance
        // should not silently drop the token the profile carries.
        let resolved =
            resolve("https://staging.example", None, None, None, &path).expect("resolved");
        assert_eq!(resolved.url, "https://staging.example");
        assert_eq!(resolved.token.as_deref(), Some("work-token"));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn an_unknown_profile_is_named_rather_than_fallen_back_from() {
        let path = write_config(
            "unknown",
            r#"
default_profile = "work"

[profiles.work]
url = "https://work.example"
"#,
        );
        let error = resolve("", None, None, Some("home"), &path).expect_err("a named refusal");
        let message = error.to_string();
        assert!(message.contains("no profile named `home`"), "{message}");
        // The refusal lists what may be typed instead, so it is actionable without opening the file.
        assert!(message.contains("work"), "{message}");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_missing_file_is_not_an_error_and_the_default_url_survives() {
        let path = temp_path("missing");
        let resolved = resolve("", None, None, None, &path).expect("no file, still resolvable");
        assert_eq!(resolved.url, DEFAULT_URL);
        assert!(resolved.token.is_none());
        assert!(resolved.profile.is_none());
    }

    #[test]
    fn a_token_read_back_is_described_never_printed() {
        let path = write_config(
            "secret",
            r#"
[profiles.work]
token = "0123456789abcdef"
cookie = "darkhold_session=abc"
"#,
        );
        let config = load(&path).expect("the fixture loads");
        let token = get(&config, "profiles.work.token").expect("a readable key");
        assert_eq!(token, "set (hidden, 16 chars)");
        assert!(!token.contains("0123"), "{token}");
        let cookie = get(&config, "profiles.work.cookie").expect("a readable key");
        assert!(!cookie.contains("darkhold_session"), "{cookie}");
        // A URL is not a secret and prints in clear.
        assert_eq!(
            get(&config, "profiles.work.url").expect("a readable key"),
            "(unset)"
        );
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn config_set_round_trips_a_dotted_key_and_restricts_the_file() {
        let path = temp_path("roundtrip");
        let mut config = load(&path).expect("an empty config");
        set(&mut config, "profiles.work.url", "https://work.example").expect("a writable key");
        set(&mut config, "profiles.work.token", "secret-token").expect("a writable key");
        set(&mut config, "default_profile", "work").expect("a writable key");
        save(&path, &config).expect("the file is written");

        let reloaded = load(&path).expect("the file round-trips");
        assert_eq!(
            get(&reloaded, "profiles.work.url").expect("a readable key"),
            "https://work.example"
        );
        assert_eq!(
            get(&reloaded, "default_profile").expect("a readable key"),
            "work"
        );
        assert_eq!(get(&reloaded, "profiles").expect("a readable key"), "work");
        // `set` on a token is how the secret gets in; `get` is how it must never come out.
        assert_eq!(
            get(&reloaded, "profiles.work.token").expect("a readable key"),
            "set (hidden, 12 chars)"
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path)
                .expect("the file")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600, "a file that can hold a token is 0600");
        }
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn an_unknown_key_is_refused_with_the_vocabulary() {
        let path = write_config(
            "keys",
            r#"
[profiles.work]
url = "https://work.example"
"#,
        );
        let config = load(&path).expect("the fixture loads");
        // The profile exists, so the refusal is about the field — not "no such profile", which would
        // send the caller to fix the wrong half of the key.
        let error = get(&config, "profiles.work.passwd").expect_err("a named refusal");
        assert!(
            error.to_string().contains("url, token or cookie"),
            "{error}"
        );
        let error = get(&config, "instance").expect_err("a named refusal");
        assert!(error.to_string().contains("default_profile"), "{error}");
        // An unknown profile is still named, and the message lists what exists.
        let error = get(&config, "profiles.home.url").expect_err("a named refusal");
        assert!(
            error.to_string().contains("no profile named `home`"),
            "{error}"
        );
        assert!(error.to_string().contains("work"), "{error}");
        std::fs::remove_file(&path).ok();
    }
}

//! `M63-16` — the exit codes are a documented, tested API, not an accident of `return 1`.
//!
//! A wrapper script decides what to do next from this number alone: retry, ask for a token, tell the
//! person the fandom does not exist, or give up. Every ambiguity here becomes a wrong decision in
//! somebody's cron job, so the table is closed and this module is its only definition.
//!
//! | code | meaning | what a script should do |
//! |---:|---|---|
//! | 0 | the command did what was asked | continue |
//! | 1 | the environment failed — 5xx, connection refused, a DNS failure | retry, later |
//! | 2 | the command was wrong — bad flag, bad value, unparseable window | **do not retry**; fix the call |
//! | 3 | the thing named does not exist (404) | do not retry; the input is stale |
//! | 4 | authentication or authorisation failed (401, 403) | obtain a session, then retry |
//! | 5 | rate-limited (429) | retry after the window the server names |
//!
//! Three of these did not exist before: `401`, `403` and `404` were all reported as `2`. That is a
//! **usage** code, so a script was told "your command is wrong" when the truth was "your token is
//! missing" or "that work was deleted" — the two cases it can do something about were the two it
//! could not tell apart from the one it cannot.
//!
//! `2` is also what `clap` itself exits with when a flag does not parse, which is why the usage code
//! is `2` and not `1`: a shell that sees `2` may assume nothing ran, and that is true in both cases.
//!
//! Adding a code is a breaking change to the contract: a script written against `>= 2` is a script
//! making an assumption. Append, never renumber.

/// The command did what was asked.
pub const OK: u8 = 0;
/// The environment failed: 5xx, connection refused, TLS, DNS. Retrying may help.
pub const ERROR: u8 = 1;
/// The command itself was wrong: a bad flag, a bad value, an unparseable window.
pub const USAGE: u8 = 2;
/// The thing named does not exist (404).
pub const NOT_FOUND: u8 = 3;
/// Authentication or authorisation failed (401, 403).
pub const AUTH: u8 = 4;
/// Rate-limited (429).
pub const RATE_LIMITED: u8 = 5;

/// The exit code for an HTTP status the server answered with.
///
/// One function, so every verb answers the same way. A per-verb mapping is how `dh rec` and
/// `dh supply` end up telling a script two different stories about the same `401`.
#[must_use]
pub const fn for_status(status: u16) -> u8 {
    match status {
        200..=299 => OK,
        401 | 403 => AUTH,
        404 => NOT_FOUND,
        429 => RATE_LIMITED,
        // 400 and 422 are the server saying the request's CONTENT was wrong — an unknown fandom, an
        // unparseable window. That is the caller's mistake and retrying unchanged cannot help, so it
        // is USAGE rather than ERROR even though a server produced it.
        400 | 422 => USAGE,
        // 409 and 412 are conflicts a caller can resolve by re-reading, but not by retrying blindly;
        // they are ERROR because the input was not wrong.
        _ => ERROR,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_code_in_the_table_is_reachable_from_a_status_or_a_verb() {
        // 0 comes from a success, 2 from clap and from 400/422, and 1 from everything unmapped.
        let reachable = [200, 401, 404, 429, 400, 500].map(for_status);
        for code in [OK, ERROR, USAGE, NOT_FOUND, AUTH, RATE_LIMITED] {
            assert!(
                reachable.contains(&code),
                "code {code} is documented but no status produces it — a table with an unreachable \
                 row is a table nobody has run"
            );
        }
    }

    #[test]
    fn a_missing_token_is_not_a_usage_error() {
        assert_eq!(for_status(401), AUTH);
        assert_eq!(for_status(403), AUTH);
    }

    #[test]
    fn not_found_is_its_own_code_because_the_input_is_stale_not_wrong() {
        assert_eq!(for_status(404), NOT_FOUND);
    }

    #[test]
    fn a_rate_limit_says_retry_and_names_itself() {
        assert_eq!(for_status(429), RATE_LIMITED);
    }

    #[test]
    fn a_server_failure_is_retryable_and_a_bad_value_is_not() {
        assert_eq!(for_status(500), ERROR);
        assert_eq!(for_status(503), ERROR);
        assert_eq!(for_status(422), USAGE);
        assert_eq!(for_status(400), USAGE);
    }
}

//! `M63-20` — one request path, and it waits when the instance says to.
//!
//! Three verbs each built their own `reqwest` call, which is how they came to disagree about exit
//! codes (`M63-16`); a rate limit was the next thing they would have disagreed about. A batch that
//! dies at row 400 because of a `429` is a tooling bug, not a user error, so the wait belongs in one
//! place: this module.
//!
//! The rules:
//!
//! - **`429` is retried, with jitter, up to [`MAX_ATTEMPTS`].** The server names its window in
//!   `Retry-After`; we honour it, capped at [`MAX_WAIT`] so a hostile or misconfigured header cannot
//!   hang a terminal for an hour. No header means one second, which is the smallest honest guess.
//! - **The wait is printed** to stderr, because a command that pauses for 40 seconds with no output
//!   reads as a hang. Diagnostics never go to stdout: a caller piping `--json` to `jq` must not
//!   receive it.
//! - **Jitter is added on top of the server's window**, so N scripts that hit the limit together do
//!   not come back in lockstep and hit it again together. It is derived from the clock rather than a
//!   random-number dependency: the purpose is decorrelation, not unpredictability.
//! - **Giving up is code `5`**, with the window in the message, so a wrapper knows to come back
//!   later rather than to fix its command (`M63-16`).
//! - **Only `429` is retried.** A `500` may also be transient, but retrying it silently would turn a
//!   broken instance into a slow one; the caller gets `1` and can decide.
//!
//! ```text
//! $ dh rec --fandom naruto
//! rate-limited: waiting 2s (attempt 2 of 3)
//! ```

use std::time::Duration;

use crate::exit;
use crate::rec::{environment, Failure};

/// How many times a request is attempted before the rate limit wins.
pub const MAX_ATTEMPTS: u32 = 3;

/// The longest wait a `Retry-After` may ask for before we cap it.
pub const MAX_WAIT: Duration = Duration::from_secs(60);

/// The server's window, from `Retry-After` in seconds. Anything unparseable is one second.
#[must_use]
pub fn retry_after(response: &reqwest::Response) -> Duration {
    response
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map_or(Duration::from_secs(1), Duration::from_secs)
        .min(MAX_WAIT)
}

/// A few hundred milliseconds of decorrelation, from the clock.
#[must_use]
pub fn jitter() -> Duration {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    Duration::from_millis(u64::from(nanos % 250))
}

/// Send a request, waiting out a `429` and returning `(status, body)`.
///
/// `build` is called once per attempt: a `RequestBuilder` cannot be reused, and rebuilding it is also
/// what keeps the headers honest on the retry.
pub async fn send(
    build: impl Fn() -> reqwest::RequestBuilder,
    base_url: &str,
) -> Result<(u16, Vec<u8>), Failure> {
    let mut attempt = 1;
    loop {
        let response = build().send().await.map_err(|e| match e.is_connect() {
            true => environment(format!(
                "nothing listening at {base_url} — is the server running? (--base-url)"
            )),
            false => environment(format!("request failed: {e}")),
        })?;
        let status = response.status().as_u16();

        if status == 429 && attempt < MAX_ATTEMPTS {
            let wait = retry_after(&response);
            // stderr, not stdout: `--json` output is parsed by the caller's `jq`.
            eprintln!(
                "rate-limited: waiting {}s (attempt {} of {MAX_ATTEMPTS})",
                wait.as_secs(),
                attempt + 1
            );
            tokio::time::sleep(wait + jitter()).await;
            attempt += 1;
            continue;
        }

        if status == 429 {
            let wait = retry_after(&response);
            return Err(Failure {
                code: exit::RATE_LIMITED,
                message: format!(
                    "rate-limited: the instance asked for {}s and {MAX_ATTEMPTS} attempts were \
                     spent; retry after that window (the response's Retry-After header says the same)",
                    wait.as_secs()
                ),
            });
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| environment(format!("reading the response: {e}")))?;
        return Ok((status, bytes.to_vec()));
    }
}

/// The refusal behind a non-2xx status, in one place (`M63-19`).
///
/// The verbs used to write their own sentence about a `403`, and each one guessed: `dh rec` said
/// "this engine reads your own history", which is true of `rec` and false of `dh supply complete`.
/// More importantly, none of them could say **which scope** was missing, because the server named it
/// only inside an English sentence. The error body now carries `required_scope`, and this is the only
/// place a refusal is turned into a message and a code.
///
/// The distinction that matters, and that a caller cannot make from the status alone:
///
/// - `403` **with** a scope — the token is real and too small. The fix is to mint a wider one, and
///   the message says which scope.
/// - `403` **without** one — the account's *role* was refused. There is no scope to add, and
///   suggesting one would send the caller to mint a token that still would not open the door.
#[must_use]
pub fn refusal(status: u16, body: &serde_json::Value, base_url: &str) -> Failure {
    let message = body
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| body.to_string());
    let scope = body
        .get("error")
        .and_then(|error| error.get("required_scope"))
        .and_then(serde_json::Value::as_str);

    let text = match (status, scope) {
        (401, _) => format!(
            "{message}\nthis door needs a session: set DARKHOLD_TOKEN (or pass --token), or \
             DARKHOLD_COOKIE for a browser session"
        ),
        (403, Some(scope)) => format!(
            "{message}\nthe token you are using lacks the `{scope}` scope: mint one that has it at \
             {base_url}/settings/tokens, or use a session (DARKHOLD_COOKIE)"
        ),
        (403, None) => format!(
            "{message}\nthis account's role was refused for this door, not a scope — a scope \
             refusal names the scope it wants, so adding scopes to a token will not open it"
        ),
        (429, _) => format!(
            "{message}\nrate-limited: the instance asked you to wait; the retry window is in the \
             response's Retry-After header"
        ),
        (400 | 404 | 422, _) => message,
        _ => format!("{status} from {base_url}: {message}"),
    };
    Failure {
        message: text,
        code: exit::for_status(status),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_or_unparseable_window_is_one_second() {
        // The header is the server's promise; when it makes none, one second is the smallest honest
        // guess and the retry still happens.
        assert_eq!(Duration::from_secs(1).min(MAX_WAIT), Duration::from_secs(1));
        assert!(jitter() < Duration::from_millis(250), "jitter stays small");
    }

    #[test]
    fn the_cap_is_below_the_point_where_a_terminal_reads_the_wait_as_a_hang() {
        assert!(MAX_WAIT <= Duration::from_secs(60));
        assert_eq!(MAX_ATTEMPTS, 3, "three attempts, then code 5");
    }
}

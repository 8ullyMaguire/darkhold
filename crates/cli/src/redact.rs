//! `M63-21` — `dh redact`: what leaves your machine is the thing worth pasting.
//!
//! The rule §0.3 states for artefacts applies at the tool boundary too: never an id in something a
//! person will paste into an issue, a chat, or a support thread. The alternative is remembering to
//! strip it by hand at the moment of frustration, which is when nobody does.
//!
//! ```text
//! $ dh rec --fandom naruto --json | dh redact
//! $ dh redact < transcript.json
//! ```
//!
//! **What is removed**, and why each one is a class rather than a guess:
//!
//! - **UUIDs** — every account, pseud, work and routing id in this system is one. A single leaked
//!   account id joins every other row about that person.
//! - **Email addresses** — an account identifier that is also a real-world identity.
//! - **Session cookies** (`darkhold_session=…`) — a live credential, not an identifier.
//! - **Bearer tokens** (`Bearer …`, `DARKHOLD_TOKEN=…`) — the same.
//!
//! **What is deliberately not touched:** fandom names, handles typed by the user, work titles. A
//! redactor that guesses at "pseudonymous-looking" words corrupts the very output that makes the
//! paste useful, and a wrong redaction is worse than none: it reads as if the field were scrubbed.
//!
//! **Replacements are numbered per class in order of first appearance** (`uuid#1`, `email#1`), so two
//! pastes of the same transcript redact identically, and a reader can still see that two lines refer
//! to the same value without learning which value it is. A hash would do the first but not the second;
//! a random token would do neither.

/// The classes this redactor knows, each with its placeholder stem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    Uuid,
    Email,
    Cookie,
    Token,
}

impl Class {
    fn stem(self) -> &'static str {
        match self {
            Class::Uuid => "uuid",
            Class::Email => "email",
            Class::Cookie => "session",
            Class::Token => "token",
        }
    }
}

/// Redact every known secret from `input`, numbering each class by first appearance.
#[must_use]
pub fn redact(input: &str) -> String {
    let bytes: Vec<char> = input.chars().collect();
    let mut out = String::with_capacity(input.len());
    // Per class: the values seen so far, in order. A linear scan is right here — a transcript has a
    // handful of distinct ids, and a map would only make the numbering harder to reason about.
    let mut seen: Vec<(Class, String, usize)> = Vec::new();
    let mut i = 0;

    while i < bytes.len() {
        if let Some((class, end)) = match_uuid(&bytes, i) {
            out.push_str(&placeholder(&mut seen, class, &bytes[i..end]));
            i = end;
            continue;
        }
        if let Some((class, end)) = match_email(&bytes, i) {
            out.push_str(&placeholder(&mut seen, class, &bytes[i..end]));
            i = end;
            continue;
        }
        if let Some((class, end)) =
            match_prefixed_secret(&bytes, i, "darkhold_session=", Class::Cookie)
        {
            out.push_str("darkhold_session=");
            out.push_str(&placeholder(
                &mut seen,
                class,
                &bytes[i + "darkhold_session=".len()..end],
            ));
            i = end;
            continue;
        }
        if let Some((class, end)) = match_prefixed_secret(&bytes, i, "Bearer ", Class::Token) {
            out.push_str("Bearer ");
            out.push_str(&placeholder(
                &mut seen,
                class,
                &bytes[i + "Bearer ".len()..end],
            ));
            i = end;
            continue;
        }
        if let Some((class, end)) =
            match_prefixed_secret(&bytes, i, "DARKHOLD_TOKEN=", Class::Token)
        {
            out.push_str("DARKHOLD_TOKEN=");
            out.push_str(&placeholder(
                &mut seen,
                class,
                &bytes[i + "DARKHOLD_TOKEN=".len()..end],
            ));
            i = end;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

/// The `class#n` for a value, allocating the next number the first time it is seen.
fn placeholder(seen: &mut Vec<(Class, String, usize)>, class: Class, value: &[char]) -> String {
    let text: String = value.iter().collect();
    if let Some((_, _, n)) = seen.iter().find(|(c, v, _)| *c == class && *v == text) {
        return format!("{}#{}", class.stem(), n);
    }
    let n = seen.iter().filter(|(c, _, _)| *c == class).count() + 1;
    seen.push((class, text, n));
    format!("{}#{}", class.stem(), n)
}

fn is_hex(c: char) -> bool {
    c.is_ascii_hexdigit()
}

/// `8-4-4-4-12` hex. Anchored on the dashes so a random word is not mistaken for an id.
fn match_uuid(bytes: &[char], start: usize) -> Option<(Class, usize)> {
    const GROUPS: [usize; 5] = [8, 4, 4, 4, 12];
    let mut at = start;
    for (index, len) in GROUPS.iter().enumerate() {
        for _ in 0..*len {
            if at >= bytes.len() || !is_hex(bytes[at]) {
                return None;
            }
            at += 1;
        }
        if index + 1 < GROUPS.len() {
            if at >= bytes.len() || bytes[at] != '-' {
                return None;
            }
            at += 1;
        }
    }
    // A uuid glued to more hex digits is a longer token, not one of these.
    if at < bytes.len() && is_hex(bytes[at]) {
        return None;
    }
    Some((Class::Uuid, at))
}

/// `local@domain.tld`: letters, digits and `._%+-` before the `@`; a dotted domain with a 2+ letter
/// final label after it. Deliberately conservative — a missed email is a nuisance, a false positive
/// mangles a work title.
fn match_email(bytes: &[char], start: usize) -> Option<(Class, usize)> {
    let mut at = start;
    let local_start = at;
    while at < bytes.len()
        && (bytes[at].is_alphanumeric() || matches!(bytes[at], '.' | '_' | '%' | '+' | '-'))
    {
        at += 1;
    }
    if at == local_start || at >= bytes.len() || bytes[at] != '@' {
        return None;
    }
    at += 1;
    let domain_start = at;
    while at < bytes.len() && (bytes[at].is_alphanumeric() || bytes[at] == '.' || bytes[at] == '-')
    {
        at += 1;
    }
    let domain: String = bytes[domain_start..at].iter().collect();
    if !domain.contains('.') {
        return None;
    }
    let tld = domain.rsplit('.').next().unwrap_or("");
    if tld.len() < 2 || !tld.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    Some((Class::Email, at))
}

/// A secret after a known prefix, ending at whitespace or one of `"',;)}]`.
fn match_prefixed_secret(
    bytes: &[char],
    start: usize,
    prefix: &str,
    class: Class,
) -> Option<(Class, usize)> {
    let prefix_chars: Vec<char> = prefix.chars().collect();
    if start + prefix_chars.len() > bytes.len() {
        return None;
    }
    if bytes[start..start + prefix_chars.len()] != prefix_chars[..] {
        return None;
    }
    let mut at = start + prefix_chars.len();
    let value_start = at;
    while at < bytes.len()
        && !bytes[at].is_whitespace()
        && !matches!(bytes[at], '"' | '\'' | ',' | ';' | ')' | '}' | ']')
    {
        at += 1;
    }
    if at == value_start {
        return None;
    }
    Some((class, at))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_uuid_becomes_a_numbered_placeholder_and_repeats_keep_their_number() {
        let input = "{\"pseud_id\":\"734911ed-41fc-4458-aa70-2c5734fb0bc1\",\"again\":\"734911ed-41fc-4458-aa70-2c5734fb0bc1\"}";
        assert_eq!(
            redact(input),
            "{\"pseud_id\":\"uuid#1\",\"again\":\"uuid#1\"}"
        );
    }

    #[test]
    fn two_different_uuids_get_different_numbers_in_order_of_appearance() {
        let input = "a 00000000-0000-0000-0000-000000000001 b 00000000-0000-0000-0000-000000000002";
        assert_eq!(redact(input), "a uuid#1 b uuid#2");
    }

    #[test]
    fn an_email_is_redacted_and_a_work_title_is_not() {
        assert_eq!(
            redact("owner: alvaro@example.com wrote The @ Naruto Fic"),
            "owner: email#1 wrote The @ Naruto Fic"
        );
    }

    #[test]
    fn a_session_cookie_and_a_bearer_token_lose_only_their_values() {
        assert_eq!(
            redact("Cookie: darkhold_session=abc123; other=kept"),
            "Cookie: darkhold_session=session#1; other=kept"
        );
        assert_eq!(
            redact("Authorization: Bearer eyJhbGciOi.abc.def"),
            "Authorization: Bearer token#1"
        );
    }

    #[test]
    fn a_secret_keeps_its_prefix_so_the_line_still_reads_as_what_it_was() {
        assert_eq!(
            redact("DARKHOLD_TOKEN=deadbeefcafe"),
            "DARKHOLD_TOKEN=token#1"
        );
    }

    #[test]
    fn text_without_secrets_is_unchanged() {
        let input = "naruto, 2026-01-01..2026-10-15, wilson, 12 rows";
        assert_eq!(redact(input), input);
    }

    #[test]
    fn the_same_transcript_redacts_identically_twice() {
        let input = "a@b.co 00000000-0000-0000-0000-00000000000f a@b.co";
        assert_eq!(redact(input), redact(input));
        assert_eq!(redact(input), "email#1 uuid#1 email#1");
    }

    #[test]
    fn a_uuid_glued_to_more_hex_digits_is_left_alone() {
        // The anchor on the dashes is what stops a longer hash being half-redacted, which would leave
        // a fragment that looks like an id and is not one.
        let input = "734911ed-41fc-4458-aa70-2c5734fb0bc1a";
        assert_eq!(redact(input), input);
    }
}

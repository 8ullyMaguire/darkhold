//! M69-21 — a Saved Filter's public life: the keybind and the blend (Suki §8.2–§8.5).
//!
//! # What a blend is, and what it is not
//!
//! A blend is two saved filters answering one question. It is **not** a third filter language: both sides are saved views the
//! reader already owns (or can see), each already validated by its own writer, and the only new decision is how to interleave
//! their results. Two modes, and the difference is what the reader is asking for:
//!
//! * [`BlendMode::Interleave`] — alternate, keeping each side's own order. This is the "show me both, fairly" reading.
//! * [`BlendMode::Union`] — every row from both sides, deduplicated, in each side's own order. This is the "show me
//!   everything that matches either" reading.
//!
//! A third mode was considered and rejected: weighting one side more heavily needs a weight, a weight needs an interface, and a
//! weighted blend is a ranking question dressed as a filter — the reader who wants that wants a single filter with both
//! conditions, which the query language already expresses.
//!
//! # The keybind
//!
//! One digit, `1`–`9`, on the number row. `0` is excluded deliberately: on almost every keyboard layout it is the key next to
//! `backspace` used for nothing in a browser UI, and a binding there would be pressed by accident more often than on purpose.
//! Ten is not a keyboard outcome.
//!
//! A keybind is a *shortcut*, so a collision is refused rather than resolved: two filters on one key means the reader pressed
//! one key and got a surface they did not expect, and the second one is unreachable in a way they cannot see.

/// How two saved filters' results are combined.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlendMode {
    /// One row from the left, one from the right, and so on, keeping each side's own order.
    Interleave,
    /// Every row from the left, then every row from the right, with a row that appears in both kept once (at its left-hand
    /// position).
    Union,
}

impl BlendMode {
    /// The stored spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Interleave => "interleave",
            Self::Union => "union",
        }
    }

    /// Parse a stored spelling. Unknown spellings are an error rather than a default, because a blend that silently became
    /// interleaved is a blend the reader did not ask for.
    ///
    /// # Errors
    ///
    /// Returns a message naming the two modes when the spelling is neither.
    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw.trim() {
            "interleave" => Ok(Self::Interleave),
            "union" => Ok(Self::Union),
            other => Err(format!(
                "'{other}' is not a blend mode; the modes are 'interleave' and 'union'"
            )),
        }
    }
}

/// The first digit a keybind may use.
pub const MIN_KEYBIND: i64 = 1;

/// The last digit a keybind may use. Nine, not ten: see the module doc.
pub const MAX_KEYBIND: i64 = 9;

/// Whether a number is a usable keybind.
#[must_use]
pub fn keybind_is_valid(keybind: i64) -> bool {
    (MIN_KEYBIND..=MAX_KEYBIND).contains(&keybind)
}

/// Blend two ordered lists.
///
/// Both sides keep their own order; nothing is sorted, scored or re-ranked here. A blend that reordered either side would be a
/// third ranking rather than a view of two, and the reader who built each filter would not recognise their own results.
#[must_use]
pub fn blend<T: Clone + PartialEq>(left: &[T], right: &[T], mode: BlendMode) -> Vec<T> {
    match mode {
        BlendMode::Interleave => {
            let mut out: Vec<T> = Vec::with_capacity(left.len() + right.len());
            let mut left_iter = left.iter();
            let mut right_iter = right.iter();
            // Alternating *by position*, not by dedup: a row on both sides is the same recommendation and appears twice only if
            // both sides put it at the same turn, which is the honest picture of "both filters want you to read this".
            loop {
                let next_left = left_iter.next();
                let next_right = right_iter.next();
                if next_left.is_none() && next_right.is_none() {
                    break;
                }
                if let Some(row) = next_left {
                    out.push(row.clone());
                }
                if let Some(row) = next_right {
                    out.push(row.clone());
                }
            }
            out
        }
        BlendMode::Union => {
            let mut out: Vec<T> = Vec::with_capacity(left.len() + right.len());
            for row in left {
                if !out.contains(row) {
                    out.push(row.clone());
                }
            }
            for row in right {
                if !out.contains(row) {
                    out.push(row.clone());
                }
            }
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interleave_alternates_and_each_side_keeps_its_order() {
        let left = vec!["a1", "a2", "a3"];
        let right = vec!["b1", "b2"];
        assert_eq!(
            blend(&left, &right, BlendMode::Interleave),
            vec!["a1", "b1", "a2", "b2", "a3"]
        );
    }

    #[test]
    fn interleave_does_not_drop_the_longer_side() {
        let left: Vec<&str> = vec![];
        let right = vec!["b1", "b2", "b3"];
        assert_eq!(
            blend(&left, &right, BlendMode::Interleave),
            vec!["b1", "b2", "b3"]
        );
        assert_eq!(
            blend(&right, &left, BlendMode::Interleave),
            vec!["b1", "b2", "b3"],
            "an empty left side is not an error, and the right side is still whole"
        );
    }

    #[test]
    fn union_keeps_left_order_then_the_rest_and_deduplicates() {
        let left = vec!["a1", "shared", "a2"];
        let right = vec!["shared", "b1"];
        assert_eq!(
            blend(&left, &right, BlendMode::Union),
            vec!["a1", "shared", "a2", "b1"],
            "a row in both sides keeps its left-hand position and is not repeated"
        );
    }

    #[test]
    fn a_blend_of_two_empty_filters_is_empty() {
        let none: Vec<&str> = Vec::new();
        assert!(blend(&none, &none, BlendMode::Interleave).is_empty());
        assert!(blend(&none, &none, BlendMode::Union).is_empty());
    }

    #[test]
    fn a_keybind_is_one_digit_from_one_to_nine() {
        assert!(keybind_is_valid(1));
        assert!(keybind_is_valid(9));
        assert!(!keybind_is_valid(0), "zero is the key beside backspace");
        assert!(!keybind_is_valid(10), "there is no tenth number key");
        assert!(!keybind_is_valid(-1));
    }

    #[test]
    fn a_blend_mode_round_trips_and_an_unknown_one_is_an_error() {
        for mode in [BlendMode::Interleave, BlendMode::Union] {
            assert_eq!(BlendMode::parse(mode.as_str()), Ok(mode));
        }
        let error = BlendMode::parse("weighted").expect_err("'weighted' is not a mode");
        assert!(error.contains("interleave"), "{error}");
        assert!(error.contains("union"), "{error}");
    }
}

//! The reader's gravity dial, as arithmetic (M64-09, spec §16.5; ladder: M69-26).
//!
//! Kept pure and separate from the route because this is the part that decides what a reader is allowed to see, and a rule
//! about a reader's control should be testable without a database, a session or an HTTP request.

/// Why a reader's request cannot be honoured.
#[derive(Debug, Clone, PartialEq)]
pub enum DialRefusal {
    /// The instance is locked: the operator pinned the pull and the reader cannot move it.
    Locked,
    /// A number outside `0.0..=1.0`. Refused rather than clamped — a reader who typed 5.0 meant something, and silently
    /// serving 1.0 answers a question they did not ask.
    OutOfRange,
    /// Above the instance's own strength. The dial narrows; it never widens.
    AboveInstance(f64),
}

/// The strength in force for one reader on one surface.
///
/// `instance` is already ceiling-clamped by `TasteConfig::strength_for`. A stored value narrows it, and is clamped on **read**
/// so that an operator lowering the instance does not leave readers pinned above the new number by rows that were legal when
/// written.
pub fn effective(instance: f64, stored: Option<f64>) -> f64 {
    match stored {
        Some(stored) => stored.clamp(0.0, instance),
        None => instance,
    }
}

/// What to store for a reader's request, or why it is refused.
pub fn narrow(instance: f64, requested: f64, locked: bool) -> Result<f64, DialRefusal> {
    if locked {
        return Err(DialRefusal::Locked);
    }
    if !(0.0..=1.0).contains(&requested) || requested.is_nan() {
        return Err(DialRefusal::OutOfRange);
    }
    if requested > instance + f64::EPSILON {
        return Err(DialRefusal::AboveInstance(instance));
    }
    Ok(requested)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dial_narrows_and_never_widens() {
        assert_eq!(narrow(0.15, 0.0, false), Ok(0.0), "a reader may switch gravity off");
        assert_eq!(narrow(0.15, 0.10, false), Ok(0.10));
        assert_eq!(narrow(0.15, 0.15, false), Ok(0.15), "asking for what they already get is allowed");
        assert_eq!(
            narrow(0.15, 0.30, false),
            Err(DialRefusal::AboveInstance(0.15)),
            "a reader may not choose a stronger hand than the operator configured"
        );
    }

    #[test]
    fn a_locked_instance_cannot_be_moved_by_the_reader_even_out_of_range() {
        // The lock is checked first: a refused number on a locked instance is still a locked instance, and telling the reader
        // "5.0 is out of range" would suggest that 0.15 would have worked.
        assert_eq!(narrow(0.15, 5.0, true), Err(DialRefusal::Locked));
        assert_eq!(narrow(0.15, 0.0, true), Err(DialRefusal::Locked));
    }

    #[test]
    fn out_of_range_is_refused_rather_than_clamped() {
        assert_eq!(narrow(0.5, -0.1, false), Err(DialRefusal::OutOfRange));
        assert_eq!(narrow(1.0, 1.5, false), Err(DialRefusal::OutOfRange));
        assert_eq!(narrow(0.5, f64::NAN, false), Err(DialRefusal::OutOfRange));
    }

    #[test]
    fn an_operator_lowering_the_instance_pulls_every_reader_down_with_it() {
        // 0.30 was legal when the instance ran 0.40. When it drops to 0.10, the reader gets 0.10 — not 0.30, and not a
        // silent 0.10 that then looks like a choice they made.
        assert_eq!(effective(0.10, Some(0.30)), 0.10);
        assert_eq!(effective(0.15, None), 0.15, "no stored choice means the instance's value");
        assert_eq!(effective(0.15, Some(0.0)), 0.0, "an explicit zero stays zero");
    }
}

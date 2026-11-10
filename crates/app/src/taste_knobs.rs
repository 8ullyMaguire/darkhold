//! M45-46 — which taste knobs are actually in force.
//!
//! `instance_taste_settings` (0077) records what an operator set through `/operator/taste-profile`, and until this
//! module existed **nothing on a reading path read it**: the feed took its strength and its weighting policy from
//! `darkhold.toml` alone. A panel that writes a row no surface consults is a panel that lies, and "preview before
//! apply" cannot be built on top of it — a preview of a change that would not happen is a fiction with a UI.
//!
//! Two decisions, both about not inventing meaning:
//!
//! 1. **A stored value beats the file.** The admin read door already documents it ("a stored dimension set wins over
//!    the config"), and two precedence rules for the same two sources would be a bug waiting to be reported.
//! 2. **The unit is basis points of the ceiling** — 10_000 is the instance's own `gravity_ceiling`. The column is an
//!    `INTEGER` with no unit comment, so the unit has to be *stated* somewhere that both the write door and the read
//!    path can point at; this is that place. A negative stored value is not a strength and is treated as unset (the
//!    config applies) rather than clamped to zero, because clamping would silently turn a nonsense number into "off".

use crate::state::AppState;

/// The weighting policies the write door accepts.
///
/// Shared so the store, the admin door and the operator's preview cannot disagree about the vocabulary: a preview that
/// accepted a policy the write door refuses would show a page that could never be served.
pub const SIGNAL_WEIGHT_MODES: [&str; 4] = ["taste_weighted", "balanced", "popularity", "uniform"];

/// Where the value in force came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The instance's `darkhold.toml`.
    Config,
    /// The operator's stored row.
    Stored,
}

/// The knobs in force at one surface.
#[derive(Debug, Clone)]
pub struct Knobs {
    /// The strength actually served, already clamped to the ceiling.
    pub strength: f64,
    /// The weighting policy actually served.
    pub signal_weight_mode: String,
    /// Which of the two sources won.
    pub source: Source,
}

/// Which knobs are in force at `surface`.
pub async fn effective(state: &AppState, surface: &str) -> Knobs {
    let taste = &state.config().taste;
    let fallback = || Knobs {
        strength: taste.strength_for(surface),
        signal_weight_mode: taste.signal_weight_mode.clone(),
        source: Source::Config,
    };
    let row = match darkhold_db::instance_taste_settings::read(state.db()).await {
        Ok(row) => row,
        Err(error) => {
            // A failed read is not "no settings": it is a database this instance cannot currently answer with, so the
            // config applies and the failure is logged rather than turned into a silent zero-strength page.
            tracing::warn!(%error, "taste knobs unavailable; the config applies");
            return fallback();
        }
    };
    let Some(row) = row else {
        return fallback();
    };
    let Some(strength) = from_basis_points(row.gravity_strength, taste.gravity_ceiling) else {
        tracing::warn!(
            stored = row.gravity_strength,
            "a stored gravity strength outside 0..=10000 basis points is not a strength; the config applies"
        );
        return fallback();
    };
    Knobs {
        strength,
        signal_weight_mode: row.signal_weight_mode,
        source: Source::Stored,
    }
}

/// Convert a stored strength to the fraction the ranking takes.
///
/// `None` for anything that cannot mean a strength: a negative number, or one above the ceiling's own basis points.
/// Returning `None` rather than clamping is deliberate — a row of `99999` was written by something that did not share
/// this unit, and pretending it means "as strong as possible" would make the console's oldest mistake permanent.
fn from_basis_points(stored: i64, ceiling: f64) -> Option<f64> {
    if !(0..=10_000).contains(&stored) {
        return None;
    }
    Some((stored as f64 / 10_000.0).clamp(0.0, ceiling))
}

/// The knobs a *proposed* change would put in force, without writing anything.
///
/// Errors are field-level sentences, because each one is an operator mistyping a number in a form: the preview must say
/// which field and why, exactly as the write door does, or it teaches a different contract than the door it previews.
pub fn proposed(
    base: &Knobs,
    ceiling: f64,
    strength_bp: Option<i64>,
    signal_weight_mode: Option<&str>,
) -> Result<Knobs, (&'static str, String)> {
    let strength = match strength_bp {
        Some(value) => from_basis_points(value, ceiling).ok_or_else(|| {
            (
                "gravity_strength",
                format!("a strength is 0..=10000 basis points of the ceiling, not {value}"),
            )
        })?,
        None => base.strength,
    };
    let mode = match signal_weight_mode {
        Some(value) if SIGNAL_WEIGHT_MODES.contains(&value) => value.to_owned(),
        Some(value) => {
            return Err((
                "signal_weight_mode",
                format!(
                    "signal_weight_mode must be one of {SIGNAL_WEIGHT_MODES:?}, not \"{value}\""
                ),
            ))
        }
        None => base.signal_weight_mode.clone(),
    };
    Ok(Knobs {
        strength,
        signal_weight_mode: mode,
        source: base.source,
    })
}

#[cfg(test)]
mod tests {
    use super::{from_basis_points, proposed, Knobs, Source};

    #[test]
    fn the_unit_is_basis_points_of_the_ceiling() {
        assert_eq!(from_basis_points(750, 0.5), Some(0.075));
        assert_eq!(
            from_basis_points(5_000, 0.5),
            Some(0.5),
            "clamped by the ceiling"
        );
        assert_eq!(
            from_basis_points(0, 0.5),
            Some(0.0),
            "off is a value, not a hole"
        );
        assert_eq!(
            from_basis_points(-250, 0.5),
            None,
            "a negative strength is not a strength"
        );
        assert_eq!(
            from_basis_points(10_001, 0.5),
            None,
            "above the scale, so it is a different unit"
        );
    }

    #[test]
    fn a_proposal_answers_the_same_contract_as_the_write_door() {
        let base = Knobs {
            strength: 0.0,
            signal_weight_mode: "taste_weighted".to_owned(),
            source: Source::Config,
        };
        let ok = proposed(&base, 0.5, Some(2_000), Some("balanced")).expect("a valid proposal");
        assert!((ok.strength - 0.2).abs() < f64::EPSILON);
        assert_eq!(ok.signal_weight_mode, "balanced");

        let (field, why) = proposed(&base, 0.5, Some(-1), None).expect_err("a refusal");
        assert_eq!(field, "gravity_strength");
        assert!(why.contains("0..=10000"), "{why}");

        let (field, why) = proposed(&base, 0.5, None, Some("vibes")).expect_err("a refusal");
        assert_eq!(field, "signal_weight_mode");
        assert!(why.contains("balanced"), "{why}");

        // Omitted fields keep what is in force, so a preview of one knob does not silently zero the other.
        let partial = proposed(&base, 0.5, None, None).expect("a valid proposal");
        assert_eq!(partial.signal_weight_mode, "taste_weighted");
        assert!((partial.strength - 0.0).abs() < f64::EPSILON);
    }
}

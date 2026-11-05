//! M45-32 — the trend radar: which fandoms are *arriving*, so an event or a bounty can be timed.
//!
//! # The decision rule, fixed before the data
//!
//! A fandom is **emerging** when it gained at least [`MIN_RECENT`] works in the recent window *and* its
//! rate is at least [`RATIO`] times its own baseline rate. Both halves matter: the ratio alone calls a
//! fandom that went 1 → 3 emerging, and the count alone calls the biggest fandom emerging every week.
//! A fandom whose baseline is zero and which now has works is *new*, not infinitely emerging — the
//! verdict says so rather than printing a division by zero.
//!
//! **No verdict is an answer.** Below [`MIN_RECENT`] works, in a window with no baseline at all, the
//! assessment says which of the two it is. An empty verdict would be read as "not emerging", which is a
//! claim this data cannot support at that sample size.

/// The fewest works in the recent window before any verdict is given.
pub const MIN_RECENT: i64 = 3;

/// How much faster than its own baseline a fandom must move to be called emerging.
pub const RATIO: f64 = 2.0;

/// What the radar says about one fandom.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Arriving: enough works, and at least [`RATIO`] times the baseline rate.
    Emerging,
    /// The baseline was empty and it has works now.
    New,
    /// Between half and [`RATIO`] times baseline.
    Steady,
    /// Under half its baseline rate.
    Fading,
    /// Not enough evidence, with the reason.
    NoVerdict,
}

impl Verdict {
    /// The word the interface prints.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Emerging => "emerging",
            Self::New => "new",
            Self::Steady => "steady",
            Self::Fading => "fading",
            Self::NoVerdict => "no_verdict",
        }
    }
}

/// One fandom's counts, as the store reported them.
#[derive(Debug, Clone, PartialEq)]
pub struct Counts {
    pub recent: i64,
    /// Works that arrived during the baseline window.
    pub baseline: i64,
    /// How many weeks the baseline covers, so the rate is per-week on both sides.
    pub baseline_weeks: f64,
    /// How many weeks the recent window covers. A ratio between two windows of different lengths is
    /// not a ratio: eight recent works in two weeks is four a week, not eight.
    pub recent_weeks: f64,
}

/// The radar's answer for one fandom.
#[derive(Debug, Clone, PartialEq)]
pub struct Assessment {
    /// Recent works against the baseline's per-week rate; `None` when there is no baseline to divide by.
    pub ratio: Option<f64>,
    pub verdict: Verdict,
    /// A sentence for the interface, and the reason a verdict is absent.
    pub reason: String,
}

/// Assess one fandom.
#[must_use]
pub fn assess(counts: &Counts) -> Assessment {
    if counts.recent < MIN_RECENT {
        return Assessment {
            ratio: None,
            verdict: Verdict::NoVerdict,
            reason: format!(
                "{} works in the recent window; {MIN_RECENT} are needed before a ratio means anything",
                counts.recent
            ),
        };
    }
    if counts.baseline_weeks <= 0.0 || counts.recent_weeks <= 0.0 {
        return Assessment {
            ratio: None,
            verdict: Verdict::NoVerdict,
            reason: "no baseline window configured".to_owned(),
        };
    }
    let baseline_rate = counts.baseline as f64 / counts.baseline_weeks;
    if baseline_rate <= 0.0 {
        return Assessment {
            ratio: None,
            verdict: Verdict::New,
            reason: format!(
                "{} works, none in the baseline window — this fandom did not exist here a window ago",
                counts.recent
            ),
        };
    }
    let recent_rate = counts.recent as f64 / counts.recent_weeks;
    let ratio = recent_rate / baseline_rate;
    let verdict = if ratio >= RATIO {
        Verdict::Emerging
    } else if ratio <= 1.0 / RATIO {
        Verdict::Fading
    } else {
        Verdict::Steady
    };
    Assessment {
        ratio: Some(ratio),
        verdict,
        reason: format!(
            "{ratio:.1}x its baseline rate: {recent_rate:.1} works per week now against \
             {baseline_rate:.1} per baseline week"
        ),
    }
}

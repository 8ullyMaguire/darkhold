//! M16-13 — the theme model: tokens, slots, scoping, preview, reset, safe mode, accessibility.
//!
//! A theme is **data, not a document**. M70-06 gives a reader a stylesheet (a scanner and a size cap, because
//! CSS is what it accepts); this is the layer that makes a theme composable and undoable, and it exists
//! because a stylesheet is the wrong primitive for the parts that must go right:
//!
//! * **Design tokens.** A theme states *values* for a closed set of names — surfaces, ink, accent, measure.
//!   It cannot write selectors, so it cannot target an element, so it cannot reach a surface it was not
//!   given. That is the difference between "we ask themes not to hide the safety controls" and "a theme has
//!   no vocabulary for hiding anything".
//! * **Constrained layout slots.** A theme may place the named regions in one of a closed set of
//!   arrangements. It cannot remove one. A layout that could hide `nav` would be a layout that can hide the
//!   way back to settings.
//! * **Scoping.** An instance theme dresses the building; a reader theme recolours one room. A reader's theme
//!   may set palette tokens and nothing else — no layout, because layout is a property of the building.
//! * **Preview, reset, safe mode.** A preview is visible to the session that asked for it and expires; reset
//!   is a delete, not a write of defaults; safe mode is the state in which *no* theme is consulted, so a
//!   theme that breaks a renderer cannot be the theme that hides the way out of itself.
//! * **Accessibility checks.** Contrast is computed here, from the tokens themselves, so "this theme is
//!   readable" is arithmetic rather than a reviewer's opinion — and a theme that fails cannot be published.
//!
//! # The cannot-hide rule, as a mechanism
//!
//! §21.7 names four surfaces a theme may not hide: the positivity filter status, the feedback mechanism, the
//! safety controls, and extension management. A rule in prose does not survive contact with CSS, because CSS
//! can select anything in the document it is applied to. The mechanism is therefore positional: those four
//! surfaces render in the **host document**, outside the element the theme's custom properties are applied
//! to, and the theme root is a shadow root — so the theme's selectors cannot cross the boundary and its
//! custom properties do not inherit into it. [`Surface::mechanism`] spells that out per surface so the
//! answer is checkable against the renderer rather than remembered.

use std::collections::BTreeMap;

/// The closed set of design tokens a theme may set.
///
/// Every name here is a decision that something in the shell reads. A token nobody reads is a theme writing
/// into a void, and a token that is missing from this list cannot be set at all — which is what keeps the
/// vocabulary small enough to be reviewable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Token {
    /// Page background.
    Surface,
    /// Raised panels, cards, menus.
    SurfaceRaised,
    /// Body text.
    Ink,
    /// Secondary text: timestamps, counts, hints.
    InkMuted,
    /// The one signature accent (ADR 0007).
    Accent,
    /// Text on the accent, for filled buttons and chips.
    AccentInk,
    /// Hairlines and dividers.
    Border,
    /// The reading surface, which is warmer than the page (DIRECTIONS: The Reading Room).
    ReadingSurface,
    /// Body text on the reading surface.
    ReadingInk,
    /// The largest text measure, in `ch`, so a theme can widen or narrow the column.
    Measure,
}

impl Token {
    /// The CSS custom property name.
    #[must_use]
    pub fn css_name(self) -> &'static str {
        match self {
            Self::Surface => "--dh-surface",
            Self::SurfaceRaised => "--dh-surface-raised",
            Self::Ink => "--dh-ink",
            Self::InkMuted => "--dh-ink-muted",
            Self::Accent => "--dh-accent",
            Self::AccentInk => "--dh-accent-ink",
            Self::Border => "--dh-border",
            Self::ReadingSurface => "--dh-reading-surface",
            Self::ReadingInk => "--dh-reading-ink",
            Self::Measure => "--dh-measure",
        }
    }

    /// Whether the token carries a colour rather than a length.
    #[must_use]
    pub fn is_colour(self) -> bool {
        !matches!(self, Self::Measure)
    }

    /// Every token, in the order a stylesheet should declare them.
    #[must_use]
    pub fn all() -> &'static [Token] {
        &[
            Self::Surface,
            Self::SurfaceRaised,
            Self::Ink,
            Self::InkMuted,
            Self::Accent,
            Self::AccentInk,
            Self::Border,
            Self::ReadingSurface,
            Self::ReadingInk,
            Self::Measure,
        ]
    }
}

/// The text/background pairs a theme must keep readable, and at what ratio.
///
/// The requirement is WCAG 2.2 AA: 4.5:1 for body text, 3:1 for large text and for non-text boundaries that
/// carry meaning (a divider that disappears is a layout that has no dividers).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Requirement {
    /// 4.5:1 — body text.
    BodyText,
    /// 3:1 — large text and meaningful non-text.
    LargeOrBoundary,
}

impl Requirement {
    /// The minimum ratio, in hundredths, so the comparison is integer arithmetic.
    #[must_use]
    pub fn minimum_hundredths(self) -> u32 {
        match self {
            Self::BodyText => 450,
            Self::LargeOrBoundary => 300,
        }
    }
}

/// The pairs every theme is checked against.
///
/// Not derived from what the palette happens to contain: a theme that omits `reading_ink` still gives the
/// reader body text, and the checker must have an opinion about it. Missing tokens fall back to the core
/// values, which are themselves checked, so the pairs always have two colours.
///
/// `--dh-border` is deliberately **not** here. A hairline divider is decoration, not a WCAG 1.4.11
/// "graphical object required to understand the content", and holding it to 3:1 would force every theme
/// either to draw a garish line or to be refused — the check would be measuring taste, not readability.
/// Where a boundary carries meaning (an input's edge, a chip's outline) the shell draws it with `Ink` or
/// `Accent`, which are enforced pairs.
#[must_use]
pub fn required_pairs() -> &'static [(Token, Token, Requirement)] {
    &[
        (Token::Ink, Token::Surface, Requirement::BodyText),
        (Token::Ink, Token::SurfaceRaised, Requirement::BodyText),
        (Token::InkMuted, Token::Surface, Requirement::BodyText),
        (Token::AccentInk, Token::Accent, Requirement::BodyText),
        (Token::ReadingInk, Token::ReadingSurface, Requirement::BodyText),
    ]
}

/// A contrast failure, named by the pair that failed.
#[derive(Debug, Clone, PartialEq)]
pub struct ContrastFailure {
    /// Foreground token.
    pub foreground: Token,
    /// Background token.
    pub background: Token,
    /// What the pair is for.
    pub requirement: Requirement,
    /// The measured ratio in hundredths.
    pub measured_hundredths: u32,
}

impl ContrastFailure {
    /// A sentence naming both tokens and both numbers, so a fix is a lookup rather than a search.
    #[must_use]
    pub fn message(&self) -> String {
        format!(
            "{} on {} measures {:.2}:1; it needs {:.2}:1",
            self.foreground.css_name(),
            self.background.css_name(),
            f64::from(self.measured_hundredths) / 100.0,
            f64::from(self.requirement.minimum_hundredths()) / 100.0,
        )
    }
}

/// The core palette: the values a theme inherits when it says nothing.
///
/// This is also the safe-mode palette — the one state in which no theme is consulted — so a theme cannot
/// change what "reset" means. The values are ADR 0007's, measured to pass every pair in [`required_pairs`].
#[must_use]
pub fn core_palette() -> BTreeMap<Token, String> {
    BTreeMap::from([
        (Token::Surface, "#0f0f12".to_owned()),
        (Token::SurfaceRaised, "#181820".to_owned()),
        (Token::Ink, "#e8e6e0".to_owned()),
        (Token::InkMuted, "#a8a49c".to_owned()),
        (Token::Accent, "#c9a227".to_owned()),
        (Token::AccentInk, "#12100a".to_owned()),
        (Token::Border, "#3a3a44".to_owned()),
        (Token::ReadingSurface, "#1a1a1f".to_owned()),
        (Token::ReadingInk, "#ece9e2".to_owned()),
        (Token::Measure, "68ch".to_owned()),
    ])
}

/// Parse `#rrggbb` into linear-light channels.
///
/// Three- and four-digit forms are refused rather than expanded: a theme that writes `#fff` and means the
/// same thing as `#ffffff` is fine, but the expenser is the browser's, and a contrast check that guesses the
/// expansion can disagree with what renders. One accepted form, checked the same way everywhere.
fn parse_hex(value: &str) -> Option<[f64; 3]> {
    let hex = value.strip_prefix('#')?;
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let channel = |i: usize| -> f64 {
        let byte = u8::from_str_radix(&hex[i..i + 2], 16).expect("validated hex");
        let srgb = f64::from(byte) / 255.0;
        // WCAG 2.x relative luminance: the sRGB transfer function, not a plain square.
        if srgb <= 0.039_28 {
            srgb / 12.92
        } else {
            ((srgb + 0.055) / 1.055).powf(2.4)
        }
    };
    Some([channel(0), channel(2), channel(4)])
}

/// The WCAG contrast ratio between two `#rrggbb` colours, in hundredths.
///
/// `None` when either value is not a colour — which the validator refuses before this is reached, but this
/// function does not panic on unvalidated input.
#[must_use]
pub fn contrast_hundredths(a: &str, b: &str) -> Option<u32> {
    let (first, second) = (parse_hex(a)?, parse_hex(b)?);
    let luminance = |c: [f64; 3]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
    let (la, lb) = (luminance(first), luminance(second));
    let (lighter, darker) = if la >= lb { (la, lb) } else { (lb, la) };
    let ratio = (lighter + 0.05) / (darker + 0.05);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Some((ratio * 100.0).round() as u32)
}

/// Why a theme was refused.
#[derive(Debug, Clone, PartialEq)]
pub enum Refusal {
    /// The theme has no name.
    Unnamed,
    /// A token value that is not of the token's own kind.
    BadValue {
        /// Which token.
        token: Token,
        /// What was supplied.
        value: String,
    },
    /// A contrast pair below its minimum.
    Contrast(ContrastFailure),
    /// A reader theme tried to set a layout; layout belongs to the instance.
    ReaderCannotLayOut,
    /// A layout that would remove a region.
    SlotRemoved {
        /// The slot.
        slot: Slot,
    },
}

impl Refusal {
    /// A sentence a person can act on.
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Self::Unnamed => "a theme needs a name".to_owned(),
            Self::BadValue { token, value } => format!(
                "{} is {value:?}; {} takes {}",
                token.css_name(),
                token.css_name(),
                if token.is_colour() {
                    "a #rrggbb colour"
                } else {
                    "a length in ch, like 68ch"
                }
            ),
            Self::Contrast(failure) => failure.message(),
            Self::ReaderCannotLayOut => {
                "a reader's theme may recolour the page but not rearrange it; layout belongs to the instance"
                    .to_owned()
            }
            Self::SlotRemoved { slot } => format!(
                "the {} region cannot be removed; a theme that can hide it can hide the way to settings",
                slot.name()
            ),
        }
    }
}

/// A named region of the shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Slot {
    /// Site name and primary actions.
    Header,
    /// Primary navigation.
    Nav,
    /// The main column.
    Content,
    /// Secondary column.
    Aside,
    /// The reader's own bar (progress, font size, save).
    ReaderBar,
    /// End matter.
    Footer,
}

impl Slot {
    /// The wire name.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Header => "header",
            Self::Nav => "nav",
            Self::Content => "content",
            Self::Aside => "aside",
            Self::ReaderBar => "reader_bar",
            Self::Footer => "footer",
        }
    }

    /// Every slot, all of which must be placed.
    #[must_use]
    pub fn all() -> &'static [Slot] {
        &[
            Self::Header,
            Self::Nav,
            Self::Content,
            Self::Aside,
            Self::ReaderBar,
            Self::Footer,
        ]
    }
}

/// How a region is placed. A closed set: no offsets, no absolute positioning, no z-index.
///
/// A theme that could position regions freely could stack one over another, which is hiding with extra steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// In the flow, full width.
    Stacked,
    /// Side by side with its neighbour.
    Inline,
    /// In the narrow column.
    Aside,
    /// Spanning the full width beneath the others.
    Full,
}

impl Placement {
    /// The wire name.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Stacked => "stacked",
            Self::Inline => "inline",
            Self::Aside => "aside",
            Self::Full => "full",
        }
    }

    /// Parse a wire name, refusing anything outside the set.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "stacked" => Some(Self::Stacked),
            "inline" => Some(Self::Inline),
            "aside" => Some(Self::Aside),
            "full" => Some(Self::Full),
            _ => None,
        }
    }
}

/// The core layout: the arrangement a theme inherits when it says nothing.
#[must_use]
pub fn core_layout() -> BTreeMap<Slot, Placement> {
    BTreeMap::from([
        (Slot::Header, Placement::Stacked),
        (Slot::Nav, Placement::Inline),
        (Slot::Content, Placement::Stacked),
        (Slot::Aside, Placement::Aside),
        (Slot::ReaderBar, Placement::Inline),
        (Slot::Footer, Placement::Full),
    ])
}

/// Whose appearance a theme states.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// The instance's own theme, set by an operator.
    Instance,
    /// A reader's own theme: palette only.
    Reader(String),
}

impl Scope {
    /// Whether a theme in this scope may set layout.
    #[must_use]
    pub fn may_lay_out(&self) -> bool {
        matches!(self, Self::Instance)
    }
}

/// A theme as stored: a name, token values, and (for an instance theme) slot placements.
#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    /// Shown in the settings list.
    pub name: String,
    /// Token values; any token absent falls back to [`core_palette`].
    pub palette: BTreeMap<Token, String>,
    /// Slot placements; an instance theme may leave them absent for the core layout.
    pub layout: BTreeMap<Slot, Placement>,
}

impl Theme {
    /// Validate a theme for a scope, without publishing it.
    ///
    /// Draft validation is the same check as publishing, with one exception: contrast failures are returned
    /// so a form can show them live, and the caller decides whether to save. Publishing refuses them
    /// (`publish` below); a draft that refused to save would be a form nobody could fill in.
    pub fn validate(&self, scope: &Scope) -> Vec<Refusal> {
        let mut refusals = Vec::new();
        if self.name.trim().is_empty() {
            refusals.push(Refusal::Unnamed);
        }
        for (token, value) in &self.palette {
            let ok = if token.is_colour() {
                parse_hex(value).is_some()
            } else {
                // `68ch`, `75ch` … a length in ch, bounded to something a page can render.
                value
                    .strip_suffix("ch")
                    .and_then(|n| n.trim().parse::<u32>().ok())
                    .is_some_and(|n| (30..=120).contains(&n))
            };
            if !ok {
                refusals.push(Refusal::BadValue {
                    token: *token,
                    value: value.clone(),
                });
            }
        }
        if !scope.may_lay_out() && !self.layout.is_empty() {
            refusals.push(Refusal::ReaderCannotLayOut);
        }
        for failure in self.contrast_failures() {
            refusals.push(Refusal::Contrast(failure));
        }
        refusals
    }

    /// The effective palette: the theme's values over the core ones.
    #[must_use]
    pub fn palette_with_core(&self) -> BTreeMap<Token, String> {
        let mut palette = core_palette();
        palette.extend(self.palette.clone());
        palette
    }

    /// The effective layout: the theme's placements over the core ones.
    ///
    /// A theme may change a placement but not remove a slot: a missing slot means "core", never "gone". That
    /// is enforced at the type level (`Slot::all()` is what a renderer walks) and checked again here by
    /// filling in the core value.
    #[must_use]
    pub fn layout_with_core(&self) -> BTreeMap<Slot, Placement> {
        let mut layout = core_layout();
        layout.extend(self.layout.clone());
        layout
    }

    /// Every contrast pair that fails, measured against the effective palette.
    #[must_use]
    pub fn contrast_failures(&self) -> Vec<ContrastFailure> {
        let palette = self.palette_with_core();
        required_pairs()
            .iter()
            .filter_map(|(foreground, background, requirement)| {
                let (Some(fg), Some(bg)) = (palette.get(foreground), palette.get(background)) else {
                    return None;
                };
                let measured = contrast_hundredths(fg, bg)?;
                (measured < requirement.minimum_hundredths()).then_some(ContrastFailure {
                    foreground: *foreground,
                    background: *background,
                    requirement: *requirement,
                    measured_hundredths: measured,
                })
            })
            .collect()
    }

    /// The declarations this theme emits, in stylesheet order.
    ///
    /// Only custom properties are emitted — that is the whole mechanism: a value stream, not a rule set.
    #[must_use]
    pub fn declarations(&self) -> String {
        self.palette_with_core()
            .iter()
            .map(|(token, value)| format!("{}: {value};", token.css_name()))
            .collect::<Vec<String>>()
            .join("\n")
    }
}

/// The state in which no theme is consulted.
///
/// Entered by an operator, and by the shell itself when a theme fails to apply (the browser-side rule: if the
/// theme loader throws, the page renders core). Safe mode is a *state*, not a theme: it cannot be saved over,
/// and leaving it is an explicit act, because the thing that broke may be the thing that would have shown the
/// way out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafeMode {
    /// Whether it is on.
    pub active: bool,
    /// Why it was entered, as a sentence for the operator console.
    pub reason: Option<String>,
}

impl SafeMode {
    /// Off, which is the normal state.
    #[must_use]
    pub fn off() -> Self {
        Self {
            active: false,
            reason: None,
        }
    }

    /// On, with a reason. An empty reason is refused: a safe mode nobody can explain is a bug report with no
    /// subject.
    #[must_use]
    pub fn on(reason: impl Into<String>) -> Self {
        Self {
            active: true,
            reason: Some(reason.into()),
        }
    }
}

/// One of the four surfaces a theme may not hide (§21.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    /// Whether a positivity filter is on, and its strength.
    PositivityStatus,
    /// The feedback mechanism: reporting a work, a bug, a bad recommendation.
    Feedback,
    /// The safety controls: blocking, muting, leaving a space.
    SafetyControls,
    /// Extension management: which packages run, and how to stop them.
    ExtensionManagement,
}

impl Surface {
    /// The mechanism that keeps it reachable, per surface.
    ///
    /// Each is a *position* rather than a promise: the surface is rendered by the host, in the host document,
    /// and the theme's custom properties are set on a sibling element whose subtree is a shadow root. A
    /// selector inside the theme's scope cannot match across the boundary, and a custom property set outside
    /// does not inherit in. The `all: revert` reset means that even a host-level reset sheet cannot be
    /// overridden by an inherited value.
    #[must_use]
    pub fn mechanism(self) -> &'static str {
        match self {
            Self::PositivityStatus => {
                "rendered by the host above the theme root, with `all: revert` on the status bar; a theme's custom properties are set on the theme root sibling and do not cross into the host document"
            }
            Self::Feedback => {
                "the feedback affordance lives in the host document outside the theme root; the theme root is a shadow root, so the theme's selectors cannot match it"
            }
            Self::SafetyControls => {
                "safety controls render in a host-owned region with `all: revert` and a fixed stacking order; no theme placement can move or cover them because placements are flex/flow values, not positioning"
            }
            Self::ExtensionManagement => {
                "extension management is a host route reached through the shell's own navigation, which is outside the theme root and cannot be hidden by a slot placement"
            }
        }
    }

    /// Every surface, so a check can be written as a loop rather than a list in prose.
    #[must_use]
    pub fn all() -> &'static [Surface] {
        &[
            Self::PositivityStatus,
            Self::Feedback,
            Self::SafetyControls,
            Self::ExtensionManagement,
        ]
    }
}

/// What actually renders, given what is stored.
#[derive(Debug, Clone, PartialEq)]
pub enum Effective {
    /// Core appearance only, because safe mode is on.
    Core {
        /// Why.
        reason: String,
    },
    /// The published theme, or core if the instance never set one.
    Themed {
        /// The palette to emit.
        palette: BTreeMap<Token, String>,
        /// The layout to render.
        layout: BTreeMap<Slot, Placement>,
        /// Where it came from: `instance`, `reader`, `preview`.
        source: &'static str,
    },
}

/// Decide what renders.
///
/// The order is the rule and is stated once, here: **safe mode beats everything**, including a preview —
/// the preview is exactly the thing most likely to be the broken theme. Then a reader's own theme for that
/// reader, then the instance's published theme, then core.
#[must_use]
pub fn effective(
    safe_mode: &SafeMode,
    instance: Option<&Theme>,
    reader: Option<&Theme>,
    preview: Option<&Theme>,
) -> Effective {
    if safe_mode.active {
        return Effective::Core {
            reason: safe_mode
                .reason
                .clone()
                .unwrap_or_else(|| "safe mode is on".to_owned()),
        };
    }
    if let Some(preview) = preview {
        return Effective::Themed {
            palette: preview.palette_with_core(),
            layout: preview.layout_with_core(),
            source: "preview",
        };
    }
    if let Some(reader) = reader {
        // A reader theme keeps the instance's layout: palette only, which is also what `validate` enforces.
        let layout = instance.map_or_else(core_layout, Theme::layout_with_core);
        return Effective::Themed {
            palette: reader.palette_with_core(),
            layout,
            source: "reader",
        };
    }
    if let Some(instance) = instance {
        return Effective::Themed {
            palette: instance.palette_with_core(),
            layout: instance.layout_with_core(),
            source: "instance",
        };
    }
    Effective::Themed {
        palette: core_palette(),
        layout: core_layout(),
        source: "core",
    }
}

/// Whether a preview is still live.
///
/// Expiry is checked by the caller against the clock; this states the rule so a route cannot invent a
/// different one. A preview with no expiry is not a preview — it is a publication with extra steps.
#[must_use]
pub fn preview_is_live(expires_at_epoch: i64, now_epoch: i64) -> bool {
    now_epoch < expires_at_epoch
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme() -> Theme {
        Theme {
            name: "test".to_owned(),
            palette: BTreeMap::new(),
            layout: BTreeMap::new(),
        }
    }

    #[test]
    fn the_core_palette_passes_every_pair_it_claims() {
        let failures = theme().contrast_failures();
        assert!(
            failures.is_empty(),
            "the core palette must be the one state nobody can make unreadable: {failures:?}"
        );
    }

    #[test]
    fn contrast_is_the_wcag_ratio_and_refuses_non_colours() {
        // Black on white is the canonical 21:1.
        assert_eq!(contrast_hundredths("#000000", "#ffffff"), Some(2100));
        assert_eq!(contrast_hundredths("#ffffff", "#ffffff"), Some(100));
        assert_eq!(contrast_hundredths("#fff", "#ffffff"), None, "one accepted form");
        assert_eq!(contrast_hundredths("red", "#ffffff"), None);
    }

    #[test]
    fn a_theme_that_makes_text_unreadable_is_refused_with_the_measured_ratio() {
        let mut unreadable = theme();
        unreadable.palette.insert(Token::Ink, "#141418".to_owned());
        unreadable.palette.insert(Token::Surface, "#121216".to_owned());

        let failures = unreadable.contrast_failures();
        let ink = failures
            .iter()
            .find(|f| f.foreground == Token::Ink)
            .expect("ink on surface must fail");
        assert_eq!(ink.requirement, Requirement::BodyText);
        assert!(ink.measured_hundredths < 450);
        assert!(ink.message().contains("--dh-ink"), "{}", ink.message());

        let refusals = unreadable.validate(&Scope::Instance);
        assert!(refusals.iter().any(|r| matches!(r, Refusal::Contrast(_))));
    }

    #[test]
    fn a_theme_may_change_a_placement_but_not_remove_a_region() {
        let mut moved = theme();
        moved.layout.insert(Slot::Nav, Placement::Aside);

        let layout = moved.layout_with_core();
        assert_eq!(layout.len(), Slot::all().len(), "every region is still placed");
        assert_eq!(layout[&Slot::Nav], Placement::Aside, "the change is honoured");
        assert_eq!(layout[&Slot::Footer], Placement::Full, "the rest is core");
    }

    #[test]
    fn a_reader_theme_may_not_lay_out_the_building() {
        let mut reader = theme();
        assert!(reader.validate(&Scope::Reader("acct".to_owned())).is_empty());

        reader.layout.insert(Slot::Content, Placement::Inline);
        let refusals = reader.validate(&Scope::Reader("acct".to_owned()));
        assert_eq!(refusals, vec![Refusal::ReaderCannotLayOut]);
        // The same theme as an instance theme is fine: the constraint is about who it speaks for.
        assert!(reader.validate(&Scope::Instance).is_empty());
    }

    #[test]
    fn a_bad_value_names_the_token_and_what_it_takes() {
        let mut broken = theme();
        broken.palette.insert(Token::Accent, "url(https://tracker/x)".to_owned());
        broken.palette.insert(Token::Measure, "68px".to_owned());

        let refusals = broken.validate(&Scope::Instance);
        let messages: Vec<String> = refusals.iter().map(Refusal::message).collect();
        assert!(
            messages.iter().any(|m| m.contains("--dh-accent") && m.contains("#rrggbb")),
            "a value that is not a colour must be named as one: {messages:?}"
        );
        assert!(
            messages.iter().any(|m| m.contains("--dh-measure") && m.contains("68ch")),
            "{messages:?}"
        );
    }

    #[test]
    fn declarations_are_custom_properties_and_nothing_else() {
        let mut themed = theme();
        themed.palette.insert(Token::Accent, "#c9a227".to_owned());
        let css = themed.declarations();
        assert!(css.contains("--dh-accent: #c9a227;"), "{css}");
        assert!(css.contains("--dh-surface:"), "absent tokens fall back to core: {css}");
        assert!(
            !css.contains('{') && !css.contains('}'),
            "a theme emits no rule set, so it has no selector to aim: {css}"
        );
    }

    #[test]
    fn safe_mode_beats_a_preview_because_the_preview_is_the_suspect() {
        let mut instance = theme();
        instance.palette.insert(Token::Accent, "#c9a227".to_owned());
        let mut preview = theme();
        preview.palette.insert(Token::Accent, "#ff00ff".to_owned());

        let blackout = SafeMode::on("the last theme crashed the reader");
        let blacked_out = effective(&blackout, Some(&instance), None, Some(&preview));
        match blacked_out {
            Effective::Core { reason } => {
                assert!(reason.contains("crashed"), "the reason travels: {reason}");
            }
            other => panic!("safe mode must win over a live preview: {other:?}"),
        }

        // With safe mode off the preview is what a previewing session sees, and it is named as such.
        let live = effective(&SafeMode::off(), Some(&instance), None, Some(&preview));
        match live {
            Effective::Themed { source, palette, .. } => {
                assert_eq!(source, "preview");
                assert_eq!(palette[&Token::Accent], "#ff00ff");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_reader_theme_recolours_without_rearranging() {
        let mut instance = theme();
        instance.layout.insert(Slot::Aside, Placement::Stacked);
        let mut reader = theme();
        reader.palette.insert(Token::Ink, "#101014".to_owned());

        match effective(&SafeMode::off(), Some(&instance), Some(&reader), None) {
            Effective::Themed { layout, palette, source } => {
                assert_eq!(source, "reader");
                assert_eq!(layout[&Slot::Aside], Placement::Stacked, "the building stays arranged");
                assert_eq!(palette[&Token::Ink], "#101014");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn nothing_stored_means_core_and_it_says_so() {
        match effective(&SafeMode::off(), None, None, None) {
            Effective::Themed { source, palette, layout } => {
                assert_eq!(source, "core");
                assert_eq!(palette, core_palette());
                assert_eq!(layout, core_layout());
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_preview_expires() {
        assert!(preview_is_live(1_000, 999));
        assert!(!preview_is_live(1_000, 1_000), "expiry is the moment it is no longer a preview");
        assert!(!preview_is_live(1_000, 1_001));
    }

    #[test]
    fn every_protected_surface_names_a_position_not_a_promise() {
        for surface in Surface::all() {
            let mechanism = surface.mechanism();
            assert!(
                mechanism.contains("host") || mechanism.contains("outside"),
                "the mechanism must be positional: {mechanism}"
            );
        }
    }
}

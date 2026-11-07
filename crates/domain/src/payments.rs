//! §21.9's paid marketplace arithmetic, as values (M16-11).
//!
//! Money here is `i64` minor units and a currency string. Never a float: a price that cannot be represented exactly is not a price, and every
//! rounding decision below is made once, in one place, with the direction written down.
//!
//! # The four decisions
//!
//! * **The developer's share is the remainder.** The host takes `floor(gross × bp / 10_000)` and the developer gets the rest, so the two
//!   halves always add back to the gross. The alternative — rounding both halves — loses or invents a minor unit per sale, and a ledger
//!   whose rows do not sum to what was charged is a ledger nobody can audit.
//! * **A refund may be partial, and a partial refund does not revoke access.** Refusing to revoke on a partial refund is a decision: a reader
//!   who was refunded a goodwill 5% keeps what they bought, and taking it back would be a punishment for a gesture. A full refund revokes,
//!   because the money is gone and so is the sale.
//! * **An over-refund is refused, not clamped.** Clamping a bug into a correct-looking number is how a ledger hides a double refund.
//! * **Tax is computed inclusive and rounded half-up.** `tax = gross × bps / (10_000 + bps)`, so `net + tax == gross` exactly and the
//!   invoice totals reconcile. Half-up rather than truncation because a round of invoices should not systematically under-declare tax.
//!
//! # What is deliberately absent
//!
//! No processor client. §21.9 calls processor-backed payouts *optional*, and inventing an integration would mean inventing an API key, a
//! webhook shape and a failure mode that nobody has tested. `Processor::Manual` is a real processor: it records the charge and waits for an
//! operator to capture it, which is exactly how a host without a payment provider must behave.

/// An amount of money, in minor units, with its currency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Money {
    pub minor: i64,
    pub currency: String,
}

impl Money {
    /// Build an amount.
    #[must_use]
    pub fn new(minor: i64, currency: impl Into<String>) -> Self {
        Self {
            minor,
            currency: currency.into(),
        }
    }

    /// Whether this amount can be charged at all: a zero or negative price is not a price.
    #[must_use]
    pub fn is_chargeable(&self) -> bool {
        self.minor > 0
    }
}

/// The 85/15 reference split (§20.9.3), in basis points of the gross.
pub const PLATFORM_FEE_BP: i64 = 1_500;

/// The default platform fee, so a caller cannot pass a different number by accident.
#[must_use]
pub fn reference_fee_bp() -> i64 {
    PLATFORM_FEE_BP
}

/// A sale, divided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Split {
    /// What the seller receives.
    pub developer: i64,
    /// What the host keeps.
    pub host: i64,
}

impl Split {
    /// Divide `gross_minor` by `fee_bp` basis points to the host.
    ///
    /// Invalid fees are refused rather than normalised: a fee above 10 000 bp would mean the host takes more than the sale, and a negative
    /// fee would mean the host pays the seller out of nothing. Both are configuration mistakes that should stop the sale, not silently
    /// produce a number.
    #[must_use]
    pub fn of(gross_minor: i64, fee_bp: i64) -> Option<Self> {
        if gross_minor < 0 || !(0..=10_000).contains(&fee_bp) {
            return None;
        }
        let host = gross_minor * fee_bp / 10_000;
        Some(Self {
            developer: gross_minor - host,
            host,
        })
    }

    /// The same split at the reference fee.
    #[must_use]
    pub fn reference(gross_minor: i64) -> Option<Self> {
        Self::of(gross_minor, PLATFORM_FEE_BP)
    }

    /// The two halves, which always add back to what was charged.
    #[must_use]
    pub fn total(&self) -> i64 {
        self.developer + self.host
    }
}

/// A tax profile: what the host must print on an invoice.
///
/// Stored rather than compiled in, because a tax rate changes by jurisdiction and by year, and an operator who has to redeploy to change a
/// VAT rate will print the old one for a month.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaxProfile {
    /// The registered legal name, as it must appear on an invoice.
    pub legal_name: String,
    /// ISO 3166-1 alpha-2, upper case.
    pub country: String,
    /// The registration number, when the jurisdiction issues one.
    pub tax_id: Option<String>,
    /// The rate in basis points. `2000` is 20%.
    pub rate_bp: i64,
    /// The invoice number prefix, so a host's books can be told apart from any other system's.
    pub invoice_prefix: String,
}

impl TaxProfile {
    /// Validate a profile before it is stored.
    ///
    /// The refusals are the ones that would produce a wrong invoice: a rate above 100%, a country that is not two letters, an empty legal
    /// name. Everything else is a business decision the operator is allowed to make.
    pub fn validate(&self) -> Result<(), String> {
        if !(0..=10_000).contains(&self.rate_bp) {
            return Err("a tax rate is between 0% and 100%".to_owned());
        }
        if self.country.len() != 2 || !self.country.chars().all(|c| c.is_ascii_alphabetic()) {
            return Err("a country is a two-letter code".to_owned());
        }
        if self.legal_name.trim().is_empty() {
            return Err("an invoice needs a legal name".to_owned());
        }
        if self.invoice_prefix.trim().is_empty() {
            return Err("an invoice needs a number prefix".to_owned());
        }
        if self.invoice_prefix.len() > 12 {
            return Err("an invoice prefix is at most 12 characters".to_owned());
        }
        Ok(())
    }

    /// Tax contained in a tax-inclusive gross amount.
    ///
    /// `gross × bps / (10 000 + bps)`, rounded half-up, so `net + tax == gross`. The alternative (computing net from gross and tax from net)
    /// leaves a minor unit unaccounted on roughly half of all sales, and an invoice that does not add up is a support ticket.
    #[must_use]
    pub fn tax_on(&self, gross_minor: i64) -> i64 {
        if gross_minor <= 0 || self.rate_bp <= 0 {
            return 0;
        }
        let denominator = 10_000 + self.rate_bp;
        let numerator = gross_minor * self.rate_bp;
        (numerator + denominator / 2) / denominator
    }

    /// The net (pre-tax) amount: gross minus the tax it contains.
    #[must_use]
    pub fn net_of(&self, gross_minor: i64) -> i64 {
        gross_minor - self.tax_on(gross_minor)
    }
}

/// Where a payment has got to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PaymentState {
    /// Recorded, money not taken: the manual processor's whole point is that this state is visible rather than skipped.
    #[default]
    Pending,
    /// Money taken. This is the only state that grants what was bought.
    Captured,
    /// Fully returned.
    Refunded,
    /// Abandoned before capture.
    Cancelled,
}

impl PaymentState {
    /// The wire name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Captured => "captured",
            Self::Refunded => "refunded",
            Self::Cancelled => "cancelled",
        }
    }

    /// Parse the wire name.
    #[must_use]
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(text: &str) -> Option<Self> {
        Some(match text {
            "pending" => Self::Pending,
            "captured" => Self::Captured,
            "refunded" => Self::Refunded,
            "cancelled" => Self::Cancelled,
            _ => return None,
        })
    }

    /// Whether what was bought is currently the buyer's.
    #[must_use]
    pub fn grants_access(self) -> bool {
        matches!(self, Self::Captured)
    }
}

/// Why a payment transition was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaymentRefusal {
    /// The amount is not a positive number of minor units.
    NotChargeable,
    /// The fee is outside 0–10 000 basis points.
    BadFee(i64),
    /// Nothing has been captured, so there is nothing to refund.
    NotCaptured,
    /// More than remains, naming both numbers.
    OverRefund { requested: i64, remaining: i64 },
    /// A payment already fully refunded.
    AlreadyRefunded,
    /// A state this module does not know.
    UnknownState(String),
}

impl PaymentRefusal {
    /// The sentence a caller may publish.
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Self::NotChargeable => "a price is a positive amount".to_owned(),
            Self::BadFee(fee) => format!("a platform fee of {fee} basis points is outside 0–10 000"),
            Self::NotCaptured => "nothing has been captured, so there is nothing to refund".to_owned(),
            Self::OverRefund { requested, remaining } => {
                format!("a refund of {requested} exceeds the {remaining} still available")
            }
            Self::AlreadyRefunded => "this payment has already been refunded in full".to_owned(),
            Self::UnknownState(name) => format!("`{name}` is not a payment state"),
        }
    }
}

/// How much of a payment may still be returned.
#[must_use]
pub fn refundable(captured_minor: i64, already_refunded_minor: i64) -> i64 {
    (captured_minor - already_refunded_minor).max(0)
}

/// The next refund total after returning `amount_minor`, or why that is refused.
///
/// Returns the **cumulative** refunded amount, because that is what the payment row stores and comparing it with the captured amount is the
/// one check that cannot drift.
pub fn apply_refund(
    captured_minor: i64,
    already_refunded_minor: i64,
    amount_minor: i64,
    state: PaymentState,
) -> Result<i64, PaymentRefusal> {
    if state == PaymentState::Refunded {
        return Err(PaymentRefusal::AlreadyRefunded);
    }
    if !state.grants_access() {
        return Err(PaymentRefusal::NotCaptured);
    }
    if amount_minor <= 0 {
        return Err(PaymentRefusal::NotChargeable);
    }
    let remaining = refundable(captured_minor, already_refunded_minor);
    if amount_minor > remaining {
        return Err(PaymentRefusal::OverRefund {
            requested: amount_minor,
            remaining,
        });
    }
    Ok(already_refunded_minor + amount_minor)
}

/// Whether a refund of `amount_minor` against `captured_minor` returns the money in full.
///
/// Full is what decides whether access is revoked: a partial refund is a gesture about the price, a full one is the sale not having
/// happened.
#[must_use]
pub fn is_full_refund(captured_minor: i64, cumulative_refunded_minor: i64) -> bool {
    captured_minor > 0 && cumulative_refunded_minor >= captured_minor
}

/// What a processor did when asked to take money.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Receipt {
    /// The processor's own reference, for reconciliation.
    pub reference: String,
    /// Whether the money was actually taken.
    pub captured: bool,
}

/// A payment processor, as this host can actually run one.
///
/// `Manual` is not a placeholder: without a provider, an operator captures the charge by hand after taking the money by whatever means the
/// host already has (an invoice, a bank transfer). That is the only honest behaviour, and it makes `Pending` a state readers see rather than
/// a state the code pretends does not exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Processor {
    /// No provider: an operator captures.
    #[default]
    Manual,
}

impl Processor {
    /// The wire name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
        }
    }

    /// Ask the processor to take the money.
    ///
    /// A manual processor never captures on its own: it returns a receipt naming that it is waiting. A future provider-backed processor
    /// returns `captured: true` and the rest of this module does not change — which is the point of the port existing now.
    #[must_use]
    pub fn capture(self, payment_id: &str, amount_minor: i64) -> Receipt {
        match self {
            Self::Manual => Receipt {
                reference: format!("manual:{payment_id}:{amount_minor}"),
                captured: false,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_halves_always_add_back_to_the_gross() {
        let split = Split::reference(1_000).expect("split");
        assert_eq!(split.host, 150, "15% of 1000");
        assert_eq!(split.developer, 850);
        assert_eq!(split.total(), 1_000);

        // The remainder case: 15% of 333 is 49.95, so the host takes 49 and the developer keeps the minor unit.
        let odd = Split::reference(333).expect("split");
        assert_eq!(odd.host, 49);
        assert_eq!(odd.developer, 284);
        assert_eq!(odd.total(), 333);
    }

    #[test]
    fn a_fee_above_the_whole_sale_is_refused_rather_than_normalised() {
        assert_eq!(Split::of(1_000, 10_001), None);
        assert_eq!(Split::of(1_000, -1), None);
        assert_eq!(Split::of(-5, 1_500), None);
        // And a 100% fee is legal arithmetic, even if nobody would choose it.
        let all = Split::of(1_000, 10_000).expect("split");
        assert_eq!((all.host, all.developer), (1_000, 0));
    }

    #[test]
    fn a_refund_cannot_exceed_what_was_captured() {
        let state = PaymentState::Captured;
        assert_eq!(apply_refund(1_000, 0, 1_000, state), Ok(1_000));
        assert_eq!(
            apply_refund(1_000, 0, 1_001, state),
            Err(PaymentRefusal::OverRefund {
                requested: 1_001,
                remaining: 1_000
            })
        );
        // The cumulative check is the one that catches a second partial refund past the total.
        assert_eq!(
            apply_refund(1_000, 800, 300, state),
            Err(PaymentRefusal::OverRefund {
                requested: 300,
                remaining: 200
            })
        );
    }

    #[test]
    fn nothing_pending_or_cancelled_can_be_refunded() {
        assert_eq!(
            apply_refund(1_000, 0, 100, PaymentState::Pending),
            Err(PaymentRefusal::NotCaptured)
        );
        assert_eq!(
            apply_refund(1_000, 0, 100, PaymentState::Cancelled),
            Err(PaymentRefusal::NotCaptured)
        );
        assert_eq!(
            apply_refund(1_000, 1_000, 100, PaymentState::Refunded),
            Err(PaymentRefusal::AlreadyRefunded)
        );
    }

    #[test]
    fn only_a_full_refund_is_a_full_refund() {
        assert!(!is_full_refund(1_000, 999));
        assert!(is_full_refund(1_000, 1_000));
        assert!(!is_full_refund(0, 0), "nothing captured is not a full refund");
    }

    #[test]
    fn tax_is_inclusive_and_the_invoice_totals_reconcile() {
        let profile = TaxProfile {
            legal_name: "Archive Ltd".to_owned(),
            country: "DE".to_owned(),
            tax_id: Some("DE123456789".to_owned()),
            rate_bp: 1_900,
            invoice_prefix: "DH".to_owned(),
        };
        assert!(profile.validate().is_ok());
        // 19% inclusive of 1 000 is 159.66 -> 160, net 840.
        assert_eq!(profile.tax_on(1_000), 160);
        assert_eq!(profile.net_of(1_000), 840);
        assert_eq!(profile.net_of(1_000) + profile.tax_on(1_000), 1_000);
        // Zero-rated jurisdictions and zero prices are not errors.
        assert_eq!(profile.tax_on(0), 0);
        let zero = TaxProfile {
            rate_bp: 0,
            ..profile
        };
        assert_eq!(zero.tax_on(1_000), 0);
    }

    #[test]
    fn a_nonsense_tax_profile_is_refused_with_the_reason() {
        let base = TaxProfile {
            legal_name: "Archive Ltd".to_owned(),
            country: "DE".to_owned(),
            tax_id: None,
            rate_bp: 1_900,
            invoice_prefix: "DH".to_owned(),
        };
        assert!(TaxProfile {
            rate_bp: 20_000,
            ..base.clone()
        }
        .validate()
        .is_err());
        assert!(TaxProfile {
            country: "Germany".to_owned(),
            ..base.clone()
        }
        .validate()
        .is_err());
        assert!(TaxProfile {
            legal_name: "  ".to_owned(),
            ..base.clone()
        }
        .validate()
        .is_err());
        assert!(TaxProfile {
            invoice_prefix: String::new(),
            ..base
        }
        .validate()
        .is_err());
    }

    #[test]
    fn the_manual_processor_records_a_charge_and_never_pretends_to_have_taken_one() {
        let receipt = Processor::Manual.capture("pay-1", 1_000);
        assert!(!receipt.captured);
        assert!(receipt.reference.starts_with("manual:pay-1:"));
    }

    #[test]
    fn only_a_captured_payment_grants_access() {
        assert!(PaymentState::Captured.grants_access());
        assert!(!PaymentState::Pending.grants_access());
        assert!(!PaymentState::Refunded.grants_access());
        assert!(!PaymentState::Cancelled.grants_access());
        for state in [
            PaymentState::Pending,
            PaymentState::Captured,
            PaymentState::Refunded,
            PaymentState::Cancelled,
        ] {
            assert_eq!(PaymentState::from_str(state.as_str()), Some(state));
        }
    }
}

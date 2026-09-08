//! Defines what the user wants, and how much they care.

use std::time::Instant;

use bitcoin::{Amount, ScriptBuf};

/// The required information for an output creation intent.
///
/// Corresponds to just the on chain variant of `bitcoin-payment-instructions`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FixedPaymentInstructions {
    /// Output script the payment must pay to.
    pub script_pubkey: ScriptBuf,
    /// Exact amount the payment must carry.
    pub amount: Amount,
}

/// Names an intent in a [`Queue`](crate::queue::Queue).
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct IntentId(pub usize);

/// Something the user wants to happen, which can be realized in potentially more than
/// one way.
///
/// Still missing an on-chain variant: some change to the UTXO set the user wants
/// realized, carried as an unordered PSBT that may be imbalanced. That variant is the
/// only thing here that needs concurrent-psbt, so it waits until this crate takes on
/// that dependency.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Intent {
    /// Pay someone under instructions they supplied, out of band.
    OutputCreation(FixedPaymentInstructions),
}

/// Why a set of breakpoints does not describe a usable payoff curve.
#[derive(Debug, PartialEq, Eq)]
pub enum PayoffCurveError {
    /// Fewer than two breakpoints
    TooShort,
    /// Breakpoints are not strictly increasing in time.
    OutOfOrder,
    /// The first breakpoint is worth something, leaving the start open.
    OpenStart,
    /// The last breakpoint is neither worthless nor the maximum, leaving the end open.
    OpenEnd,
}

/// What realizing an intent is worth over time, as breakpoints on a piecewise linear
/// curve. Utility rises towards the deadline.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PayoffCurve(Vec<(Instant, Amount)>);

impl PayoffCurve {
    /// Take `breakpoints` if they pin down both ends of a curve.
    pub fn new(breakpoints: Vec<(Instant, Amount)>) -> Result<Self, PayoffCurveError> {
        if breakpoints.len() < 2 {
            return Err(PayoffCurveError::TooShort);
        }

        if breakpoints.windows(2).any(|pair| pair[1].0 <= pair[0].0) {
            return Err(PayoffCurveError::OutOfOrder);
        }

        let worth = |point: Option<&(Instant, Amount)>| point.expect("checked length").1;
        let peak = breakpoints
            .iter()
            .map(|&(_, amount)| amount)
            .max()
            .expect("checked length");

        if worth(breakpoints.first()) != Amount::ZERO {
            return Err(PayoffCurveError::OpenStart);
        }

        let last = worth(breakpoints.last());
        if last != Amount::ZERO && last != peak {
            return Err(PayoffCurveError::OpenEnd);
        }

        Ok(PayoffCurve(breakpoints))
    }

    /// The breakpoints, in time order.
    pub fn breakpoints(&self) -> &[(Instant, Amount)] {
        &self.0
    }
}

/// An [`Intent`] together with a [`PayoffCurve`] determined by the user.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntentWithPayoffCurve {
    pub(crate) inner: Intent,
    pub(crate) payoff_curve: PayoffCurve,
}

impl IntentWithPayoffCurve {
    /// Build an intent with the payoff curve the user attached to it.
    pub fn new(inner: Intent, payoff_curve: PayoffCurve) -> Self {
        IntentWithPayoffCurve {
            inner,
            payoff_curve,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn request() -> FixedPaymentInstructions {
        FixedPaymentInstructions {
            script_pubkey: ScriptBuf::from_bytes(vec![0x51]),
            amount: Amount::from_sat(50_000),
        }
    }

    /// A curve rising to a peak and falling back to nothing: a payment the user gives up
    /// on once the deadline is past.
    fn single_peaked(start: Instant) -> PayoffCurve {
        PayoffCurve::new(vec![
            (start, Amount::ZERO),
            (start + Duration::from_secs(1800), Amount::from_sat(100_000)),
            (start + Duration::from_secs(3600), Amount::ZERO),
        ])
        .expect("well formed")
    }

    #[test]
    fn payment_instructions_carry_a_script_and_an_amount() {
        let instructions = request();

        assert_eq!(instructions.amount, Amount::from_sat(50_000));
        assert_eq!(instructions.clone(), instructions);
    }

    /// One point fixes a value but no slope, so there is no curve to read.
    #[test]
    fn a_curve_needs_at_least_two_breakpoints() {
        let start = Instant::now();

        assert_eq!(PayoffCurve::new(vec![]), Err(PayoffCurveError::TooShort));
        assert_eq!(
            PayoffCurve::new(vec![(start, Amount::ZERO)]),
            Err(PayoffCurveError::TooShort)
        );
    }

    #[test]
    fn breakpoints_must_strictly_advance_in_time() {
        let start = Instant::now();
        let later = start + Duration::from_secs(60);

        assert_eq!(
            PayoffCurve::new(vec![(later, Amount::ZERO), (start, Amount::ZERO)]),
            Err(PayoffCurveError::OutOfOrder)
        );
        assert_eq!(
            PayoffCurve::new(vec![(start, Amount::ZERO), (start, Amount::ZERO)]),
            Err(PayoffCurveError::OutOfOrder)
        );
    }

    /// Leaving either end worth something undeclared would force the reader to guess
    /// whether the boundary segment continues or the boundary value holds.
    #[test]
    fn both_ends_of_a_curve_must_be_pinned_down() {
        let start = Instant::now();
        let later = start + Duration::from_secs(60);

        assert_eq!(
            PayoffCurve::new(vec![(start, Amount::from_sat(1)), (later, Amount::ZERO)]),
            Err(PayoffCurveError::OpenStart)
        );

        assert_eq!(
            PayoffCurve::new(vec![
                (start, Amount::ZERO),
                (start + Duration::from_secs(30), Amount::from_sat(100)),
                (later, Amount::from_sat(40)),
            ]),
            Err(PayoffCurveError::OpenEnd)
        );
    }

    /// A critical payment plateaus at the most the user will spend and holds it, so its
    /// last breakpoint is the peak rather than nothing.
    #[test]
    fn a_curve_may_end_at_its_peak_instead_of_at_nothing() {
        let start = Instant::now();
        let peak = start + Duration::from_secs(60);

        let curve = PayoffCurve::new(vec![(start, Amount::ZERO), (peak, Amount::from_sat(100))])
            .expect("plateau is a closed end");

        assert_eq!(
            curve.breakpoints().last(),
            Some(&(peak, Amount::from_sat(100)))
        );
    }

    #[test]
    fn a_curve_keeps_its_breakpoints_in_time_order() {
        let start = Instant::now();
        let curve = single_peaked(start);

        assert_eq!(curve.breakpoints().len(), 3);
        assert_eq!(curve.breakpoints()[0], (start, Amount::ZERO));
        assert_eq!(
            curve.breakpoints()[1],
            (start + Duration::from_secs(1800), Amount::from_sat(100_000))
        );
    }

    /// Ids name intents, so they compare, copy and hash like the handles they are.
    #[test]
    fn intent_ids_name_intents() {
        use std::collections::HashSet;

        let id = IntentId(2);
        let copied = id;

        assert_eq!(copied, IntentId(2));
        assert_ne!(copied, IntentId(3));
        assert_eq!(format!("{id:?}"), "IntentId(2)");
        assert_eq!(
            HashSet::from([id, IntentId(2), IntentId(3)]),
            HashSet::from([IntentId(2), IntentId(3)])
        );
    }
}

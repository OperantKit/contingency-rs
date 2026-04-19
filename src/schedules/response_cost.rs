//! Response-cost schedule wrapper — Weiner (1962); Hackenberg (2009).
//!
//! Each target response debits a fixed amount from an accumulated
//! balance. When `initial_balance` is `None`, the balance is unlimited
//! and cost is purely informational. When a finite balance is set, a
//! response that cannot be afforded is silently dropped (inner
//! schedule is not stepped) and the outcome carries a
//! `cost_insufficient` meta flag.
//!
//! # References
//!
//! Hackenberg, T. D. (2009). Token reinforcement: A review and
//! analysis. *Journal of the Experimental Analysis of Behavior*,
//! 91(2), 257-286. <https://doi.org/10.1901/jeab.2009.91-257>
//!
//! Weiner, H. (1962). Some effects of response cost upon human operant
//! behavior. *Journal of the Experimental Analysis of Behavior*, 5(2),
//! 201-208. <https://doi.org/10.1901/jeab.1962.5-201>

use crate::errors::ContingencyError;
use crate::helpers::checks::{check_event, check_time};
use crate::schedule::Schedule;
use crate::types::{MetaValue, Outcome, ResponseEvent};
use crate::Result;

/// Response-cost wrapper.
///
/// See module docs.
pub struct ResponseCost {
    inner: Box<dyn Schedule>,
    amount: f64,
    unit: String,
    initial_balance: Option<f64>,
    balance: Option<f64>,
    last_now: Option<f64>,
}

impl std::fmt::Debug for ResponseCost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResponseCost")
            .field("amount", &self.amount)
            .field("unit", &self.unit)
            .field("initial_balance", &self.initial_balance)
            .field("balance", &self.balance)
            .field("last_now", &self.last_now)
            .finish()
    }
}

impl ResponseCost {
    /// Construct a ResponseCost wrapper.
    ///
    /// # Errors
    ///
    /// Returns [`ContingencyError::Config`] if `amount <= 0` or
    /// `initial_balance < 0`.
    pub fn new(
        inner: Box<dyn Schedule>,
        amount: f64,
        unit: String,
        initial_balance: Option<f64>,
    ) -> Result<Self> {
        if !amount.is_finite() || amount <= 0.0 {
            return Err(ContingencyError::Config(format!(
                "ResponseCost requires amount > 0, got {amount}"
            )));
        }
        if let Some(b) = initial_balance {
            if !b.is_finite() || b < 0.0 {
                return Err(ContingencyError::Config(format!(
                    "ResponseCost requires initial_balance >= 0, got {b}"
                )));
            }
        }
        Ok(Self {
            inner,
            amount,
            unit,
            initial_balance,
            balance: initial_balance,
            last_now: None,
        })
    }

    /// The per-response cost amount.
    pub fn amount(&self) -> f64 {
        self.amount
    }

    /// The resource unit label.
    pub fn unit(&self) -> &str {
        &self.unit
    }

    /// The current token balance (`None` = unlimited).
    pub fn balance(&self) -> Option<f64> {
        self.balance
    }
}

impl Schedule for ResponseCost {
    fn step(&mut self, now: f64, event: Option<&ResponseEvent>) -> Result<Outcome> {
        check_time(now, self.last_now)?;
        check_event(now, event)?;
        self.last_now = Some(now);

        if event.is_none() {
            // Pure tick: propagate to inner.
            return self.inner.step(now, None);
        }

        // Event path: check balance before stepping inner.
        if let Some(bal) = self.balance {
            if bal < self.amount {
                let mut out = Outcome::empty();
                out.meta
                    .insert("cost_insufficient".to_string(), MetaValue::Bool(true));
                out.meta
                    .insert("unit".to_string(), MetaValue::Str(self.unit.clone()));
                return Ok(out);
            }
            self.balance = Some(bal - self.amount);
        }

        self.inner.step(now, event)
    }

    fn reset(&mut self) {
        self.inner.reset();
        self.balance = self.initial_balance;
        self.last_now = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schedules::FR;

    fn respond<S: Schedule>(s: &mut S, now: f64) -> Outcome {
        let ev = ResponseEvent::new(now);
        s.step(now, Some(&ev)).expect("step should succeed")
    }

    #[test]
    fn construct_rejects_non_positive_amount() {
        let inner = Box::new(FR::new(1).unwrap());
        assert!(matches!(
            ResponseCost::new(inner, 0.0, "tok".into(), None),
            Err(ContingencyError::Config(_))
        ));
        let inner = Box::new(FR::new(1).unwrap());
        assert!(matches!(
            ResponseCost::new(inner, -1.0, "tok".into(), None),
            Err(ContingencyError::Config(_))
        ));
    }

    #[test]
    fn construct_rejects_negative_initial_balance() {
        let inner = Box::new(FR::new(1).unwrap());
        assert!(matches!(
            ResponseCost::new(inner, 1.0, "tok".into(), Some(-1.0)),
            Err(ContingencyError::Config(_))
        ));
    }

    #[test]
    fn unlimited_balance_passes_through() {
        let mut s =
            ResponseCost::new(Box::new(FR::new(1).unwrap()), 5.0, "tok".into(), None).unwrap();
        for t in 1..=3 {
            let o = respond(&mut s, t as f64);
            assert!(o.reinforced);
        }
        assert!(s.balance().is_none());
    }

    #[test]
    fn finite_balance_decrements_per_response() {
        let mut s =
            ResponseCost::new(Box::new(FR::new(1).unwrap()), 2.0, "tok".into(), Some(5.0))
                .unwrap();
        let o = respond(&mut s, 1.0);
        assert!(o.reinforced);
        assert_eq!(s.balance(), Some(3.0));
        let o = respond(&mut s, 2.0);
        assert!(o.reinforced);
        assert_eq!(s.balance(), Some(1.0));
    }

    #[test]
    fn insufficient_balance_drops_response() {
        let mut s =
            ResponseCost::new(Box::new(FR::new(1).unwrap()), 3.0, "tok".into(), Some(5.0))
                .unwrap();
        // First response consumes 3 → balance 2 → reinforced.
        let o = respond(&mut s, 1.0);
        assert!(o.reinforced);
        assert_eq!(s.balance(), Some(2.0));
        // Second response cannot afford 3 → dropped, inner not stepped.
        let o = respond(&mut s, 2.0);
        assert!(!o.reinforced);
        assert_eq!(o.meta.get("cost_insufficient"), Some(&MetaValue::Bool(true)));
        assert_eq!(s.balance(), Some(2.0));
    }

    #[test]
    fn tick_without_event_forwards_to_inner() {
        let mut s =
            ResponseCost::new(Box::new(FR::new(1).unwrap()), 1.0, "tok".into(), Some(0.0))
                .unwrap();
        // Even with 0 balance, a pure tick should pass through.
        let o = s.step(1.0, None).unwrap();
        assert!(!o.reinforced);
        assert_eq!(s.balance(), Some(0.0));
    }

    #[test]
    fn reset_restores_initial_balance() {
        let mut s =
            ResponseCost::new(Box::new(FR::new(1).unwrap()), 1.0, "tok".into(), Some(3.0))
                .unwrap();
        respond(&mut s, 1.0);
        respond(&mut s, 2.0);
        assert_eq!(s.balance(), Some(1.0));
        s.reset();
        assert_eq!(s.balance(), Some(3.0));
    }

    #[test]
    fn rejects_non_monotonic_time() {
        let mut s =
            ResponseCost::new(Box::new(FR::new(1).unwrap()), 1.0, "tok".into(), None).unwrap();
        respond(&mut s, 5.0);
        let ev = ResponseEvent::new(4.0);
        assert!(matches!(
            s.step(4.0, Some(&ev)),
            Err(ContingencyError::State(_))
        ));
    }
}

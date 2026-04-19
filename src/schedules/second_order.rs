//! Second-order schedule — Kelleher (1966).
//!
//! A second-order schedule nests an inner "unit" schedule inside an
//! outer "overall" schedule. Completions of the unit count as single
//! responses toward the overall schedule's requirement; only the
//! overall's reinforcement is subject-visible.
//!
//! Semantics (direct port from the Python reference):
//!
//! 1. Every `step` delegates to the unit schedule.
//! 2. If the unit reinforces, that reinforcer is suppressed, the unit
//!    is reset, and a synthesised `ResponseEvent` tagged
//!    `operandum = "__unit_completion__"` is fed to the overall at
//!    the same `now`.
//! 3. If the unit does *not* reinforce, the overall is still stepped
//!    with `event = None` so time-based overalls accumulate elapsed
//!    time correctly.
//! 4. When the overall reinforces on the synthetic event, the
//!    returned outcome carries `meta["second_order"] = true`.
//!
//! # References
//!
//! Kelleher, R. T. (1966). Chaining and conditioned reinforcement. In
//! W. K. Honig (Ed.), *Operant behavior: Areas of research and
//! application* (pp. 160-212). Appleton-Century-Crofts.

use crate::helpers::checks::{check_event, check_time};
use crate::schedule::Schedule;
use crate::types::{MetaValue, Outcome, ResponseEvent};
use crate::Result;

/// Operandum tag for the internally-synthesised unit-completion event.
pub const UNIT_COMPLETION_OPERANDUM: &str = "__unit_completion__";

/// Nested second-order schedule `overall(unit)`.
pub struct SecondOrder {
    overall: Box<dyn Schedule>,
    unit: Box<dyn Schedule>,
    last_now: Option<f64>,
    unit_completions: u64,
}

impl std::fmt::Debug for SecondOrder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecondOrder")
            .field("last_now", &self.last_now)
            .field("unit_completions", &self.unit_completions)
            .finish()
    }
}

impl SecondOrder {
    /// Construct a second-order schedule.
    pub fn new(overall: Box<dyn Schedule>, unit: Box<dyn Schedule>) -> Self {
        Self {
            overall,
            unit,
            last_now: None,
            unit_completions: 0,
        }
    }

    /// Cumulative unit completions since construction or last reset.
    pub fn unit_completions(&self) -> u64 {
        self.unit_completions
    }
}

impl Schedule for SecondOrder {
    fn step(&mut self, now: f64, event: Option<&ResponseEvent>) -> Result<Outcome> {
        check_time(now, self.last_now)?;
        check_event(now, event)?;
        self.last_now = Some(now);

        let unit_outcome = self.unit.step(now, event)?;

        let synth_event: Option<ResponseEvent> = if unit_outcome.reinforced {
            self.unit.reset();
            self.unit_completions += 1;
            Some(ResponseEvent::on(UNIT_COMPLETION_OPERANDUM, now))
        } else {
            None
        };

        let overall_outcome = self.overall.step(now, synth_event.as_ref())?;

        if overall_outcome.reinforced {
            let mut meta = overall_outcome.meta;
            meta.insert("second_order".to_string(), MetaValue::Bool(true));
            return Ok(Outcome {
                reinforced: true,
                reinforcer: overall_outcome.reinforcer,
                meta,
            });
        }

        if unit_outcome.reinforced {
            let mut out = Outcome::empty();
            out.meta
                .insert("unit_completed".to_string(), MetaValue::Bool(true));
            return Ok(out);
        }
        Ok(Outcome::empty())
    }

    fn reset(&mut self) {
        self.overall.reset();
        self.unit.reset();
        self.last_now = None;
        self.unit_completions = 0;
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
    fn fr_of_fr_counts_unit_completions() {
        // FR3(FR2): 2 responses → unit completes (swallowed, counts as 1 toward overall).
        // 6 responses total → 3 unit completions → overall fires.
        let overall = Box::new(FR::new(3).unwrap());
        let unit = Box::new(FR::new(2).unwrap());
        let mut s = SecondOrder::new(overall, unit);

        let mut reinforced_at: Option<f64> = None;
        for i in 1..=6 {
            let t = i as f64;
            let o = respond(&mut s, t);
            if o.reinforced {
                reinforced_at = Some(t);
                assert_eq!(o.meta.get("second_order"), Some(&MetaValue::Bool(true)));
            } else if i % 2 == 0 {
                // Every 2nd response is a unit completion.
                assert_eq!(
                    o.meta.get("unit_completed"),
                    Some(&MetaValue::Bool(true))
                );
            }
        }
        assert_eq!(reinforced_at, Some(6.0));
        assert_eq!(s.unit_completions(), 3);
    }

    #[test]
    fn unit_reinforcer_is_suppressed() {
        let overall = Box::new(FR::new(5).unwrap());
        let unit = Box::new(FR::new(1).unwrap());
        let mut s = SecondOrder::new(overall, unit);
        // First 4 unit completions: overall not yet satisfied; outcomes
        // should not carry a subject-visible reinforcer.
        for i in 1..=4 {
            let o = respond(&mut s, i as f64);
            assert!(!o.reinforced);
            assert!(o.reinforcer.is_none());
            assert_eq!(
                o.meta.get("unit_completed"),
                Some(&MetaValue::Bool(true))
            );
        }
        // 5th unit completion → overall reinforces.
        let o = respond(&mut s, 5.0);
        assert!(o.reinforced);
        assert_eq!(o.meta.get("second_order"), Some(&MetaValue::Bool(true)));
    }

    #[test]
    fn reset_clears_completions() {
        let overall = Box::new(FR::new(3).unwrap());
        let unit = Box::new(FR::new(1).unwrap());
        let mut s = SecondOrder::new(overall, unit);
        respond(&mut s, 1.0);
        respond(&mut s, 2.0);
        assert_eq!(s.unit_completions(), 2);
        s.reset();
        assert_eq!(s.unit_completions(), 0);
    }

    #[test]
    fn rejects_non_monotonic_time() {
        let overall = Box::new(FR::new(3).unwrap());
        let unit = Box::new(FR::new(1).unwrap());
        let mut s = SecondOrder::new(overall, unit);
        respond(&mut s, 5.0);
        let ev = ResponseEvent::new(4.0);
        assert!(matches!(
            s.step(4.0, Some(&ev)),
            Err(crate::errors::ContingencyError::State(_))
        ));
    }
}

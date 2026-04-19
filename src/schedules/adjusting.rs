//! Adjusting schedules — Mazur (1987).
//!
//! A single numeric parameter (ratio, interval, delay, or amount)
//! shifts by a fixed `step` after every earned reinforcer, clamped
//! into `[minimum, maximum]`. The runtime realises only the `Ratio`
//! and `Interval` targets; `Delay` and `Amount` construct
//! successfully (for DSL round-trip) but raise
//! [`ContingencyError::Config`] on `step()`.
//!
//! # References
//!
//! Mazur, J. E. (1987). An adjusting procedure for studying delayed
//! reinforcement. In M. L. Commons, J. E. Mazur, J. A. Nevin, & H.
//! Rachlin (Eds.), *Quantitative analyses of behavior, Vol. 5: The
//! effect of delay and of intervening events on reinforcement value*
//! (pp. 55-73). Lawrence Erlbaum Associates.

use crate::constants::TIME_TOL;
use crate::errors::ContingencyError;
use crate::helpers::checks::{check_event, check_time};
use crate::schedule::Schedule;
use crate::types::{Outcome, Reinforcer, ResponseEvent};
use crate::Result;

/// Which parameter the adjusting schedule mutates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdjustingTarget {
    /// Ratio requirement (FR-like).
    Ratio,
    /// Interval length (FI-like).
    Interval,
    /// Reinforcement delay (DSL round-trip; not executable).
    Delay,
    /// Reinforcer magnitude (DSL round-trip; not executable).
    Amount,
}

/// Adjusting schedule with a single numeric parameter under update.
#[derive(Debug)]
pub struct AdjustingSchedule {
    target: AdjustingTarget,
    start: f64,
    step_size: f64,
    min: Option<f64>,
    max: Option<f64>,
    current: f64,
    count: u64,
    anchor: Option<f64>,
    last_now: Option<f64>,
}

impl AdjustingSchedule {
    /// Construct an adjusting schedule.
    ///
    /// # Errors
    ///
    /// - `start <= 0`
    /// - `target == Ratio` and `round(start) < 1`
    /// - `minimum > maximum`
    /// - non-finite parameters
    pub fn new(
        target: AdjustingTarget,
        start: f64,
        step: f64,
        minimum: Option<f64>,
        maximum: Option<f64>,
    ) -> Result<Self> {
        if !start.is_finite() {
            return Err(ContingencyError::Config(format!(
                "AdjustingSchedule requires finite start, got {start}"
            )));
        }
        if !step.is_finite() {
            return Err(ContingencyError::Config(format!(
                "AdjustingSchedule requires finite step, got {step}"
            )));
        }
        if let Some(m) = minimum {
            if !m.is_finite() {
                return Err(ContingencyError::Config(format!(
                    "AdjustingSchedule minimum must be finite, got {m}"
                )));
            }
        }
        if let Some(m) = maximum {
            if !m.is_finite() {
                return Err(ContingencyError::Config(format!(
                    "AdjustingSchedule maximum must be finite, got {m}"
                )));
            }
        }
        if let (Some(lo), Some(hi)) = (minimum, maximum) {
            if lo > hi {
                return Err(ContingencyError::Config(format!(
                    "AdjustingSchedule requires minimum <= maximum, got minimum={lo}, maximum={hi}"
                )));
            }
        }
        if start <= 0.0 {
            return Err(ContingencyError::Config(format!(
                "AdjustingSchedule start must be > 0, got {start}"
            )));
        }
        if matches!(target, AdjustingTarget::Ratio) && start.round() < 1.0 {
            return Err(ContingencyError::Config(format!(
                "AdjustingSchedule ratio start must round to >= 1, got {start}"
            )));
        }

        let current = clamp_value(start, minimum, maximum);
        Ok(Self {
            target,
            start,
            step_size: step,
            min: minimum,
            max: maximum,
            current,
            count: 0,
            anchor: None,
            last_now: None,
        })
    }

    /// The adjusted parameter kind.
    pub fn target(&self) -> AdjustingTarget {
        self.target
    }

    /// The initial (pre-clamp) value.
    pub fn start(&self) -> f64 {
        self.start
    }

    /// The increment applied after each reinforcement.
    pub fn step_size(&self) -> f64 {
        self.step_size
    }

    /// The lower clamp, if any.
    pub fn minimum(&self) -> Option<f64> {
        self.min
    }

    /// The upper clamp, if any.
    pub fn maximum(&self) -> Option<f64> {
        self.max
    }

    /// The current (clamped) value of the adjusted parameter.
    pub fn current_value(&self) -> f64 {
        self.current
    }

    fn advance(&mut self) {
        self.current = clamp_value(self.current + self.step_size, self.min, self.max);
    }

    fn step_ratio(&mut self, now: f64, event: Option<&ResponseEvent>) -> Result<Outcome> {
        if event.is_none() {
            return Ok(Outcome::empty());
        }
        let requirement = (self.current.round() as i64).max(1) as u64;
        self.count += 1;
        if self.count >= requirement {
            self.count = 0;
            self.advance();
            return Ok(Outcome::reinforced(Reinforcer::at(now)));
        }
        Ok(Outcome::empty())
    }

    fn step_interval(&mut self, now: f64, event: Option<&ResponseEvent>) -> Result<Outcome> {
        if self.anchor.is_none() {
            self.anchor = Some(now);
        }
        if event.is_none() {
            return Ok(Outcome::empty());
        }
        let anchor = self.anchor.expect("anchored above");
        let elapsed = now - anchor;
        if elapsed + TIME_TOL >= self.current {
            self.anchor = Some(now);
            self.advance();
            return Ok(Outcome::reinforced(Reinforcer::at(now)));
        }
        Ok(Outcome::empty())
    }
}

fn clamp_value(v: f64, min: Option<f64>, max: Option<f64>) -> f64 {
    let mut out = v;
    if let Some(lo) = min {
        if out < lo {
            out = lo;
        }
    }
    if let Some(hi) = max {
        if out > hi {
            out = hi;
        }
    }
    out
}

impl Schedule for AdjustingSchedule {
    fn step(&mut self, now: f64, event: Option<&ResponseEvent>) -> Result<Outcome> {
        check_time(now, self.last_now)?;
        check_event(now, event)?;
        self.last_now = Some(now);

        match self.target {
            AdjustingTarget::Ratio => self.step_ratio(now, event),
            AdjustingTarget::Interval => self.step_interval(now, event),
            AdjustingTarget::Delay | AdjustingTarget::Amount => {
                Err(ContingencyError::Config(format!(
                    "target={:?} runtime not yet implemented",
                    self.target
                )))
            }
        }
    }

    fn reset(&mut self) {
        self.current = clamp_value(self.start, self.min, self.max);
        self.count = 0;
        self.anchor = None;
        self.last_now = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn respond<S: Schedule>(s: &mut S, now: f64) -> Outcome {
        let ev = ResponseEvent::new(now);
        s.step(now, Some(&ev)).expect("step should succeed")
    }

    #[test]
    fn construct_rejects_non_positive_start() {
        assert!(matches!(
            AdjustingSchedule::new(AdjustingTarget::Ratio, 0.0, 1.0, None, None),
            Err(ContingencyError::Config(_))
        ));
    }

    #[test]
    fn construct_rejects_ratio_below_one() {
        assert!(matches!(
            AdjustingSchedule::new(AdjustingTarget::Ratio, 0.4, 1.0, None, None),
            Err(ContingencyError::Config(_))
        ));
    }

    #[test]
    fn construct_rejects_min_gt_max() {
        assert!(matches!(
            AdjustingSchedule::new(AdjustingTarget::Ratio, 5.0, 1.0, Some(10.0), Some(3.0)),
            Err(ContingencyError::Config(_))
        ));
    }

    #[test]
    fn ratio_increments_after_each_reinforcer() {
        let mut s =
            AdjustingSchedule::new(AdjustingTarget::Ratio, 2.0, 1.0, None, None).unwrap();
        // Requirement=2: second response reinforces, current -> 3.
        assert!(!respond(&mut s, 1.0).reinforced);
        let o = respond(&mut s, 2.0);
        assert!(o.reinforced);
        assert_eq!(s.current_value(), 3.0);
        // Requirement=3: third subsequent response reinforces.
        assert!(!respond(&mut s, 3.0).reinforced);
        assert!(!respond(&mut s, 4.0).reinforced);
        let o = respond(&mut s, 5.0);
        assert!(o.reinforced);
        assert_eq!(s.current_value(), 4.0);
    }

    #[test]
    fn ratio_clamps_to_maximum() {
        let mut s =
            AdjustingSchedule::new(AdjustingTarget::Ratio, 2.0, 10.0, None, Some(3.0)).unwrap();
        assert!(!respond(&mut s, 1.0).reinforced);
        let o = respond(&mut s, 2.0);
        assert!(o.reinforced);
        assert_eq!(s.current_value(), 3.0);
    }

    #[test]
    fn interval_anchors_on_first_step() {
        let mut s =
            AdjustingSchedule::new(AdjustingTarget::Interval, 5.0, 1.0, None, None).unwrap();
        // First step at t=10 anchors the schedule.
        assert!(!respond(&mut s, 10.0).reinforced);
        // 5 s later: reinforce, current -> 6.
        let o = respond(&mut s, 15.0);
        assert!(o.reinforced);
        assert_eq!(s.current_value(), 6.0);
    }

    #[test]
    fn delay_and_amount_error_on_step() {
        let mut d =
            AdjustingSchedule::new(AdjustingTarget::Delay, 1.0, 1.0, None, None).unwrap();
        assert!(matches!(
            d.step(0.0, None),
            Err(ContingencyError::Config(_))
        ));
        let mut a =
            AdjustingSchedule::new(AdjustingTarget::Amount, 1.0, 1.0, None, None).unwrap();
        assert!(matches!(
            a.step(0.0, None),
            Err(ContingencyError::Config(_))
        ));
    }

    #[test]
    fn reset_restores_current_and_counter() {
        let mut s =
            AdjustingSchedule::new(AdjustingTarget::Ratio, 2.0, 1.0, None, None).unwrap();
        respond(&mut s, 1.0);
        respond(&mut s, 2.0);
        assert_eq!(s.current_value(), 3.0);
        s.reset();
        assert_eq!(s.current_value(), 2.0);
    }

    #[test]
    fn rejects_non_monotonic_time() {
        let mut s =
            AdjustingSchedule::new(AdjustingTarget::Ratio, 2.0, 1.0, None, None).unwrap();
        respond(&mut s, 5.0);
        let ev = ResponseEvent::new(4.0);
        assert!(matches!(
            s.step(4.0, Some(&ev)),
            Err(ContingencyError::State(_))
        ));
    }
}

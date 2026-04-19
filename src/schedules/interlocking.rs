//! Interlocking schedule — Ferster & Skinner (1957).
//!
//! The ratio requirement `R(t) = max(0, R_0 * (1 - t/T))` decays
//! linearly over the time window `T` since the last reinforcer. Any
//! response reinforces once the response count since the last
//! reinforcer reaches the effective requirement.
//!
//! # References
//!
//! Ferster, C. B., & Skinner, B. F. (1957). *Schedules of
//! reinforcement* (pp. 501-502). Appleton-Century-Crofts.
//!
//! Berryman, R., & Nevin, J. A. (1962). Interlocking schedules of
//! reinforcement. *Journal of the Experimental Analysis of Behavior*,
//! 5(2), 213-223. <https://doi.org/10.1901/jeab.1962.5-213>

use crate::errors::ContingencyError;
use crate::helpers::checks::{check_event, check_time};
use crate::schedule::Schedule;
use crate::types::{Outcome, Reinforcer, ResponseEvent};
use crate::Result;

/// Linearly-decaying interlocking ratio/interval schedule.
#[derive(Debug)]
pub struct InterlockingSchedule {
    r0: u64,
    t_window: f64,
    count: u64,
    anchor: Option<f64>,
    last_now: Option<f64>,
}

impl InterlockingSchedule {
    /// Construct an interlocking schedule.
    ///
    /// # Errors
    ///
    /// Returns [`ContingencyError::Config`] if `initial_ratio < 1` or
    /// `decay_time <= 0` / non-finite.
    pub fn new(initial_ratio: u64, decay_time: f64) -> Result<Self> {
        if initial_ratio < 1 {
            return Err(ContingencyError::Config(format!(
                "InterlockingSchedule requires initial_ratio >= 1, got {initial_ratio}"
            )));
        }
        if !decay_time.is_finite() || decay_time <= 0.0 {
            return Err(ContingencyError::Config(format!(
                "InterlockingSchedule requires decay_time > 0, got {decay_time}"
            )));
        }
        Ok(Self {
            r0: initial_ratio,
            t_window: decay_time,
            count: 0,
            anchor: None,
            last_now: None,
        })
    }

    /// The ratio requirement `R_0` at `t = 0` since last reinforcement.
    pub fn initial_ratio(&self) -> u64 {
        self.r0
    }

    /// The time window `T` over which `R` decays to zero.
    pub fn decay_time(&self) -> f64 {
        self.t_window
    }

    /// Responses emitted since the last reinforcement (or anchor).
    pub fn response_count(&self) -> u64 {
        self.count
    }

    /// Effective requirement at `now`.
    ///
    /// If the schedule has not been anchored yet (no prior step), the
    /// reference time is implicitly `now` — so this returns `R_0`.
    pub fn current_requirement(&self, now: f64) -> u64 {
        let anchor = self.anchor.unwrap_or(now);
        let mut elapsed = now - anchor;
        if elapsed < 0.0 {
            elapsed = 0.0;
        }
        let raw = (self.r0 as f64) * (1.0 - elapsed / self.t_window);
        if raw <= 0.0 {
            0
        } else {
            raw as u64
        }
    }

    fn effective_requirement(&self, now: f64) -> u64 {
        let anchor = self.anchor.expect("anchored before call");
        let mut elapsed = now - anchor;
        if elapsed < 0.0 {
            elapsed = 0.0;
        }
        let raw = (self.r0 as f64) * (1.0 - elapsed / self.t_window);
        if raw <= 0.0 {
            0
        } else {
            raw as u64
        }
    }
}

impl Schedule for InterlockingSchedule {
    fn step(&mut self, now: f64, event: Option<&ResponseEvent>) -> Result<Outcome> {
        check_time(now, self.last_now)?;
        check_event(now, event)?;
        self.last_now = Some(now);

        // Anchor on the first tick.
        if self.anchor.is_none() {
            self.anchor = Some(now);
        }
        if event.is_none() {
            return Ok(Outcome::empty());
        }

        let requirement = self.effective_requirement(now);
        self.count += 1;
        if self.count >= requirement {
            self.count = 0;
            self.anchor = Some(now);
            return Ok(Outcome::reinforced(Reinforcer::at(now)));
        }
        Ok(Outcome::empty())
    }

    fn reset(&mut self) {
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
    fn construct_rejects_zero_ratio() {
        assert!(matches!(
            InterlockingSchedule::new(0, 10.0),
            Err(ContingencyError::Config(_))
        ));
    }

    #[test]
    fn construct_rejects_non_positive_decay() {
        assert!(matches!(
            InterlockingSchedule::new(5, 0.0),
            Err(ContingencyError::Config(_))
        ));
        assert!(matches!(
            InterlockingSchedule::new(5, -1.0),
            Err(ContingencyError::Config(_))
        ));
    }

    #[test]
    fn fires_on_r0_with_no_elapsed_time() {
        // Anchor is the first step at t=0.0. Rapid responding at the
        // same timestamp keeps the requirement at R_0.
        let mut s = InterlockingSchedule::new(5, 10.0).unwrap();
        for _ in 0..4 {
            assert!(!respond(&mut s, 0.0).reinforced);
        }
        let o = respond(&mut s, 0.0);
        assert!(o.reinforced);
    }

    #[test]
    fn requirement_decays_with_elapsed_time() {
        // R_0=10, T=10 → at t=5 the requirement is 5.
        let mut s = InterlockingSchedule::new(10, 10.0).unwrap();
        // Anchor the schedule at t=0.0 via a pure tick.
        let _ = s.step(0.0, None).unwrap();
        assert_eq!(s.current_requirement(5.0), 5);
        // 5 responses at t=5 should reinforce on the 5th.
        for _ in 0..4 {
            assert!(!respond(&mut s, 5.0).reinforced);
        }
        let o = respond(&mut s, 5.0);
        assert!(o.reinforced);
    }

    #[test]
    fn zero_requirement_reinforces_any_response() {
        // R_0=4, T=2, at t=2 elapsed ≥ T → requirement 0.
        let mut s = InterlockingSchedule::new(4, 2.0).unwrap();
        let _ = s.step(0.0, None).unwrap();
        let o = respond(&mut s, 2.0);
        assert!(o.reinforced);
    }

    #[test]
    fn non_response_tick_never_reinforces() {
        let mut s = InterlockingSchedule::new(3, 5.0).unwrap();
        for t in [0.0_f64, 1.0, 2.0, 10.0] {
            let o = s.step(t, None).unwrap();
            assert!(!o.reinforced);
        }
    }

    #[test]
    fn reset_clears_state() {
        let mut s = InterlockingSchedule::new(3, 5.0).unwrap();
        respond(&mut s, 0.0);
        respond(&mut s, 0.0);
        s.reset();
        assert_eq!(s.response_count(), 0);
        // After reset, first response counts as response 1 (not reinforced).
        assert!(!respond(&mut s, 0.0).reinforced);
    }

    #[test]
    fn rejects_non_monotonic_time() {
        let mut s = InterlockingSchedule::new(3, 5.0).unwrap();
        respond(&mut s, 5.0);
        let ev = ResponseEvent::new(4.0);
        assert!(matches!(
            s.step(4.0, Some(&ev)),
            Err(ContingencyError::State(_))
        ));
    }
}

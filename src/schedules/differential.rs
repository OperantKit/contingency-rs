//! Differential reinforcement schedules: DRO, DRL, DRH.
//!
//! This module implements the three canonical differential-reinforcement
//! families of the experimental analysis of behavior, as a faithful port
//! of `contingency.schedules.differential` in the Python reference
//! implementation:
//!
//! * [`DRO`] — Differential Reinforcement of Other behavior. Reinforces
//!   the *absence* of a response over an interval. Two variants are
//!   supported via [`DroMode`]: the resetting (default) variant, in
//!   which every response resets the DRO timer, and the momentary
//!   (whole-interval) variant, in which the timer runs continuously and
//!   boundary checks determine reinforcement.
//! * [`DRL`] — Differential Reinforcement of Low rate behavior.
//!   Reinforces a response whose inter-response time (IRT) is at least
//!   `interval` time units.
//! * [`DRH`] — Differential Reinforcement of High rate behavior.
//!   Reinforces a response whenever at least `response_count` responses
//!   have been emitted within the last `time_window` time units.
//!
//! Each schedule is unit-agnostic with respect to time (`now` is a
//! monotonic `f64` on a caller-declared clock) and follows the same
//! [`Schedule`] contract as the rest of `contingency::schedules`.
//!
//! # References
//!
//! - Ferster, C. B., & Skinner, B. F. (1957). *Schedules of
//!   reinforcement*. Appleton-Century-Crofts.
//! - Reynolds, G. S. (1961). Behavioral contrast. *Journal of the
//!   Experimental Analysis of Behavior*, 4(1), 57-71.
//!   <https://doi.org/10.1901/jeab.1961.4-57>
//! - Reynolds, G. S. (1964). Accurate and rapid reconditioning of
//!   spaced-responding by differential reinforcement of other
//!   behavior. *Journal of the Experimental Analysis of Behavior*,
//!   7(3), 223-224. <https://doi.org/10.1901/jeab.1964.7-223>
//! - Zeiler, M. D. (1977). Schedules of reinforcement: The controlling
//!   variables. In W. K. Honig & J. E. R. Staddon (Eds.), *Handbook of
//!   operant behavior* (pp. 201-232). Prentice-Hall.

use std::collections::VecDeque;

use crate::constants::TIME_TOL;
use crate::errors::ContingencyError;
use crate::helpers::checks::{check_event, check_time};
use crate::schedule::Schedule;
use crate::types::{Outcome, Reinforcer, ResponseEvent};
use crate::Result;

// ---------------------------------------------------------------------------
// DRO — Differential Reinforcement of Other behavior
// ---------------------------------------------------------------------------

/// Variant of [`DRO`].
///
/// * [`DroMode::Resetting`] — every response resets the DRO timer to
///   `now`. A reinforcer is delivered on the first step whose `now`
///   satisfies `now - anchor >= interval` (within [`TIME_TOL`]) with
///   no intervening response. After reinforcement the timer is
///   restarted from `now`.
/// * [`DroMode::Momentary`] — a.k.a. whole-interval DRO. The timer
///   runs continuously, independent of responses. At every interval
///   boundary (`now >= anchor + interval`) a reinforcer is delivered
///   iff no response occurred in the half-open window `[anchor, now)`.
///   The anchor is advanced to `now` on every boundary regardless of
///   outcome.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DroMode {
    /// Every response resets the DRO timer.
    Resetting,
    /// Timer runs continuously; boundary check decides reinforcement.
    Momentary,
}

/// Differential Reinforcement of Other behavior.
///
/// # Notes
///
/// The schedule's anchor is established on the *first* call to
/// [`Schedule::step`] — before that first call the schedule has no
/// notion of "now". This mirrors other time-anchored schedules
/// ([`crate::schedules::FT`], [`crate::schedules::VT`], etc.).
///
/// For the resetting variant, if a response arrives exactly on or
/// after the interval boundary the step performs the reinforcement
/// *and* registers the response, meaning the next DRO timer is
/// anchored at `now`. (Python reference: the event branch short-
/// circuits, so a boundary-coincident event resets the timer but
/// does not reinforce on that step.)
///
/// For the momentary variant, responses never reset the timer but
/// they do block reinforcement on the *next* boundary check. The
/// membership window is half-open: a response whose timestamp equals
/// the boundary counts for the *following* window, not the window
/// just closing.
#[derive(Debug, Clone)]
pub struct DRO {
    interval: f64,
    mode: DroMode,
    anchor: Option<f64>,
    has_response_in_window: bool,
    last_now: Option<f64>,
}

impl DRO {
    /// Construct a DRO schedule with the given `interval` and `mode`.
    ///
    /// # Errors
    ///
    /// Returns [`ContingencyError::Config`] when `interval` is
    /// non-finite or `<= 0`.
    pub fn new(interval: f64, mode: DroMode) -> Result<Self> {
        if !interval.is_finite() || interval <= 0.0 {
            return Err(ContingencyError::Config(format!(
                "DRO requires interval > 0 and finite, got {interval}"
            )));
        }
        Ok(Self {
            interval,
            mode,
            anchor: None,
            has_response_in_window: false,
            last_now: None,
        })
    }

    /// Construct a resetting DRO — every event resets the timer.
    pub fn resetting(interval: f64) -> Result<Self> {
        Self::new(interval, DroMode::Resetting)
    }

    /// Construct a momentary DRO — timer runs independently of events.
    pub fn momentary(interval: f64) -> Result<Self> {
        Self::new(interval, DroMode::Momentary)
    }

    /// The configured DRO interval.
    pub fn interval(&self) -> f64 {
        self.interval
    }

    /// The configured DRO variant.
    pub fn mode(&self) -> DroMode {
        self.mode
    }

    fn step_resetting(&mut self, now: f64, event: Option<&ResponseEvent>) -> Outcome {
        if event.is_some() {
            // Any response resets the DRO timer. No reinforcement.
            self.anchor = Some(now);
            return Outcome::empty();
        }
        let anchor = self.anchor.expect("anchor set after first step");
        if now - anchor + TIME_TOL >= self.interval {
            self.anchor = Some(now);
            return Outcome::reinforced(Reinforcer::at(now));
        }
        Outcome::empty()
    }

    fn step_momentary(&mut self, now: f64, event: Option<&ResponseEvent>) -> Outcome {
        // Evaluate the boundary FIRST, before recording the incoming
        // event: the window [anchor, now) is half-open, so a response
        // timestamped at `now` belongs to the next window.
        let anchor = self.anchor.expect("anchor set after first step");
        let mut reinforced = false;
        if now - anchor + TIME_TOL >= self.interval {
            if !self.has_response_in_window {
                reinforced = true;
            }
            // Advance to the next interval regardless of outcome; the
            // in-window flag is reset because we are opening a fresh
            // window anchored at `now`.
            self.anchor = Some(now);
            self.has_response_in_window = false;
        }
        if event.is_some() {
            self.has_response_in_window = true;
        }
        if reinforced {
            Outcome::reinforced(Reinforcer::at(now))
        } else {
            Outcome::empty()
        }
    }
}

impl Schedule for DRO {
    fn step(&mut self, now: f64, event: Option<&ResponseEvent>) -> Result<Outcome> {
        check_time(now, self.last_now)?;
        check_event(now, event)?;
        self.last_now = Some(now);

        // First step anchors the timer at `now`.
        if self.anchor.is_none() {
            self.anchor = Some(now);
            if event.is_some() {
                // An event on the anchor step starts the first window
                // with a response already recorded (momentary variant)
                // and leaves the resetting timer pinned at `now`.
                self.has_response_in_window = true;
            }
            return Ok(Outcome::empty());
        }

        match self.mode {
            DroMode::Resetting => Ok(self.step_resetting(now, event)),
            DroMode::Momentary => Ok(self.step_momentary(now, event)),
        }
    }

    fn reset(&mut self) {
        self.anchor = None;
        self.has_response_in_window = false;
        self.last_now = None;
    }
}

// ---------------------------------------------------------------------------
// DRL — Differential Reinforcement of Low rate behavior
// ---------------------------------------------------------------------------

/// Differential Reinforcement of Low rate behavior.
///
/// A response at `now` is reinforced iff either:
///
/// * it is the very first response since construction (or
///   [`Schedule::reset`]), or
/// * the previous response occurred at least `interval` time units
///   before `now` (i.e. `now - last_response_time >= interval`,
///   within [`TIME_TOL`]).
///
/// Every response — reinforced or not — updates the internal
/// last-response bookmark to `now`. The IRT clock is therefore always
/// measured between consecutive responses. Steps with `event = None`
/// never modify state beyond the monotonic-time check and never
/// produce reinforcement.
#[derive(Debug, Clone)]
pub struct DRL {
    interval: f64,
    last_response_time: Option<f64>,
    last_now: Option<f64>,
}

impl DRL {
    /// Construct a DRL schedule with the given minimum IRT `interval`.
    ///
    /// # Errors
    ///
    /// Returns [`ContingencyError::Config`] when `interval` is
    /// non-finite or `<= 0`.
    pub fn new(interval: f64) -> Result<Self> {
        if !interval.is_finite() || interval <= 0.0 {
            return Err(ContingencyError::Config(format!(
                "DRL requires interval > 0 and finite, got {interval}"
            )));
        }
        Ok(Self {
            interval,
            last_response_time: None,
            last_now: None,
        })
    }

    /// The configured minimum IRT.
    pub fn interval(&self) -> f64 {
        self.interval
    }
}

impl Schedule for DRL {
    fn step(&mut self, now: f64, event: Option<&ResponseEvent>) -> Result<Outcome> {
        check_time(now, self.last_now)?;
        check_event(now, event)?;
        self.last_now = Some(now);

        if event.is_none() {
            return Ok(Outcome::empty());
        }
        let prev = self.last_response_time;
        // Always update the last-response bookmark; the IRT clock
        // advances with every response regardless of reinforcement.
        self.last_response_time = Some(now);
        match prev {
            None => Ok(Outcome::reinforced(Reinforcer::at(now))),
            Some(p) if now - p + TIME_TOL >= self.interval => {
                Ok(Outcome::reinforced(Reinforcer::at(now)))
            }
            Some(_) => Ok(Outcome::empty()),
        }
    }

    fn reset(&mut self) {
        self.last_response_time = None;
        self.last_now = None;
    }
}

// ---------------------------------------------------------------------------
// DRH — Differential Reinforcement of High rate behavior
// ---------------------------------------------------------------------------

/// Differential Reinforcement of High rate behavior.
///
/// The schedule maintains a FIFO [`VecDeque`] of recent response
/// timestamps. On every [`Schedule::step`] — whether or not it carries
/// an event — timestamps strictly older than `now - time_window`
/// (outside the [`TIME_TOL`] band) are evicted from the front of the
/// deque. When an event arrives, its timestamp is pushed to the back
/// of the deque; if the deque then contains at least `response_count`
/// entries, a reinforcer is delivered at `now`.
///
/// The window bound uses `>=`: a response whose timestamp equals
/// `now - time_window` is considered still within the window (within
/// [`TIME_TOL`]). This matches the intuitive reading "rate of at
/// least `response_count` per `time_window`".
///
/// The deque is **not** emptied on reinforcement — DRH cares about
/// rate, so a sustained high-rate train will continue to produce
/// reinforcers on each qualifying response. Callers wanting a
/// different policy can chain a further schedule on top.
#[derive(Debug, Clone)]
pub struct DRH {
    response_count: u32,
    time_window: f64,
    window: VecDeque<f64>,
    last_now: Option<f64>,
}

impl DRH {
    /// Construct a DRH schedule requiring `response_count` responses
    /// within the last `time_window` time units.
    ///
    /// # Errors
    ///
    /// Returns [`ContingencyError::Config`] when `response_count == 0`
    /// or `time_window` is non-finite or `<= 0`.
    pub fn new(response_count: u32, time_window: f64) -> Result<Self> {
        if response_count == 0 {
            return Err(ContingencyError::Config(format!(
                "DRH requires response_count >= 1, got {response_count}"
            )));
        }
        if !time_window.is_finite() || time_window <= 0.0 {
            return Err(ContingencyError::Config(format!(
                "DRH requires time_window > 0 and finite, got {time_window}"
            )));
        }
        Ok(Self {
            response_count,
            time_window,
            window: VecDeque::new(),
            last_now: None,
        })
    }

    /// Number of responses required within the window.
    pub fn response_count(&self) -> u32 {
        self.response_count
    }

    /// Duration of the sliding window.
    pub fn time_window(&self) -> f64 {
        self.time_window
    }

    fn evict_old(&mut self, now: f64) {
        let cutoff = now - self.time_window;
        while let Some(&front) = self.window.front() {
            if front < cutoff - TIME_TOL {
                self.window.pop_front();
            } else {
                break;
            }
        }
    }
}

impl Schedule for DRH {
    fn step(&mut self, now: f64, event: Option<&ResponseEvent>) -> Result<Outcome> {
        check_time(now, self.last_now)?;
        check_event(now, event)?;
        self.last_now = Some(now);
        self.evict_old(now);
        if event.is_none() {
            return Ok(Outcome::empty());
        }
        self.window.push_back(now);
        if self.window.len() >= self.response_count as usize {
            return Ok(Outcome::reinforced(Reinforcer::at(now)));
        }
        Ok(Outcome::empty())
    }

    fn reset(&mut self) {
        self.window.clear();
        self.last_now = None;
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // =====================================================================
    // DRO — resetting
    // =====================================================================

    #[test]
    fn dro_resetting_rejects_non_positive_interval() {
        assert!(matches!(
            DRO::resetting(0.0).unwrap_err(),
            ContingencyError::Config(_)
        ));
        assert!(matches!(
            DRO::resetting(-1.0).unwrap_err(),
            ContingencyError::Config(_)
        ));
    }

    #[test]
    fn dro_resetting_rejects_nan_and_infinite() {
        assert!(matches!(
            DRO::resetting(f64::NAN).unwrap_err(),
            ContingencyError::Config(_)
        ));
        assert!(matches!(
            DRO::resetting(f64::INFINITY).unwrap_err(),
            ContingencyError::Config(_)
        ));
    }

    #[test]
    fn dro_resetting_first_step_anchors_and_returns_empty() {
        let mut dro = DRO::resetting(5.0).unwrap();
        let out = dro.step(0.0, None).unwrap();
        assert!(!out.reinforced);
    }

    #[test]
    fn dro_resetting_no_reinforcement_before_interval() {
        let mut dro = DRO::resetting(5.0).unwrap();
        dro.step(0.0, None).unwrap();
        for &t in &[1.0, 3.0, 4.9] {
            let out = dro.step(t, None).unwrap();
            assert!(!out.reinforced, "unexpected reinforcement at t={t}");
        }
    }

    #[test]
    fn dro_resetting_reinforces_on_interval_boundary() {
        let mut dro = DRO::resetting(5.0).unwrap();
        dro.step(0.0, None).unwrap();
        let out = dro.step(5.0, None).unwrap();
        assert!(out.reinforced);
        let r = out.reinforcer.unwrap();
        assert_eq!(r.time, 5.0);
        assert_eq!(r.label, "SR+");
    }

    #[test]
    fn dro_resetting_reinforces_within_tolerance_band() {
        let mut dro = DRO::resetting(5.0).unwrap();
        dro.step(0.0, None).unwrap();
        // `5.0 - 1e-12` should be treated as the boundary within TIME_TOL.
        let t = 5.0 - 1e-12;
        let out = dro.step(t, None).unwrap();
        assert!(out.reinforced);
    }

    #[test]
    fn dro_resetting_event_inside_interval_resets_anchor() {
        let mut dro = DRO::resetting(5.0).unwrap();
        dro.step(0.0, None).unwrap();
        let ev = ResponseEvent::new(4.0);
        let out = dro.step(4.0, Some(&ev)).unwrap();
        assert!(!out.reinforced);
        // Now reinforcement should come at t = 4 + 5 = 9, not at 5.
        let not_yet = dro.step(8.0, None).unwrap();
        assert!(!not_yet.reinforced);
        let fires = dro.step(9.0, None).unwrap();
        assert!(fires.reinforced);
    }

    #[test]
    fn dro_resetting_conformance_trajectory() {
        // Mirrors conformance/differential/dro_resetting.json.
        let mut dro = DRO::resetting(5.0).unwrap();
        let steps: Vec<(f64, Option<f64>, bool)> = vec![
            (0.0, None, false),
            (3.0, None, false),
            (4.0, Some(4.0), false),
            (9.0, None, true),
            (14.0, None, true),
        ];
        for (now, ev_time, expect_rf) in steps {
            let ev = ev_time.map(ResponseEvent::new);
            let out = dro.step(now, ev.as_ref()).unwrap();
            assert_eq!(out.reinforced, expect_rf, "mismatch at now={now}");
            if expect_rf {
                assert_eq!(out.reinforcer.unwrap().time, now);
            }
        }
    }

    #[test]
    fn dro_resetting_consecutive_reinforcements() {
        // After each empty interval, another reinforcer fires.
        let mut dro = DRO::resetting(5.0).unwrap();
        dro.step(0.0, None).unwrap();
        for &t in &[5.0, 10.0, 15.0, 20.0] {
            let out = dro.step(t, None).unwrap();
            assert!(out.reinforced, "expected rf at t={t}");
        }
    }

    #[test]
    fn dro_resetting_tick_only_event_at_boundary_resets_without_fire() {
        // Python semantics: the event branch short-circuits, so a
        // response landing at the boundary resets without reinforcing.
        let mut dro = DRO::resetting(5.0).unwrap();
        dro.step(0.0, None).unwrap();
        let ev = ResponseEvent::new(5.0);
        let out = dro.step(5.0, Some(&ev)).unwrap();
        assert!(!out.reinforced);
        // Next reinforcer at 5 + 5 = 10.
        let out = dro.step(10.0, None).unwrap();
        assert!(out.reinforced);
    }

    // =====================================================================
    // DRO — momentary
    // =====================================================================

    #[test]
    fn dro_momentary_first_step_anchors() {
        let mut dro = DRO::momentary(5.0).unwrap();
        let out = dro.step(0.0, None).unwrap();
        assert!(!out.reinforced);
    }

    #[test]
    fn dro_momentary_event_does_not_reset_anchor() {
        // Momentary DRO: event at t=2 flags the window but does NOT
        // reset. The boundary at t=5 therefore fires no reinforcer
        // (because window [0,5) had a response).
        let mut dro = DRO::momentary(5.0).unwrap();
        dro.step(0.0, None).unwrap();
        let ev = ResponseEvent::new(2.0);
        let out = dro.step(2.0, Some(&ev)).unwrap();
        assert!(!out.reinforced);
        let out = dro.step(5.0, None).unwrap();
        assert!(!out.reinforced, "polluted window must not reinforce");
        // The next boundary at t=10 — window [5,10) was empty.
        let out = dro.step(10.0, None).unwrap();
        assert!(out.reinforced);
    }

    #[test]
    fn dro_momentary_boundary_without_event_reinforces() {
        let mut dro = DRO::momentary(5.0).unwrap();
        dro.step(0.0, None).unwrap();
        let out = dro.step(5.0, None).unwrap();
        assert!(out.reinforced);
        let r = out.reinforcer.unwrap();
        assert_eq!(r.time, 5.0);
    }

    #[test]
    fn dro_momentary_half_open_boundary_event_belongs_to_next_window() {
        // Anchor at 0. Boundary at 5. An event exactly at t=5 belongs
        // to window [5, 10), not [0, 5). So the boundary check at t=5
        // fires (window [0, 5) was clean); the flag is then set for
        // the next window. The boundary at t=10 should NOT reinforce.
        let mut dro = DRO::momentary(5.0).unwrap();
        dro.step(0.0, None).unwrap();
        let ev = ResponseEvent::new(5.0);
        let out = dro.step(5.0, Some(&ev)).unwrap();
        assert!(
            out.reinforced,
            "boundary-coincident event must not pollute closing window"
        );
        let out = dro.step(10.0, None).unwrap();
        assert!(!out.reinforced, "polluted next window must not reinforce");
    }

    #[test]
    fn dro_momentary_first_step_event_pollutes_first_window() {
        // Anchor at 0 with an event — first window [0, 5) is polluted.
        let mut dro = DRO::momentary(5.0).unwrap();
        let ev = ResponseEvent::new(0.0);
        dro.step(0.0, Some(&ev)).unwrap();
        let out = dro.step(5.0, None).unwrap();
        assert!(!out.reinforced);
        // Subsequent window is clean.
        let out = dro.step(10.0, None).unwrap();
        assert!(out.reinforced);
    }

    #[test]
    fn dro_momentary_conformance_trajectory() {
        // Mirrors conformance/differential/dro_momentary.json.
        let mut dro = DRO::momentary(5.0).unwrap();
        let steps: Vec<(f64, Option<f64>, bool)> = vec![
            (0.0, None, false),
            (2.0, Some(2.0), false),
            (5.0, None, false),
            (10.0, None, true),
            (12.0, Some(12.0), false),
            (15.0, None, false),
        ];
        for (now, ev_time, expect_rf) in steps {
            let ev = ev_time.map(ResponseEvent::new);
            let out = dro.step(now, ev.as_ref()).unwrap();
            assert_eq!(out.reinforced, expect_rf, "mismatch at now={now}");
            if expect_rf {
                assert_eq!(out.reinforcer.unwrap().time, now);
            }
        }
    }

    // =====================================================================
    // DRO — errors
    // =====================================================================

    #[test]
    fn dro_rejects_non_monotonic_time() {
        let mut dro = DRO::resetting(5.0).unwrap();
        dro.step(3.0, None).unwrap();
        let err = dro.step(2.0, None).unwrap_err();
        assert!(matches!(err, ContingencyError::State(_)));
    }

    #[test]
    fn dro_rejects_event_now_mismatch() {
        let mut dro = DRO::resetting(5.0).unwrap();
        let bad = ResponseEvent::new(5.0);
        let err = dro.step(4.0, Some(&bad)).unwrap_err();
        assert!(matches!(err, ContingencyError::State(_)));
    }

    #[test]
    fn dro_reset_clears_state() {
        let mut dro = DRO::resetting(5.0).unwrap();
        dro.step(0.0, None).unwrap();
        dro.step(5.0, None).unwrap();
        dro.reset();
        // After reset, clock goes back to None — first step re-anchors.
        let out = dro.step(0.0, None).unwrap();
        assert!(!out.reinforced);
        let out = dro.step(5.0, None).unwrap();
        assert!(out.reinforced);
    }

    // =====================================================================
    // DRL
    // =====================================================================

    #[test]
    fn drl_rejects_non_positive_interval() {
        assert!(matches!(
            DRL::new(-1.0).unwrap_err(),
            ContingencyError::Config(_)
        ));
        assert!(matches!(
            DRL::new(0.0).unwrap_err(),
            ContingencyError::Config(_)
        ));
    }

    #[test]
    fn drl_rejects_nan_and_infinite() {
        assert!(matches!(
            DRL::new(f64::NAN).unwrap_err(),
            ContingencyError::Config(_)
        ));
        assert!(matches!(
            DRL::new(f64::INFINITY).unwrap_err(),
            ContingencyError::Config(_)
        ));
    }

    #[test]
    fn drl_first_event_reinforces() {
        let mut drl = DRL::new(3.0).unwrap();
        let ev = ResponseEvent::new(0.0);
        let out = drl.step(0.0, Some(&ev)).unwrap();
        assert!(out.reinforced);
        assert_eq!(out.reinforcer.unwrap().time, 0.0);
    }

    #[test]
    fn drl_event_within_interval_not_reinforced_but_resets_clock() {
        let mut drl = DRL::new(3.0).unwrap();
        let e0 = ResponseEvent::new(0.0);
        drl.step(0.0, Some(&e0)).unwrap();
        // Too soon — no reinforcement.
        let e1 = ResponseEvent::new(1.0);
        let out = drl.step(1.0, Some(&e1)).unwrap();
        assert!(!out.reinforced);
        // Clock reset to t=1, so reinforcer comes at t=4, not t=3.
        let e2 = ResponseEvent::new(3.0);
        let out = drl.step(3.0, Some(&e2)).unwrap();
        assert!(!out.reinforced, "IRT 2s < 3s should not reinforce");
        let e3 = ResponseEvent::new(6.0);
        let out = drl.step(6.0, Some(&e3)).unwrap();
        assert!(out.reinforced);
    }

    #[test]
    fn drl_event_after_interval_reinforces() {
        let mut drl = DRL::new(3.0).unwrap();
        let e0 = ResponseEvent::new(0.0);
        drl.step(0.0, Some(&e0)).unwrap();
        let e1 = ResponseEvent::new(5.0);
        let out = drl.step(5.0, Some(&e1)).unwrap();
        assert!(out.reinforced);
    }

    #[test]
    fn drl_conformance_trajectory() {
        // Mirrors conformance/differential/drl_basic.json.
        let mut drl = DRL::new(3.0).unwrap();
        let steps: Vec<(f64, Option<f64>, bool)> = vec![
            (0.0, Some(0.0), true),
            (1.0, Some(1.0), false),
            (5.0, Some(5.0), true),
            (6.0, Some(6.0), false),
            (9.0, Some(9.0), true),
        ];
        for (now, ev_time, expect_rf) in steps {
            let ev = ev_time.map(ResponseEvent::new);
            let out = drl.step(now, ev.as_ref()).unwrap();
            assert_eq!(out.reinforced, expect_rf, "mismatch at now={now}");
            if expect_rf {
                assert_eq!(out.reinforcer.unwrap().time, now);
            }
        }
    }

    #[test]
    fn drl_tick_never_reinforces() {
        let mut drl = DRL::new(3.0).unwrap();
        for &t in &[0.0, 1.0, 5.0, 10.0] {
            let out = drl.step(t, None).unwrap();
            assert!(!out.reinforced);
        }
    }

    #[test]
    fn drl_rejects_non_monotonic_time() {
        let mut drl = DRL::new(3.0).unwrap();
        drl.step(5.0, None).unwrap();
        let err = drl.step(4.0, None).unwrap_err();
        assert!(matches!(err, ContingencyError::State(_)));
    }

    #[test]
    fn drl_rejects_event_now_mismatch() {
        let mut drl = DRL::new(3.0).unwrap();
        let bad = ResponseEvent::new(5.0);
        let err = drl.step(4.0, Some(&bad)).unwrap_err();
        assert!(matches!(err, ContingencyError::State(_)));
    }

    #[test]
    fn drl_reset_clears_state() {
        let mut drl = DRL::new(3.0).unwrap();
        let e = ResponseEvent::new(0.0);
        drl.step(0.0, Some(&e)).unwrap();
        drl.reset();
        // After reset, the next event is "first" again.
        let e = ResponseEvent::new(0.0);
        let out = drl.step(0.0, Some(&e)).unwrap();
        assert!(out.reinforced);
    }

    // =====================================================================
    // DRH
    // =====================================================================

    #[test]
    fn drh_rejects_zero_response_count() {
        assert!(matches!(
            DRH::new(0, 5.0).unwrap_err(),
            ContingencyError::Config(_)
        ));
    }

    #[test]
    fn drh_rejects_non_positive_time_window() {
        assert!(matches!(
            DRH::new(3, 0.0).unwrap_err(),
            ContingencyError::Config(_)
        ));
        assert!(matches!(
            DRH::new(3, -1.0).unwrap_err(),
            ContingencyError::Config(_)
        ));
    }

    #[test]
    fn drh_rejects_nan_and_infinite_time_window() {
        assert!(matches!(
            DRH::new(3, f64::NAN).unwrap_err(),
            ContingencyError::Config(_)
        ));
        assert!(matches!(
            DRH::new(3, f64::INFINITY).unwrap_err(),
            ContingencyError::Config(_)
        ));
    }

    #[test]
    fn drh_first_two_unreinforced_third_reinforces() {
        // DRH(3, 5.0): first 2 responses do not reinforce; the 3rd
        // within 5s reinforces.
        let mut drh = DRH::new(3, 5.0).unwrap();
        let e1 = ResponseEvent::new(0.0);
        assert!(!drh.step(0.0, Some(&e1)).unwrap().reinforced);
        let e2 = ResponseEvent::new(1.0);
        assert!(!drh.step(1.0, Some(&e2)).unwrap().reinforced);
        let e3 = ResponseEvent::new(2.0);
        let out = drh.step(2.0, Some(&e3)).unwrap();
        assert!(out.reinforced);
        assert_eq!(out.reinforcer.unwrap().time, 2.0);
    }

    #[test]
    fn drh_window_evicts_stale_timestamps() {
        // DRH(3, 5.0): 4 events spanning >5s — only the last few count.
        let mut drh = DRH::new(3, 5.0).unwrap();
        let e1 = ResponseEvent::new(0.0);
        drh.step(0.0, Some(&e1)).unwrap();
        let e2 = ResponseEvent::new(1.0);
        drh.step(1.0, Some(&e2)).unwrap();
        // 10s later — events at 0 and 1 are now far outside the window.
        let e3 = ResponseEvent::new(10.0);
        assert!(!drh.step(10.0, Some(&e3)).unwrap().reinforced);
        // Still only 1 event within [10-5, 10], need 3.
        let e4 = ResponseEvent::new(11.0);
        assert!(!drh.step(11.0, Some(&e4)).unwrap().reinforced);
        let e5 = ResponseEvent::new(12.0);
        assert!(drh.step(12.0, Some(&e5)).unwrap().reinforced);
    }

    #[test]
    fn drh_sustained_high_rate_keeps_reinforcing() {
        // Deque is NOT cleared on reinforcement.
        let mut drh = DRH::new(3, 5.0).unwrap();
        for &t in &[0.0, 1.0, 2.0] {
            let e = ResponseEvent::new(t);
            drh.step(t, Some(&e)).unwrap();
        }
        // The next response (still within window) keeps firing.
        let e = ResponseEvent::new(3.0);
        let out = drh.step(3.0, Some(&e)).unwrap();
        assert!(out.reinforced);
        let e = ResponseEvent::new(4.0);
        let out = drh.step(4.0, Some(&e)).unwrap();
        assert!(out.reinforced);
    }

    #[test]
    fn drh_conformance_trajectory() {
        // Mirrors conformance/differential/drh_basic.json.
        let mut drh = DRH::new(3, 2.0).unwrap();
        let steps: Vec<(f64, Option<f64>, bool)> = vec![
            (0.0, Some(0.0), false),
            (0.5, Some(0.5), false),
            (1.0, Some(1.0), true),
            (1.5, Some(1.5), true),
            (5.0, Some(5.0), false),
        ];
        for (now, ev_time, expect_rf) in steps {
            let ev = ev_time.map(ResponseEvent::new);
            let out = drh.step(now, ev.as_ref()).unwrap();
            assert_eq!(out.reinforced, expect_rf, "mismatch at now={now}");
            if expect_rf {
                assert_eq!(out.reinforcer.unwrap().time, now);
            }
        }
    }

    #[test]
    fn drh_tick_only_never_reinforces() {
        let mut drh = DRH::new(3, 5.0).unwrap();
        for &t in &[0.0, 1.0, 2.0, 3.0, 4.0] {
            assert!(!drh.step(t, None).unwrap().reinforced);
        }
    }

    #[test]
    fn drh_response_count_one_fires_on_every_event() {
        let mut drh = DRH::new(1, 1.0).unwrap();
        for &t in &[0.0, 0.5, 1.0] {
            let e = ResponseEvent::new(t);
            assert!(drh.step(t, Some(&e)).unwrap().reinforced, "t={t}");
        }
    }

    #[test]
    fn drh_rejects_non_monotonic_time() {
        let mut drh = DRH::new(3, 5.0).unwrap();
        drh.step(3.0, None).unwrap();
        let err = drh.step(2.0, None).unwrap_err();
        assert!(matches!(err, ContingencyError::State(_)));
    }

    #[test]
    fn drh_rejects_event_now_mismatch() {
        let mut drh = DRH::new(3, 5.0).unwrap();
        let bad = ResponseEvent::new(5.0);
        let err = drh.step(4.0, Some(&bad)).unwrap_err();
        assert!(matches!(err, ContingencyError::State(_)));
    }

    #[test]
    fn drh_reset_clears_state() {
        let mut drh = DRH::new(3, 5.0).unwrap();
        for &t in &[0.0, 1.0, 2.0] {
            let e = ResponseEvent::new(t);
            drh.step(t, Some(&e)).unwrap();
        }
        drh.reset();
        // Fresh: first event no longer triggers rf (count-of-1 < 3).
        let e = ResponseEvent::new(0.0);
        assert!(!drh.step(0.0, Some(&e)).unwrap().reinforced);
    }
}

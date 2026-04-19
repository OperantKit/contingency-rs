//! Interpolate compound schedule — mid-session probe windows.
//!
//! An Interpolate runs a `base` schedule as the default for the
//! session and periodically swaps to a `probe` schedule for a brief
//! window (`probe_duration`). Only one component is "on" at a time:
//! during a probe window only `probe` receives events and ticks;
//! outside the window only `base` does. The meta dictionary reports
//! the current window via `in_probe: bool`.
//!
//! The first probe window opens at `first_probe_at` (default:
//! `interval` after session start — the anchor is the first `step`'s
//! `now`). Successive probe windows open every `interval` seconds
//! from that anchor, i.e. at `anchor + first_probe_at + k * interval`
//! for `k >= 0`. A window covers `[probe_start, probe_start +
//! probe_duration)`.
//!
//! # References
//!
//! Catania, A. C., & Reynolds, G. S. (1968). A quantitative analysis
//! of responding maintained by interval schedules of reinforcement.
//! *Journal of the Experimental Analysis of Behavior*, 11(3s),
//! 327-383. <https://doi.org/10.1901/jeab.1968.11-s327>

use crate::constants::TIME_TOL;
use crate::errors::ContingencyError;
use crate::helpers::checks::{check_event, check_time};
use crate::schedule::Schedule;
use crate::types::{MetaValue, Outcome, ResponseEvent};
use crate::Result;

/// Interpolate compound schedule.
pub struct Interpolate {
    base: Box<dyn Schedule>,
    probe: Box<dyn Schedule>,
    interval: f64,
    probe_duration: f64,
    first_probe_at: f64,
    anchor: Option<f64>,
    last_now: Option<f64>,
}

impl std::fmt::Debug for Interpolate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Interpolate")
            .field("interval", &self.interval)
            .field("probe_duration", &self.probe_duration)
            .field("first_probe_at", &self.first_probe_at)
            .field("anchor", &self.anchor)
            .field("last_now", &self.last_now)
            .finish()
    }
}

impl Interpolate {
    /// Construct an `Interpolate` schedule.
    ///
    /// `first_probe_at = None` defaults to `interval`.
    pub fn new(
        base: Box<dyn Schedule>,
        probe: Box<dyn Schedule>,
        interval: f64,
        probe_duration: f64,
        first_probe_at: Option<f64>,
    ) -> Result<Self> {
        if !interval.is_finite() || interval <= 0.0 {
            return Err(ContingencyError::Config(format!(
                "Interpolate requires interval > 0, got {interval}"
            )));
        }
        if !probe_duration.is_finite() || probe_duration <= 0.0 {
            return Err(ContingencyError::Config(format!(
                "Interpolate requires probe_duration > 0, got {probe_duration}"
            )));
        }
        if probe_duration >= interval {
            return Err(ContingencyError::Config(format!(
                "Interpolate requires probe_duration ({probe_duration}) < interval ({interval})"
            )));
        }
        let first = match first_probe_at {
            None => interval,
            Some(v) => {
                if !v.is_finite() || v < 0.0 {
                    return Err(ContingencyError::Config(format!(
                        "Interpolate requires first_probe_at >= 0, got {v}"
                    )));
                }
                v
            }
        };
        Ok(Self {
            base,
            probe,
            interval,
            probe_duration,
            first_probe_at: first,
            anchor: None,
            last_now: None,
        })
    }

    /// Configured interval.
    pub fn interval(&self) -> f64 {
        self.interval
    }

    /// Configured probe duration.
    pub fn probe_duration(&self) -> f64 {
        self.probe_duration
    }

    /// Configured first-probe offset.
    pub fn first_probe_at(&self) -> f64 {
        self.first_probe_at
    }

    fn in_probe_window(&self, now: f64) -> bool {
        let Some(anchor) = self.anchor else {
            return false;
        };
        let t = now - anchor - self.first_probe_at;
        if t < -TIME_TOL {
            return false;
        }
        // Within the current interval, the window spans [0, probe_duration).
        let frac = t - (t / self.interval).floor() * self.interval;
        frac < self.probe_duration - TIME_TOL
    }
}

impl Schedule for Interpolate {
    fn step(&mut self, now: f64, event: Option<&ResponseEvent>) -> Result<Outcome> {
        check_time(now, self.last_now)?;
        check_event(now, event)?;
        if self.anchor.is_none() {
            self.anchor = Some(now);
        }
        self.last_now = Some(now);

        let in_probe = self.in_probe_window(now);
        let inner = if in_probe {
            self.probe.step(now, event)?
        } else {
            self.base.step(now, event)?
        };
        let mut meta = inner.meta.clone();
        meta.insert("in_probe".to_string(), MetaValue::Bool(in_probe));
        Ok(Outcome {
            reinforced: inner.reinforced,
            reinforcer: inner.reinforcer,
            meta,
        })
    }

    fn reset(&mut self) {
        self.base.reset();
        self.probe.reset();
        self.anchor = None;
        self.last_now = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schedules::{EXT, FR};

    fn ev(t: f64) -> ResponseEvent {
        ResponseEvent::new(t)
    }

    // --- Config --------------------------------------------------------

    #[test]
    fn rejects_non_positive_interval() {
        let err = Interpolate::new(
            Box::new(FR::new(1).unwrap()),
            Box::new(FR::new(2).unwrap()),
            0.0,
            1.0,
            None,
        )
        .unwrap_err();
        assert!(matches!(err, ContingencyError::Config(_)));
    }

    #[test]
    fn rejects_non_positive_probe_duration() {
        let err = Interpolate::new(
            Box::new(FR::new(1).unwrap()),
            Box::new(FR::new(2).unwrap()),
            10.0,
            0.0,
            None,
        )
        .unwrap_err();
        assert!(matches!(err, ContingencyError::Config(_)));
    }

    #[test]
    fn rejects_probe_duration_geq_interval() {
        let err = Interpolate::new(
            Box::new(FR::new(1).unwrap()),
            Box::new(FR::new(2).unwrap()),
            5.0,
            5.0,
            None,
        )
        .unwrap_err();
        assert!(matches!(err, ContingencyError::Config(_)));
    }

    #[test]
    fn rejects_negative_first_probe_at() {
        let err = Interpolate::new(
            Box::new(FR::new(1).unwrap()),
            Box::new(FR::new(2).unwrap()),
            10.0,
            1.0,
            Some(-1.0),
        )
        .unwrap_err();
        assert!(matches!(err, ContingencyError::Config(_)));
    }

    #[test]
    fn default_first_probe_at_equals_interval() {
        let sch = Interpolate::new(
            Box::new(FR::new(1).unwrap()),
            Box::new(FR::new(2).unwrap()),
            10.0,
            1.0,
            None,
        )
        .unwrap();
        assert!((sch.first_probe_at() - 10.0).abs() < 1e-9);
    }

    // --- Step ---------------------------------------------------------

    #[test]
    fn starts_outside_probe() {
        let mut sch = Interpolate::new(
            Box::new(FR::new(1).unwrap()),
            Box::new(EXT::new()),
            10.0,
            2.0,
            Some(5.0),
        )
        .unwrap();
        let out = sch.step(0.0, Some(&ev(0.0))).unwrap();
        assert_eq!(out.meta.get("in_probe"), Some(&MetaValue::Bool(false)));
        assert!(out.reinforced);
    }

    #[test]
    fn enters_probe_window_at_first_probe_at() {
        let mut sch = Interpolate::new(
            Box::new(EXT::new()),
            Box::new(FR::new(1).unwrap()),
            10.0,
            2.0,
            Some(5.0),
        )
        .unwrap();
        sch.step(0.0, None).unwrap();
        let out = sch.step(5.0, Some(&ev(5.0))).unwrap();
        assert_eq!(out.meta.get("in_probe"), Some(&MetaValue::Bool(true)));
        assert!(out.reinforced);
    }

    #[test]
    fn exits_probe_window_after_probe_duration() {
        let mut sch = Interpolate::new(
            Box::new(EXT::new()),
            Box::new(FR::new(1).unwrap()),
            10.0,
            2.0,
            Some(5.0),
        )
        .unwrap();
        sch.step(0.0, None).unwrap();
        let out = sch.step(5.0, Some(&ev(5.0))).unwrap();
        assert_eq!(out.meta.get("in_probe"), Some(&MetaValue::Bool(true)));
        let out = sch.step(7.5, Some(&ev(7.5))).unwrap();
        assert_eq!(out.meta.get("in_probe"), Some(&MetaValue::Bool(false)));
        assert!(!out.reinforced);
    }

    #[test]
    fn probe_window_repeats_every_interval() {
        let mut sch = Interpolate::new(
            Box::new(EXT::new()),
            Box::new(FR::new(1).unwrap()),
            10.0,
            2.0,
            Some(5.0),
        )
        .unwrap();
        sch.step(0.0, None).unwrap();
        assert_eq!(
            sch.step(5.0, Some(&ev(5.0))).unwrap().meta.get("in_probe"),
            Some(&MetaValue::Bool(true))
        );
        assert_eq!(
            sch.step(8.0, Some(&ev(8.0))).unwrap().meta.get("in_probe"),
            Some(&MetaValue::Bool(false))
        );
        assert_eq!(
            sch.step(15.0, Some(&ev(15.0)))
                .unwrap()
                .meta
                .get("in_probe"),
            Some(&MetaValue::Bool(true))
        );
        assert_eq!(
            sch.step(16.5, Some(&ev(16.5)))
                .unwrap()
                .meta
                .get("in_probe"),
            Some(&MetaValue::Bool(true))
        );
        assert_eq!(
            sch.step(18.0, Some(&ev(18.0)))
                .unwrap()
                .meta
                .get("in_probe"),
            Some(&MetaValue::Bool(false))
        );
    }

    #[test]
    fn base_state_persists_across_probe() {
        let mut sch = Interpolate::new(
            Box::new(FR::new(3).unwrap()),
            Box::new(EXT::new()),
            10.0,
            2.0,
            Some(5.0),
        )
        .unwrap();
        sch.step(0.0, Some(&ev(0.0))).unwrap();
        sch.step(1.0, Some(&ev(1.0))).unwrap();
        // During probe (EXT), not reinforced.
        let out = sch.step(5.0, Some(&ev(5.0))).unwrap();
        assert_eq!(out.meta.get("in_probe"), Some(&MetaValue::Bool(true)));
        assert!(!out.reinforced);
        // After probe, the post-probe response completes FR(3).
        let out = sch.step(8.0, Some(&ev(8.0))).unwrap();
        assert_eq!(out.meta.get("in_probe"), Some(&MetaValue::Bool(false)));
        assert!(out.reinforced);
    }

    #[test]
    fn reset_clears_anchor_and_components() {
        let mut sch = Interpolate::new(
            Box::new(FR::new(1).unwrap()),
            Box::new(EXT::new()),
            10.0,
            2.0,
            Some(5.0),
        )
        .unwrap();
        sch.step(0.0, Some(&ev(0.0))).unwrap();
        sch.reset();
        let out = sch.step(0.0, Some(&ev(0.0))).unwrap();
        assert_eq!(out.meta.get("in_probe"), Some(&MetaValue::Bool(false)));
        assert!(out.reinforced);
    }

    #[test]
    fn non_monotonic_time_raises() {
        let mut sch = Interpolate::new(
            Box::new(FR::new(1).unwrap()),
            Box::new(FR::new(2).unwrap()),
            10.0,
            1.0,
            None,
        )
        .unwrap();
        sch.step(1.0, Some(&ev(1.0))).unwrap();
        let err = sch.step(0.5, Some(&ev(0.5))).unwrap_err();
        assert!(matches!(err, ContingencyError::State(_)));
    }

    #[test]
    fn event_time_mismatch_raises() {
        let mut sch = Interpolate::new(
            Box::new(FR::new(1).unwrap()),
            Box::new(FR::new(2).unwrap()),
            10.0,
            1.0,
            None,
        )
        .unwrap();
        let bad = ResponseEvent::new(0.9);
        let err = sch.step(1.0, Some(&bad)).unwrap_err();
        assert!(matches!(err, ContingencyError::State(_)));
    }

    #[test]
    fn first_probe_at_zero_means_probe_active_from_start() {
        let mut sch = Interpolate::new(
            Box::new(EXT::new()),
            Box::new(FR::new(1).unwrap()),
            10.0,
            2.0,
            Some(0.0),
        )
        .unwrap();
        let out = sch.step(0.0, Some(&ev(0.0))).unwrap();
        assert_eq!(out.meta.get("in_probe"), Some(&MetaValue::Bool(true)));
        assert!(out.reinforced);
    }

    #[test]
    fn probe_window_boundary_half_open() {
        let mut sch = Interpolate::new(
            Box::new(EXT::new()),
            Box::new(FR::new(1).unwrap()),
            10.0,
            2.0,
            Some(5.0),
        )
        .unwrap();
        sch.step(0.0, None).unwrap();
        let out = sch.step(7.0, Some(&ev(7.0))).unwrap();
        assert_eq!(out.meta.get("in_probe"), Some(&MetaValue::Bool(false)));
    }

    #[test]
    fn meta_passthrough_from_inner_schedule() {
        let mut sch = Interpolate::new(
            Box::new(FR::new(1).unwrap()),
            Box::new(EXT::new()),
            10.0,
            2.0,
            Some(5.0),
        )
        .unwrap();
        let out = sch.step(0.0, Some(&ev(0.0))).unwrap();
        assert!(out.meta.contains_key("in_probe"));
    }
}

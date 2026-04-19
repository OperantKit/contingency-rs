//! Unified factory facade for schedule construction.
//!
//! [`ScheduleBuilder`] mirrors the PyO3 `PySchedule` classmethod API
//! so that Rust-native callers have the same ergonomic entrypoint as
//! Python consumers. All methods return `Box<dyn Schedule>` so the
//! facade is compositional.

use indexmap::IndexMap;

use crate::schedules::{self, DroMode, LimitedHold, ProgressiveRatio};
use crate::{Result, Schedule};

/// Unified factory facade for schedule construction.
///
/// This is a zero-sized namespace type; every method is an associated
/// function that wraps one concrete schedule constructor and returns a
/// type-erased `Box<dyn Schedule>`. The set of methods mirrors the
/// PyO3 `Schedule` classmethod surface in `python.rs`.
pub struct ScheduleBuilder;

impl ScheduleBuilder {
    // ------------------------------------------------------------------
    // Ratio family
    // ------------------------------------------------------------------

    /// Fixed-ratio schedule (FR n).
    pub fn fr(n: u64) -> Result<Box<dyn Schedule>> {
        Ok(Box::new(schedules::FR::new(n)?))
    }

    /// Continuous-reinforcement schedule (CRF == FR 1).
    pub fn crf() -> Box<dyn Schedule> {
        Box::new(schedules::crf())
    }

    /// Variable-ratio schedule (VR mean).
    pub fn vr(mean: f64, n_intervals: usize, seed: Option<u64>) -> Result<Box<dyn Schedule>> {
        Ok(Box::new(schedules::VR::new(mean, n_intervals, seed)?))
    }

    /// Random-ratio schedule (RR p).
    pub fn rr(probability: f64, seed: Option<u64>) -> Result<Box<dyn Schedule>> {
        Ok(Box::new(schedules::RR::new(probability, seed)?))
    }

    // ------------------------------------------------------------------
    // Interval family
    // ------------------------------------------------------------------

    /// Fixed-interval schedule (FI t).
    pub fn fi(interval: f64) -> Result<Box<dyn Schedule>> {
        Ok(Box::new(schedules::FI::new(interval)?))
    }

    /// Variable-interval schedule (VI t).
    pub fn vi(
        mean_interval: f64,
        n_intervals: usize,
        seed: Option<u64>,
    ) -> Result<Box<dyn Schedule>> {
        Ok(Box::new(schedules::VI::new(
            mean_interval,
            n_intervals,
            seed,
        )?))
    }

    /// Random-interval schedule (RI t).
    pub fn ri(mean_interval: f64, seed: Option<u64>) -> Result<Box<dyn Schedule>> {
        Ok(Box::new(schedules::RI::new(mean_interval, seed)?))
    }

    // ------------------------------------------------------------------
    // Limited-hold wrappers
    // ------------------------------------------------------------------

    /// Fixed-interval schedule wrapped in a limited-hold.
    pub fn limited_hold_fi(interval: f64, hold: f64) -> Result<Box<dyn Schedule>> {
        let inner = schedules::FI::new(interval)?;
        Ok(Box::new(LimitedHold::new(inner, hold)?))
    }

    /// Variable-interval schedule wrapped in a limited-hold.
    pub fn limited_hold_vi(
        mean_interval: f64,
        n_intervals: usize,
        seed: Option<u64>,
        hold: f64,
    ) -> Result<Box<dyn Schedule>> {
        let inner = schedules::VI::new(mean_interval, n_intervals, seed)?;
        Ok(Box::new(LimitedHold::new(inner, hold)?))
    }

    /// Random-interval schedule wrapped in a limited-hold.
    pub fn limited_hold_ri(
        mean_interval: f64,
        seed: Option<u64>,
        hold: f64,
    ) -> Result<Box<dyn Schedule>> {
        let inner = schedules::RI::new(mean_interval, seed)?;
        Ok(Box::new(LimitedHold::new(inner, hold)?))
    }

    // ------------------------------------------------------------------
    // Time-based family
    // ------------------------------------------------------------------

    /// Fixed-time schedule (FT t).
    pub fn ft(interval: f64) -> Result<Box<dyn Schedule>> {
        Ok(Box::new(schedules::FT::new(interval)?))
    }

    /// Variable-time schedule (VT t).
    pub fn vt(
        mean_interval: f64,
        n_intervals: usize,
        seed: Option<u64>,
    ) -> Result<Box<dyn Schedule>> {
        Ok(Box::new(schedules::VT::new(
            mean_interval,
            n_intervals,
            seed,
        )?))
    }

    /// Random-time schedule (RT t).
    pub fn rt(mean_interval: f64, seed: Option<u64>) -> Result<Box<dyn Schedule>> {
        Ok(Box::new(schedules::RT::new(mean_interval, seed)?))
    }

    /// Extinction schedule.
    pub fn ext() -> Box<dyn Schedule> {
        Box::new(schedules::EXT::new())
    }

    // ------------------------------------------------------------------
    // Differential family
    // ------------------------------------------------------------------

    /// Differential reinforcement of other behaviour (DRO, resetting).
    pub fn dro_resetting(interval: f64) -> Result<Box<dyn Schedule>> {
        Ok(Box::new(schedules::DRO::new(interval, DroMode::Resetting)?))
    }

    /// Differential reinforcement of other behaviour (DRO, momentary).
    pub fn dro_momentary(interval: f64) -> Result<Box<dyn Schedule>> {
        Ok(Box::new(schedules::DRO::new(interval, DroMode::Momentary)?))
    }

    /// Differential reinforcement of low rates (DRL).
    pub fn drl(interval: f64) -> Result<Box<dyn Schedule>> {
        Ok(Box::new(schedules::DRL::new(interval)?))
    }

    /// Differential reinforcement of high rates (DRH).
    pub fn drh(response_count: u32, time_window: f64) -> Result<Box<dyn Schedule>> {
        Ok(Box::new(schedules::DRH::new(response_count, time_window)?))
    }

    // ------------------------------------------------------------------
    // Progressive ratio
    // ------------------------------------------------------------------

    /// Progressive-ratio schedule with arithmetic step function.
    pub fn pr_arithmetic(start: u32, step: u32) -> Result<Box<dyn Schedule>> {
        let fn_ = schedules::arithmetic(start, step)?;
        Ok(Box::new(ProgressiveRatio::new(fn_)))
    }

    /// Progressive-ratio schedule with geometric step function.
    pub fn pr_geometric(start: u32, ratio: f64) -> Result<Box<dyn Schedule>> {
        let fn_ = schedules::geometric(start, ratio)?;
        Ok(Box::new(ProgressiveRatio::new(fn_)))
    }

    /// Progressive-ratio schedule using the Richardson-Roberts series.
    pub fn pr_richardson_roberts() -> Box<dyn Schedule> {
        Box::new(ProgressiveRatio::new(schedules::richardson_roberts()))
    }

    // ------------------------------------------------------------------
    // Compound schedules (consume components)
    // ------------------------------------------------------------------

    /// Alternative (whichever-first) compound schedule.
    pub fn alternative(
        first: Box<dyn Schedule>,
        second: Box<dyn Schedule>,
    ) -> Box<dyn Schedule> {
        Box::new(schedules::Alternative::new(first, second))
    }

    /// Multiple compound schedule.
    pub fn multiple(
        components: Vec<Box<dyn Schedule>>,
        stimuli: Option<Vec<String>>,
    ) -> Result<Box<dyn Schedule>> {
        Ok(Box::new(schedules::Multiple::new(components, stimuli)?))
    }

    /// Chained compound schedule.
    pub fn chained(
        components: Vec<Box<dyn Schedule>>,
        stimuli: Option<Vec<String>>,
    ) -> Result<Box<dyn Schedule>> {
        Ok(Box::new(schedules::Chained::new(components, stimuli)?))
    }

    /// Tandem compound schedule.
    pub fn tandem(components: Vec<Box<dyn Schedule>>) -> Result<Box<dyn Schedule>> {
        Ok(Box::new(schedules::Tandem::new(components)?))
    }

    /// Concurrent compound schedule keyed by operandum.
    pub fn concurrent(
        components: IndexMap<String, Box<dyn Schedule>>,
        cod: f64,
        cor: u32,
    ) -> Result<Box<dyn Schedule>> {
        Ok(Box::new(schedules::Concurrent::new(components, cod, cor)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ResponseEvent;

    fn step_reinforced(s: &mut dyn Schedule, t: f64, ev: Option<&ResponseEvent>) -> bool {
        s.step(t, ev).unwrap().reinforced
    }

    #[test]
    fn fr_builds_and_fires_on_nth() {
        let mut s = ScheduleBuilder::fr(3).unwrap();
        assert!(!step_reinforced(&mut *s, 1.0, Some(&ResponseEvent::new(1.0))));
        assert!(!step_reinforced(&mut *s, 2.0, Some(&ResponseEvent::new(2.0))));
        assert!(step_reinforced(&mut *s, 3.0, Some(&ResponseEvent::new(3.0))));
    }

    #[test]
    fn fr_zero_is_config_error() {
        assert!(ScheduleBuilder::fr(0).is_err());
    }

    #[test]
    fn crf_fires_every_response() {
        let mut s = ScheduleBuilder::crf();
        assert!(step_reinforced(&mut *s, 1.0, Some(&ResponseEvent::new(1.0))));
        assert!(step_reinforced(&mut *s, 2.0, Some(&ResponseEvent::new(2.0))));
    }

    #[test]
    fn vr_builds() {
        let mut s = ScheduleBuilder::vr(5.0, 12, Some(42)).unwrap();
        // Drive a handful of responses — at least one should fire in a
        // VR(5) over 50 responses with any reasonable seed.
        let mut any = false;
        for i in 1..=50 {
            let t = i as f64;
            any |= step_reinforced(&mut *s, t, Some(&ResponseEvent::new(t)));
        }
        assert!(any);
    }

    #[test]
    fn rr_builds() {
        let mut s = ScheduleBuilder::rr(0.5, Some(1)).unwrap();
        let mut hits = 0;
        for i in 1..=200 {
            let t = i as f64;
            if step_reinforced(&mut *s, t, Some(&ResponseEvent::new(t))) {
                hits += 1;
            }
        }
        assert!(hits > 0 && hits < 200);
    }

    #[test]
    fn rr_bad_probability_errors() {
        assert!(ScheduleBuilder::rr(1.5, None).is_err());
    }

    #[test]
    fn fi_builds_and_fires_after_interval() {
        let mut s = ScheduleBuilder::fi(2.0).unwrap();
        assert!(!step_reinforced(&mut *s, 1.0, Some(&ResponseEvent::new(1.0))));
        assert!(step_reinforced(&mut *s, 2.5, Some(&ResponseEvent::new(2.5))));
    }

    #[test]
    fn vi_builds() {
        let s = ScheduleBuilder::vi(5.0, 12, Some(7)).unwrap();
        drop(s);
    }

    #[test]
    fn ri_builds() {
        let s = ScheduleBuilder::ri(5.0, Some(7)).unwrap();
        drop(s);
    }

    #[test]
    fn limited_hold_fi_builds() {
        let mut s = ScheduleBuilder::limited_hold_fi(2.0, 0.5).unwrap();
        // Response well after (interval + hold) misses the window.
        assert!(!step_reinforced(&mut *s, 5.0, Some(&ResponseEvent::new(5.0))));
    }

    #[test]
    fn limited_hold_vi_builds() {
        let s = ScheduleBuilder::limited_hold_vi(5.0, 8, Some(3), 1.0).unwrap();
        drop(s);
    }

    #[test]
    fn limited_hold_ri_builds() {
        let s = ScheduleBuilder::limited_hold_ri(5.0, Some(3), 1.0).unwrap();
        drop(s);
    }

    #[test]
    fn ft_builds_and_fires_at_interval() {
        let mut s = ScheduleBuilder::ft(2.0).unwrap();
        // First step anchors the clock; second step past the interval fires.
        assert!(!step_reinforced(&mut *s, 0.0, None));
        assert!(!step_reinforced(&mut *s, 1.0, None));
        assert!(step_reinforced(&mut *s, 2.5, None));
    }

    #[test]
    fn vt_builds() {
        let s = ScheduleBuilder::vt(5.0, 8, Some(9)).unwrap();
        drop(s);
    }

    #[test]
    fn rt_builds() {
        let s = ScheduleBuilder::rt(5.0, Some(9)).unwrap();
        drop(s);
    }

    #[test]
    fn ext_never_reinforces() {
        let mut s = ScheduleBuilder::ext();
        for i in 1..=10 {
            let t = i as f64;
            assert!(!step_reinforced(&mut *s, t, Some(&ResponseEvent::new(t))));
        }
    }

    #[test]
    fn dro_resetting_builds() {
        let s = ScheduleBuilder::dro_resetting(2.0).unwrap();
        drop(s);
    }

    #[test]
    fn dro_momentary_builds() {
        let s = ScheduleBuilder::dro_momentary(2.0).unwrap();
        drop(s);
    }

    #[test]
    fn drl_builds() {
        let mut s = ScheduleBuilder::drl(2.0).unwrap();
        // first response reinforces
        assert!(step_reinforced(&mut *s, 1.0, Some(&ResponseEvent::new(1.0))));
        // within-interval does not
        assert!(!step_reinforced(&mut *s, 1.5, Some(&ResponseEvent::new(1.5))));
    }

    #[test]
    fn drh_builds() {
        let s = ScheduleBuilder::drh(3, 2.0).unwrap();
        drop(s);
    }

    #[test]
    fn pr_arithmetic_builds() {
        let s = ScheduleBuilder::pr_arithmetic(1, 1).unwrap();
        drop(s);
    }

    #[test]
    fn pr_geometric_builds() {
        let s = ScheduleBuilder::pr_geometric(1, 2.0).unwrap();
        drop(s);
    }

    #[test]
    fn pr_richardson_roberts_builds() {
        let s = ScheduleBuilder::pr_richardson_roberts();
        drop(s);
    }

    #[test]
    fn alternative_builds() {
        let a = ScheduleBuilder::fr(2).unwrap();
        let b = ScheduleBuilder::fr(5).unwrap();
        let mut s = ScheduleBuilder::alternative(a, b);
        // First component (FR 2) fires first on response 2.
        assert!(!step_reinforced(&mut *s, 1.0, Some(&ResponseEvent::new(1.0))));
        assert!(step_reinforced(&mut *s, 2.0, Some(&ResponseEvent::new(2.0))));
    }

    #[test]
    fn multiple_builds() {
        let components: Vec<Box<dyn Schedule>> = vec![
            ScheduleBuilder::fr(2).unwrap(),
            ScheduleBuilder::fr(3).unwrap(),
        ];
        let s = ScheduleBuilder::multiple(components, None).unwrap();
        drop(s);
    }

    #[test]
    fn chained_builds() {
        let components: Vec<Box<dyn Schedule>> = vec![
            ScheduleBuilder::fr(2).unwrap(),
            ScheduleBuilder::fr(2).unwrap(),
        ];
        let s = ScheduleBuilder::chained(components, None).unwrap();
        drop(s);
    }

    #[test]
    fn tandem_builds() {
        let components: Vec<Box<dyn Schedule>> = vec![
            ScheduleBuilder::fr(2).unwrap(),
            ScheduleBuilder::fr(2).unwrap(),
        ];
        let s = ScheduleBuilder::tandem(components).unwrap();
        drop(s);
    }

    #[test]
    fn concurrent_builds() {
        let mut map: IndexMap<String, Box<dyn Schedule>> = IndexMap::new();
        map.insert("left".into(), ScheduleBuilder::fr(2).unwrap());
        map.insert("right".into(), ScheduleBuilder::fr(3).unwrap());
        let s = ScheduleBuilder::concurrent(map, 0.0, 0).unwrap();
        drop(s);
    }

    #[test]
    fn concurrent_single_component_errors() {
        let mut map: IndexMap<String, Box<dyn Schedule>> = IndexMap::new();
        map.insert("solo".into(), ScheduleBuilder::fr(2).unwrap());
        assert!(ScheduleBuilder::concurrent(map, 0.0, 0).is_err());
    }
}

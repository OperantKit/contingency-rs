//! Ratio-family reinforcement schedules (FR, VR, RR, CRF).
//!
//! This module implements the four canonical ratio schedules of the
//! experimental analysis of behavior, as a faithful port of
//! `contingency.schedules.ratio` in the Python reference implementation.
//!
//! * [`FR`] — Fixed Ratio. The subject must emit a fixed count of
//!   responses for each reinforcer.
//! * [`crf`] — Continuous Reinforcement, a factory returning `FR(1)`.
//!   Every response is reinforced.
//! * [`VR`] — Variable Ratio. Ratio requirements are drawn from a
//!   Fleshler-Hoffman progression with a configured mean.
//! * [`RR`] — Random Ratio. Each response is reinforced independently
//!   with probability `p`.
//!
//! Each schedule is unit-agnostic with respect to time (`now` is a
//! monotonic `f64` on a caller-declared clock). Ratio schedules use
//! `now` only to time-stamp emitted [`Reinforcer`] values; the
//! reinforcement logic is driven by the presence of a
//! [`ResponseEvent`].
//!
//! # References
//!
//! Fleshler, M., & Hoffman, H. S. (1962). A progression for generating
//! variable-interval schedules. *Journal of the Experimental Analysis
//! of Behavior*, 5(4), 529-530. <https://doi.org/10.1901/jeab.1962.5-529>
//!
//! Skinner, B. F. (1957). *Schedules of reinforcement* (with C. B.
//! Ferster). Appleton-Century-Crofts.

use rand::{rngs::SmallRng, Rng, SeedableRng};

use crate::errors::ContingencyError;
use crate::helpers::checks::{check_event, check_time};
use crate::helpers::fleshler_hoffman::generate_ratios;
use crate::schedule::Schedule;
use crate::types::{Outcome, Reinforcer, ResponseEvent};
use crate::Result;

/// Fixed-Ratio schedule: reinforce every `n`-th response.
///
/// The schedule keeps an internal response counter that is incremented
/// each time [`FR::step`] is called with `Some(event)`. When the counter
/// reaches `n` it is reset to zero and a [`Reinforcer`] timestamped at
/// `now` is emitted.
///
/// `FR(1)` is equivalent to continuous reinforcement; see [`crf`].
///
/// # References
///
/// Skinner, B. F. (1957). *Schedules of reinforcement* (with C. B.
/// Ferster). Appleton-Century-Crofts.
#[derive(Debug)]
pub struct FR {
    n: u64,
    count: u64,
    last_now: Option<f64>,
}

impl FR {
    /// Construct a fixed-ratio schedule with ratio requirement `n`.
    ///
    /// # Errors
    ///
    /// Returns [`ContingencyError::Config`] if `n < 1`.
    pub fn new(n: u64) -> Result<Self> {
        if n < 1 {
            return Err(ContingencyError::Config(format!(
                "FR requires n >= 1, got {n}"
            )));
        }
        Ok(Self {
            n,
            count: 0,
            last_now: None,
        })
    }

    /// The configured ratio requirement.
    pub fn n(&self) -> u64 {
        self.n
    }
}

impl Schedule for FR {
    fn step(&mut self, now: f64, event: Option<&ResponseEvent>) -> Result<Outcome> {
        check_time(now, self.last_now)?;
        check_event(now, event)?;
        self.last_now = Some(now);
        if event.is_none() {
            return Ok(Outcome::empty());
        }
        self.count += 1;
        if self.count >= self.n {
            self.count = 0;
            return Ok(Outcome::reinforced(Reinforcer::at(now)));
        }
        Ok(Outcome::empty())
    }

    fn reset(&mut self) {
        self.count = 0;
        self.last_now = None;
    }
}

/// Continuous Reinforcement — factory returning `FR(1)`.
///
/// Every response produces a reinforcer. CRF is the limiting case of
/// fixed ratio with ratio requirement one.
///
/// # References
///
/// Skinner, B. F. (1957). *Schedules of reinforcement* (with C. B.
/// Ferster). Appleton-Century-Crofts.
#[allow(non_snake_case)]
pub fn crf() -> FR {
    FR::new(1).expect("FR(1) is always valid")
}

/// Variable-Ratio schedule driven by a Fleshler-Hoffman progression.
///
/// Each step with a response increments an internal counter. When the
/// counter meets the current requirement, reinforcement is delivered,
/// the counter is zeroed, and the next requirement is taken from the
/// pre-generated Fleshler-Hoffman sequence. When the sequence is
/// exhausted, a fresh sequence is generated using a seed derived from
/// the internal RNG, preserving determinism under a given construction
/// seed.
///
/// # References
///
/// Fleshler, M., & Hoffman, H. S. (1962). A progression for generating
/// variable-interval schedules. *Journal of the Experimental Analysis
/// of Behavior*, 5(4), 529-530. <https://doi.org/10.1901/jeab.1962.5-529>
///
/// Skinner, B. F. (1957). *Schedules of reinforcement* (with C. B.
/// Ferster). Appleton-Century-Crofts.
#[derive(Debug)]
pub struct VR {
    mean: f64,
    n_intervals: usize,
    seed: Option<u64>,
    rng: SmallRng,
    sequence: Vec<u64>,
    cursor: usize,
    count: u64,
    requirement: u64,
    last_now: Option<f64>,
}

impl VR {
    /// Construct a variable-ratio schedule.
    ///
    /// # Parameters
    ///
    /// * `mean` — target mean ratio requirement, must be `> 0`.
    /// * `n_intervals` — number of ratio values generated per cycle;
    ///   must be `>= 1`. A common default is 12.
    /// * `seed` — optional RNG seed for deterministic sequences. When
    ///   provided, [`VR::reset`] reshuffles with a fresh sub-seed
    ///   derived from the original seed so that `reset()` replays the
    ///   same trajectory.
    ///
    /// # Errors
    ///
    /// Returns [`ContingencyError::Config`] if `mean <= 0` or
    /// `n_intervals == 0`.
    pub fn new(mean: f64, n_intervals: usize, seed: Option<u64>) -> Result<Self> {
        if !mean.is_finite() || mean <= 0.0 {
            return Err(ContingencyError::Config(format!(
                "VR requires mean > 0, got {mean}"
            )));
        }
        if n_intervals < 1 {
            return Err(ContingencyError::Config(format!(
                "VR requires n_intervals >= 1, got {n_intervals}"
            )));
        }
        let rng = match seed {
            Some(s) => SmallRng::seed_from_u64(s),
            None => SmallRng::from_entropy(),
        };
        let mut s = Self {
            mean,
            n_intervals,
            seed,
            rng,
            sequence: Vec::new(),
            cursor: 0,
            count: 0,
            requirement: 0,
            last_now: None,
        };
        s.reload_sequence();
        Ok(s)
    }

    /// Construct a VR schedule with `n_intervals = 12` and no seed.
    pub fn with_mean(mean: f64) -> Result<Self> {
        Self::new(mean, 12, None)
    }

    /// The configured mean ratio.
    pub fn mean(&self) -> f64 {
        self.mean
    }

    /// The configured number of ratio values per cycle.
    pub fn n_intervals(&self) -> usize {
        self.n_intervals
    }

    fn next_sub_seed(&mut self) -> u64 {
        self.rng.gen::<u64>()
    }

    fn reload_sequence(&mut self) {
        let sub_seed = self.next_sub_seed();
        self.sequence = generate_ratios(self.mean, self.n_intervals, Some(sub_seed));
        self.cursor = 0;
        // `generate_ratios` guarantees a non-empty vector when
        // `n_intervals >= 1` and all values are `>= 1`.
        self.requirement = self.sequence[0];
    }

    fn advance_requirement(&mut self) {
        self.cursor += 1;
        if self.cursor >= self.sequence.len() {
            self.reload_sequence();
        } else {
            self.requirement = self.sequence[self.cursor];
        }
    }
}

impl Schedule for VR {
    fn step(&mut self, now: f64, event: Option<&ResponseEvent>) -> Result<Outcome> {
        check_time(now, self.last_now)?;
        check_event(now, event)?;
        self.last_now = Some(now);
        if event.is_none() {
            return Ok(Outcome::empty());
        }
        self.count += 1;
        if self.count >= self.requirement {
            self.count = 0;
            self.advance_requirement();
            return Ok(Outcome::reinforced(Reinforcer::at(now)));
        }
        Ok(Outcome::empty())
    }

    fn reset(&mut self) {
        self.rng = match self.seed {
            Some(s) => SmallRng::seed_from_u64(s),
            None => SmallRng::from_entropy(),
        };
        self.count = 0;
        self.last_now = None;
        self.reload_sequence();
    }
}

/// Random-Ratio schedule: each response reinforced with probability `p`.
///
/// Determinism: when a `seed` is supplied, an [`SmallRng`] snapshot is
/// captured at construction. [`RR::reset`] restores the snapshot so the
/// Bernoulli draw sequence replays identically. When `seed` is `None`,
/// a fresh entropy-seeded RNG is created and its initial state is
/// snapshotted via `clone` so `reset()` still replays the original
/// draw sequence for this instance.
///
/// # References
///
/// Skinner, B. F. (1957). *Schedules of reinforcement* (with C. B.
/// Ferster). Appleton-Century-Crofts.
#[derive(Debug)]
pub struct RR {
    p: f64,
    initial_rng: SmallRng,
    rng: SmallRng,
    last_now: Option<f64>,
}

impl RR {
    /// Construct a random-ratio schedule with reinforcement probability `p`.
    ///
    /// # Parameters
    ///
    /// * `probability` — per-response Bernoulli probability, must
    ///   satisfy `0 < p <= 1`.
    /// * `seed` — optional RNG seed. If `None`, an entropy-seeded RNG
    ///   is used; [`RR::reset`] still replays the instance's original
    ///   draw sequence via an internal snapshot.
    ///
    /// # Errors
    ///
    /// Returns [`ContingencyError::Config`] if `probability` is not
    /// finite, `<= 0`, or `> 1`.
    pub fn new(probability: f64, seed: Option<u64>) -> Result<Self> {
        if !probability.is_finite() || probability <= 0.0 || probability > 1.0 {
            return Err(ContingencyError::Config(format!(
                "RR requires 0 < probability <= 1, got {probability}"
            )));
        }
        let initial_rng = match seed {
            Some(s) => SmallRng::seed_from_u64(s),
            None => SmallRng::from_entropy(),
        };
        let rng = initial_rng.clone();
        Ok(Self {
            p: probability,
            initial_rng,
            rng,
            last_now: None,
        })
    }

    /// The configured reinforcement probability.
    pub fn probability(&self) -> f64 {
        self.p
    }
}

impl Schedule for RR {
    fn step(&mut self, now: f64, event: Option<&ResponseEvent>) -> Result<Outcome> {
        check_time(now, self.last_now)?;
        check_event(now, event)?;
        self.last_now = Some(now);
        if event.is_none() {
            return Ok(Outcome::empty());
        }
        let draw: f64 = self.rng.gen::<f64>();
        if draw < self.p {
            return Ok(Outcome::reinforced(Reinforcer::at(now)));
        }
        Ok(Outcome::empty())
    }

    fn reset(&mut self) {
        self.rng = self.initial_rng.clone();
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

    // --- FR --------------------------------------------------------------

    #[test]
    fn fr_reinforces_on_exact_nth_response() {
        let mut s = FR::new(5).unwrap();
        let mut reinforced_at: Vec<usize> = Vec::new();
        for i in 1..=15usize {
            let o = respond(&mut s, i as f64);
            if o.reinforced {
                reinforced_at.push(i);
            }
        }
        assert_eq!(reinforced_at, vec![5, 10, 15]);
    }

    #[test]
    fn fr_reinforcer_timestamp_equals_now() {
        let mut s = FR::new(3).unwrap();
        respond(&mut s, 1.0);
        respond(&mut s, 2.0);
        let o = respond(&mut s, 3.0);
        assert!(o.reinforced);
        let r = o.reinforcer.expect("reinforcer");
        assert_eq!(r.time, 3.0);
        assert_eq!(r.label, "SR+");
        assert_eq!(r.magnitude, 1.0);
    }

    #[test]
    fn fr_non_response_step_does_not_advance() {
        let mut s = FR::new(3).unwrap();
        // Two ticks without events must not reinforce nor move the count.
        for t in [0.1f64, 0.2, 0.3, 0.4, 0.5] {
            let o = s.step(t, None).unwrap();
            assert!(!o.reinforced);
            assert!(o.reinforcer.is_none());
        }
        // After three real responses (starting at monotonic t=1.0..),
        // the schedule should reinforce because the tick-only steps
        // were ignored.
        respond(&mut s, 1.0);
        respond(&mut s, 2.0);
        let o = respond(&mut s, 3.0);
        assert!(o.reinforced);
    }

    #[test]
    fn fr_counter_restarts_after_reinforcement() {
        let mut s = FR::new(2).unwrap();
        let _ = respond(&mut s, 1.0);
        let o = respond(&mut s, 2.0);
        assert!(o.reinforced);
        let o = respond(&mut s, 3.0);
        assert!(!o.reinforced);
        let o = respond(&mut s, 4.0);
        assert!(o.reinforced);
    }

    #[test]
    fn fr_reset_restarts_count_and_clears_time() {
        let mut s = FR::new(4).unwrap();
        respond(&mut s, 1.0);
        respond(&mut s, 2.0);
        respond(&mut s, 3.0);
        s.reset();
        // With count restarted we need 4 more responses (from earlier time).
        for t in [1.0f64, 2.0, 3.0] {
            let o = respond(&mut s, t);
            assert!(!o.reinforced);
        }
        let o = respond(&mut s, 4.0);
        assert!(o.reinforced);
    }

    #[test]
    fn fr_zero_returns_config_error() {
        let err = FR::new(0).unwrap_err();
        assert!(matches!(err, ContingencyError::Config(_)));
    }

    #[test]
    fn fr_n_property() {
        assert_eq!(FR::new(7).unwrap().n(), 7);
    }

    // --- CRF -------------------------------------------------------------

    #[test]
    fn crf_reinforces_every_response() {
        let mut s = crf();
        assert_eq!(s.n(), 1);
        for i in 1..=20 {
            let o = respond(&mut s, i as f64);
            assert!(o.reinforced);
            let r = o.reinforcer.expect("reinforcer");
            assert_eq!(r.time, i as f64);
        }
    }

    #[test]
    fn crf_zero_responses_zero_reinforcement() {
        let mut s = crf();
        for t in [0.1f64, 0.2, 0.3] {
            let o = s.step(t, None).unwrap();
            assert!(!o.reinforced);
        }
    }

    // --- VR --------------------------------------------------------------

    fn run_until_reinforcers<S: Schedule>(schedule: &mut S, target: usize) -> Vec<u64> {
        let mut counts: Vec<u64> = Vec::with_capacity(target);
        let mut responses_since_last: u64 = 0;
        let mut t: f64 = 0.0;
        let safety_cap = (target * 1000).max(10_000);
        let mut ticks = 0usize;
        while counts.len() < target {
            ticks += 1;
            if ticks > safety_cap {
                panic!("Schedule failed to produce {target} reinforcers in {safety_cap} steps");
            }
            t += 1.0;
            responses_since_last += 1;
            let o = respond(schedule, t);
            if o.reinforced {
                counts.push(responses_since_last);
                responses_since_last = 0;
            }
        }
        counts
    }

    #[test]
    fn vr_mean_zero_is_config_error() {
        let err = VR::new(0.0, 12, Some(0)).unwrap_err();
        assert!(matches!(err, ContingencyError::Config(_)));
    }

    #[test]
    fn vr_mean_negative_is_config_error() {
        let err = VR::new(-2.0, 12, Some(0)).unwrap_err();
        assert!(matches!(err, ContingencyError::Config(_)));
    }

    #[test]
    fn vr_n_intervals_zero_is_config_error() {
        let err = VR::new(5.0, 0, Some(0)).unwrap_err();
        assert!(matches!(err, ContingencyError::Config(_)));
    }

    #[test]
    fn vr_properties() {
        let s = VR::new(7.0, 12, Some(0)).unwrap();
        assert_eq!(s.mean(), 7.0);
        assert_eq!(s.n_intervals(), 12);
    }

    #[test]
    fn vr_deterministic_under_seed() {
        let mut a = VR::new(8.0, 12, Some(123)).unwrap();
        let mut b = VR::new(8.0, 12, Some(123)).unwrap();
        let trace_a = run_until_reinforcers(&mut a, 30);
        let trace_b = run_until_reinforcers(&mut b, 30);
        assert_eq!(trace_a, trace_b);
    }

    #[test]
    fn vr_different_seeds_differ() {
        let mut a = VR::new(8.0, 12, Some(1)).unwrap();
        let mut b = VR::new(8.0, 12, Some(2)).unwrap();
        let trace_a = run_until_reinforcers(&mut a, 30);
        let trace_b = run_until_reinforcers(&mut b, 30);
        assert_ne!(trace_a, trace_b);
    }

    #[test]
    fn vr_reset_replays_trajectory() {
        let mut s = VR::new(6.0, 12, Some(42)).unwrap();
        let before = run_until_reinforcers(&mut s, 20);
        s.reset();
        let after = run_until_reinforcers(&mut s, 20);
        assert_eq!(before, after);
    }

    #[test]
    fn vr_mean_preserved_across_many_cycles() {
        // generate_ratios preserves the integer mean exactly per cycle,
        // so an exact multiple of n_intervals reinforcers yields a
        // deterministic total sum regardless of seed.
        let mean = 10.0;
        let n_intervals = 12usize;
        let cycles = 10usize;
        let target = cycles * n_intervals;
        let mut s = VR::new(mean, n_intervals, Some(0)).unwrap();
        let counts = run_until_reinforcers(&mut s, target);
        let total: u64 = counts.iter().copied().sum();
        let expected = (mean * n_intervals as f64).round() as u64 * cycles as u64;
        assert_eq!(total, expected);
    }

    #[test]
    fn vr_sequence_regenerates_after_exhaustion() {
        // Small n_intervals forces a reload inside the run.
        let mut s = VR::new(4.0, 3, Some(0)).unwrap();
        let counts = run_until_reinforcers(&mut s, 20);
        assert_eq!(counts.len(), 20);
        assert!(counts.iter().all(|&c| c >= 1));
    }

    #[test]
    fn vr_non_response_step_does_not_reinforce() {
        let mut s = VR::new(5.0, 12, Some(0)).unwrap();
        for t in [0.1f64, 0.2, 0.3] {
            let o = s.step(t, None).unwrap();
            assert!(!o.reinforced);
        }
    }

    // --- RR --------------------------------------------------------------

    #[test]
    fn rr_invalid_probability_zero() {
        assert!(matches!(
            RR::new(0.0, Some(0)),
            Err(ContingencyError::Config(_))
        ));
    }

    #[test]
    fn rr_invalid_probability_negative() {
        assert!(matches!(
            RR::new(-0.1, Some(0)),
            Err(ContingencyError::Config(_))
        ));
    }

    #[test]
    fn rr_invalid_probability_gt_one() {
        assert!(matches!(
            RR::new(1.5, Some(0)),
            Err(ContingencyError::Config(_))
        ));
    }

    #[test]
    fn rr_probability_one_reinforces_every_response() {
        let mut s = RR::new(1.0, Some(0)).unwrap();
        for i in 1..=20 {
            let o = respond(&mut s, i as f64);
            assert!(o.reinforced);
            let r = o.reinforcer.expect("reinforcer");
            assert_eq!(r.time, i as f64);
        }
    }

    #[test]
    fn rr_monte_carlo_convergence() {
        let p = 0.1;
        let trials = 10_000usize;
        let mut s = RR::new(p, Some(42)).unwrap();
        let mut hits = 0usize;
        for i in 1..=trials {
            let o = respond(&mut s, i as f64);
            if o.reinforced {
                hits += 1;
            }
        }
        // Expect ~1000 ± 5% tolerance.
        let expected = p * trials as f64;
        let diff = (hits as f64 - expected).abs();
        assert!(
            diff < 100.0,
            "Monte Carlo |{hits} - {expected}| = {diff} exceeds 100 (5% of 2000)"
        );
    }

    #[test]
    fn rr_deterministic_under_seeded_rng() {
        let mut a = RR::new(0.4, Some(777)).unwrap();
        let mut b = RR::new(0.4, Some(777)).unwrap();
        let trace_a: Vec<bool> = (1..=200)
            .map(|i| respond(&mut a, i as f64).reinforced)
            .collect();
        let trace_b: Vec<bool> = (1..=200)
            .map(|i| respond(&mut b, i as f64).reinforced)
            .collect();
        assert_eq!(trace_a, trace_b);
    }

    #[test]
    fn rr_reset_restores_draw_sequence() {
        let mut s = RR::new(0.4, Some(777)).unwrap();
        let before: Vec<bool> = (1..=100)
            .map(|i| respond(&mut s, i as f64).reinforced)
            .collect();
        s.reset();
        let after: Vec<bool> = (1..=100)
            .map(|i| respond(&mut s, i as f64).reinforced)
            .collect();
        assert_eq!(before, after);
    }

    #[test]
    fn rr_non_response_step_does_not_reinforce() {
        let mut s = RR::new(1.0, Some(0)).unwrap();
        for t in [0.1f64, 0.2, 0.3] {
            let o = s.step(t, None).unwrap();
            assert!(!o.reinforced);
        }
    }

    #[test]
    fn rr_probability_property() {
        assert_eq!(RR::new(0.25, Some(0)).unwrap().probability(), 0.25);
    }

    // --- Shared state-error contracts -----------------------------------

    #[test]
    fn fr_rejects_non_monotonic_time() {
        let mut s = FR::new(2).unwrap();
        respond(&mut s, 5.0);
        let ev = ResponseEvent::new(4.0);
        let err = s.step(4.0, Some(&ev)).unwrap_err();
        assert!(matches!(err, ContingencyError::State(_)));
    }

    #[test]
    fn fr_allows_equal_time() {
        let mut s = FR::new(2).unwrap();
        respond(&mut s, 5.0);
        // Equal timestamps are valid (within tolerance).
        let o = respond(&mut s, 5.0);
        assert!(o.reinforced);
    }

    #[test]
    fn fr_rejects_mismatched_event_time() {
        let mut s = FR::new(2).unwrap();
        let ev = ResponseEvent::new(0.5);
        let err = s.step(1.0, Some(&ev)).unwrap_err();
        assert!(matches!(err, ContingencyError::State(_)));
    }

    #[test]
    fn fr_tolerance_accepts_tiny_event_drift() {
        let mut s = FR::new(1).unwrap();
        let ev = ResponseEvent::new(1.0 + 1e-10);
        let o = s.step(1.0, Some(&ev)).unwrap();
        assert!(o.reinforced);
    }
}

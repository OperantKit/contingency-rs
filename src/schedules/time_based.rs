//! Time-based and extinction reinforcement schedules.
//!
//! This module implements the schedules whose reinforcement rule
//! depends on elapsed time alone (or on nothing at all, in the case of
//! extinction):
//!
//! * [`FT`] — Fixed Time. A reinforcer is delivered every `interval`
//!   time units, independently of the subject's behavior.
//! * [`VT`] — Variable Time. Like [`FT`], but inter-reinforcer
//!   intervals are drawn from a Fleshler-Hoffman progression with a
//!   configured mean.
//! * [`RT`] — Random Time. Inter-reinforcer intervals are sampled from
//!   an exponential distribution (memoryless).
//! * [`EXT`] — Extinction. No response is ever reinforced.
//!
//! All three time-based schedules are **response-independent**: the
//! `event` argument to [`Schedule::step`] is ignored for the
//! reinforcement decision. The event, if supplied, is still validated
//! to have the same timestamp as `now` — consistent contract across
//! the schedule family.
//!
//! # Single-fire per step
//!
//! If [`Schedule::step`] is called with a `now` that has advanced by
//! more than one scheduled interval since the last call, **only one**
//! reinforcer is emitted. After firing, the schedule's reference time
//! is set to `now`, so subsequent steps wait a full new interval. The
//! caller is responsible for stepping frequently enough that this
//! behaviour matches the experimenter's intent.
//!
//! # References
//!
//! - Catania, A. C., & Reynolds, G. S. (1968). A quantitative analysis
//!   of the responding maintained by interval schedules of
//!   reinforcement. *Journal of the Experimental Analysis of Behavior*,
//!   11(3, Pt. 2), 327-383. <https://doi.org/10.1901/jeab.1968.11-s327>
//! - Fleshler, M., & Hoffman, H. S. (1962). A progression for
//!   generating variable-interval schedules. *Journal of the
//!   Experimental Analysis of Behavior*, 5(4), 529-530.
//!   <https://doi.org/10.1901/jeab.1962.5-529>
//! - Skinner, B. F. (1957). *Schedules of reinforcement* (with
//!   C. B. Ferster). Appleton-Century-Crofts.
//! - Zeiler, M. D. (1968). Fixed and variable schedules of
//!   response-independent reinforcement. *Journal of the Experimental
//!   Analysis of Behavior*, 11(4), 405-414.
//!   <https://doi.org/10.1901/jeab.1968.11-405>

use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};

use crate::{
    constants::TIME_TOL,
    errors::ContingencyError,
    helpers::checks::{check_event, check_time},
    helpers::fleshler_hoffman::generate_intervals,
    schedule::Schedule,
    types::{Outcome, Reinforcer, ResponseEvent},
    Result,
};

// ---------------------------------------------------------------------
// FT — Fixed Time
// ---------------------------------------------------------------------

/// Fixed-Time schedule: a reinforcer every `interval` time units.
///
/// Response-independent: `event` is validated but ignored for the
/// reinforcement decision. The first [`Schedule::step`] call anchors
/// the schedule's internal clock at `now`; the first reinforcer
/// therefore fires after one full `interval` has elapsed from that
/// first step.
///
/// Only one reinforcer is emitted per step; missed intervals are
/// **not** queued.
///
/// # References
///
/// - Skinner, B. F. (1957). *Schedules of reinforcement*.
/// - Zeiler, M. D. (1968). Fixed and variable schedules of
///   response-independent reinforcement. *JEAB*, 11(4), 405-414.
#[derive(Debug, Clone)]
pub struct FT {
    interval: f64,
    anchor: Option<f64>,
    last_now: Option<f64>,
}

impl FT {
    /// Construct an FT schedule with the given fixed interval.
    ///
    /// # Errors
    ///
    /// Returns [`ContingencyError::Config`] if `interval <= 0` or is
    /// non-finite.
    pub fn new(interval: f64) -> Result<Self> {
        if !interval.is_finite() || interval <= 0.0 {
            return Err(ContingencyError::Config(format!(
                "FT requires interval > 0, got {interval}"
            )));
        }
        Ok(Self {
            interval,
            anchor: None,
            last_now: None,
        })
    }

    /// The configured fixed interval.
    pub fn interval(&self) -> f64 {
        self.interval
    }
}

impl Schedule for FT {
    fn step(&mut self, now: f64, event: Option<&ResponseEvent>) -> Result<Outcome> {
        check_time(now, self.last_now)?;
        check_event(now, event)?;
        self.last_now = Some(now);
        match self.anchor {
            None => {
                self.anchor = Some(now);
                Ok(Outcome::empty())
            }
            Some(anchor) => {
                if now - anchor + TIME_TOL >= self.interval {
                    self.anchor = Some(now);
                    Ok(Outcome::reinforced(Reinforcer::at(now)))
                } else {
                    Ok(Outcome::empty())
                }
            }
        }
    }

    fn reset(&mut self) {
        self.anchor = None;
        self.last_now = None;
    }
}

// ---------------------------------------------------------------------
// VT — Variable Time
// ---------------------------------------------------------------------

/// Variable-Time schedule driven by a Fleshler-Hoffman progression.
///
/// A pool of `n_intervals` intervals with arithmetic mean
/// `mean_interval` is generated via
/// [`generate_intervals`][crate::helpers::fleshler_hoffman::generate_intervals].
/// The first interval becomes the current requirement. When the pool
/// is exhausted, a fresh pool is generated using a sub-seed drawn from
/// the master RNG, preserving determinism.
///
/// The first call to [`Schedule::step`] anchors the clock at `now`;
/// the first reinforcer therefore fires after one interval has elapsed
/// from that first step.
///
/// # Determinism
///
/// [`Self::reset`] rebuilds the master RNG from the original seed and
/// regenerates the opening sequence, making the post-reset trajectory
/// bit-identical to the first run (for a given seed). Cross-port
/// bit-identity with Python is *not* guaranteed — both ports use
/// different RNG engines.
///
/// # References
///
/// - Fleshler, M., & Hoffman, H. S. (1962). *JEAB*, 5(4), 529-530.
/// - Zeiler, M. D. (1968). *JEAB*, 11(4), 405-414.
#[derive(Debug, Clone)]
pub struct VT {
    mean: f64,
    n_intervals: usize,
    master_seed: Option<u64>,
    master_rng: SmallRng,
    sequence: Vec<f64>,
    cursor: usize,
    required: f64,
    anchor: Option<f64>,
    last_now: Option<f64>,
}

impl VT {
    /// Construct a VT schedule with the given mean interval.
    ///
    /// # Errors
    ///
    /// Returns [`ContingencyError::Config`] if `mean_interval <= 0` or
    /// non-finite, or if `n_intervals == 0`.
    pub fn new(mean_interval: f64, n_intervals: usize, seed: Option<u64>) -> Result<Self> {
        if !mean_interval.is_finite() || mean_interval <= 0.0 {
            return Err(ContingencyError::Config(format!(
                "VT requires mean_interval > 0, got {mean_interval}"
            )));
        }
        if n_intervals == 0 {
            return Err(ContingencyError::Config(
                "VT requires n_intervals >= 1".into(),
            ));
        }
        let master_rng = match seed {
            Some(s) => SmallRng::seed_from_u64(s),
            None => SmallRng::from_entropy(),
        };
        let mut this = Self {
            mean: mean_interval,
            n_intervals,
            master_seed: seed,
            master_rng,
            sequence: Vec::new(),
            cursor: 0,
            required: 0.0,
            anchor: None,
            last_now: None,
        };
        this.reload_sequence();
        Ok(this)
    }

    /// Configured mean interval.
    pub fn mean_interval(&self) -> f64 {
        self.mean
    }

    /// Configured number of intervals per pool.
    pub fn n_intervals(&self) -> usize {
        self.n_intervals
    }

    fn next_sub_seed(&mut self) -> u64 {
        // Use a wide draw to cover the full u64 space — the sub-seed
        // is passed through to `generate_intervals` which re-hashes
        // into a fresh SmallRng.
        self.master_rng.gen::<u64>()
    }

    fn reload_sequence(&mut self) {
        let sub_seed = self.next_sub_seed();
        self.sequence = generate_intervals(self.mean, self.n_intervals, Some(sub_seed));
        self.cursor = 0;
        self.required = self.sequence[0];
    }

    fn advance_requirement(&mut self) {
        self.cursor += 1;
        if self.cursor >= self.sequence.len() {
            self.reload_sequence();
        } else {
            self.required = self.sequence[self.cursor];
        }
    }
}

impl Schedule for VT {
    fn step(&mut self, now: f64, event: Option<&ResponseEvent>) -> Result<Outcome> {
        check_time(now, self.last_now)?;
        check_event(now, event)?;
        self.last_now = Some(now);
        match self.anchor {
            None => {
                self.anchor = Some(now);
                Ok(Outcome::empty())
            }
            Some(anchor) => {
                if now - anchor + TIME_TOL >= self.required {
                    self.anchor = Some(now);
                    self.advance_requirement();
                    Ok(Outcome::reinforced(Reinforcer::at(now)))
                } else {
                    Ok(Outcome::empty())
                }
            }
        }
    }

    fn reset(&mut self) {
        self.master_rng = match self.master_seed {
            Some(s) => SmallRng::seed_from_u64(s),
            None => SmallRng::from_entropy(),
        };
        self.anchor = None;
        self.last_now = None;
        self.reload_sequence();
    }
}

// ---------------------------------------------------------------------
// RT — Random Time
// ---------------------------------------------------------------------

/// Random-Time schedule with exponentially distributed intervals.
///
/// At any moment the probability of a reinforcer in the next
/// infinitesimal interval `dt` is `dt / mean_interval` independent of
/// elapsed time (memoryless property). RT is the time-based analogue
/// of RR.
///
/// Intervals are drawn via `-mean * (1 - r).ln()` with
/// `r = rng.gen::<f64>()`. An initial interval is drawn at
/// construction so the first reinforcer has an expected wait of
/// `mean_interval` from the first-step anchor.
///
/// [`Self::reset`] restores the RNG to the state it had at
/// construction, so the draw sequence replays bit-identically (for a
/// given seed).
///
/// # References
///
/// - Catania, A. C., & Reynolds, G. S. (1968). *JEAB*, 11(3, Pt. 2),
///   327-383.
#[derive(Debug, Clone)]
pub struct RT {
    mean: f64,
    initial_rng: SmallRng,
    current_rng: SmallRng,
    required: f64,
    anchor: Option<f64>,
    last_now: Option<f64>,
}

impl RT {
    /// Construct an RT schedule with the given exponential-distribution
    /// mean.
    ///
    /// # Errors
    ///
    /// Returns [`ContingencyError::Config`] if `mean_interval <= 0` or
    /// non-finite.
    pub fn new(mean_interval: f64, seed: Option<u64>) -> Result<Self> {
        if !mean_interval.is_finite() || mean_interval <= 0.0 {
            return Err(ContingencyError::Config(format!(
                "RT requires mean_interval > 0, got {mean_interval}"
            )));
        }
        let initial_rng = match seed {
            Some(s) => SmallRng::seed_from_u64(s),
            None => SmallRng::from_entropy(),
        };
        let mut current_rng = initial_rng.clone();
        let required = Self::draw_interval(&mut current_rng, mean_interval);
        Ok(Self {
            mean: mean_interval,
            initial_rng,
            current_rng,
            required,
            anchor: None,
            last_now: None,
        })
    }

    /// Configured mean interval.
    pub fn mean_interval(&self) -> f64 {
        self.mean
    }

    fn draw_interval(rng: &mut SmallRng, mean: f64) -> f64 {
        // r is uniform on [0, 1); `1.0 - r` is in (0, 1], so ln(..) is
        // finite and non-positive — the negation makes the result
        // non-negative.
        let r: f64 = rng.gen();
        -mean * (1.0 - r).ln()
    }
}

impl Schedule for RT {
    fn step(&mut self, now: f64, event: Option<&ResponseEvent>) -> Result<Outcome> {
        check_time(now, self.last_now)?;
        check_event(now, event)?;
        self.last_now = Some(now);
        match self.anchor {
            None => {
                self.anchor = Some(now);
                Ok(Outcome::empty())
            }
            Some(anchor) => {
                if now - anchor + TIME_TOL >= self.required {
                    self.anchor = Some(now);
                    self.required = Self::draw_interval(&mut self.current_rng, self.mean);
                    Ok(Outcome::reinforced(Reinforcer::at(now)))
                } else {
                    Ok(Outcome::empty())
                }
            }
        }
    }

    fn reset(&mut self) {
        self.current_rng = self.initial_rng.clone();
        self.required = Self::draw_interval(&mut self.current_rng, self.mean);
        self.anchor = None;
        self.last_now = None;
    }
}

// ---------------------------------------------------------------------
// EXT — Extinction
// ---------------------------------------------------------------------

/// Extinction: no response is ever reinforced.
///
/// EXT is essentially stateless — [`Schedule::step`] always returns an
/// un-reinforced [`Outcome`], regardless of `now`, `event`, or how
/// often it is called. The only dynamic state is `last_now`, retained
/// solely to enforce the monotonic-time contract shared by all
/// schedules.
///
/// # References
///
/// - Skinner, B. F. (1957). *Schedules of reinforcement*.
#[derive(Debug, Default, Clone)]
pub struct EXT {
    last_now: Option<f64>,
}

impl EXT {
    /// Construct a fresh EXT schedule.
    pub fn new() -> Self {
        Self::default()
    }
}

impl Schedule for EXT {
    fn step(&mut self, now: f64, event: Option<&ResponseEvent>) -> Result<Outcome> {
        check_time(now, self.last_now)?;
        check_event(now, event)?;
        self.last_now = Some(now);
        Ok(Outcome::empty())
    }

    fn reset(&mut self) {
        self.last_now = None;
    }
}

// ---------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- FT -----------------------------------------------------------

    #[test]
    fn ft_rejects_non_positive_interval() {
        assert!(FT::new(0.0).is_err());
        assert!(FT::new(-1.0).is_err());
        assert!(FT::new(f64::NAN).is_err());
        assert!(FT::new(f64::INFINITY).is_err());
    }

    #[test]
    fn ft_first_step_anchors_no_reinforce() {
        let mut s = FT::new(5.0).unwrap();
        let o = s.step(0.0, None).unwrap();
        assert!(!o.reinforced);
    }

    #[test]
    fn ft_does_not_reinforce_before_interval() {
        let mut s = FT::new(5.0).unwrap();
        assert!(!s.step(0.0, None).unwrap().reinforced);
        for &t in &[1.0, 2.0, 3.0, 4.999] {
            assert!(!s.step(t, None).unwrap().reinforced, "fired early at t={t}");
        }
    }

    #[test]
    fn ft_reinforces_at_interval_boundary() {
        let mut s = FT::new(5.0).unwrap();
        s.step(0.0, None).unwrap();
        // Boundary within TIME_TOL should still fire.
        let o = s.step(5.0 - 1e-12, None).unwrap();
        assert!(o.reinforced);
        let r = o.reinforcer.as_ref().unwrap();
        assert!((r.time - (5.0 - 1e-12)).abs() < 1e-9);
        assert_eq!(r.label, "SR+");
        assert_eq!(r.magnitude, 1.0);
    }

    #[test]
    fn ft_reinforces_at_exactly_interval() {
        let mut s = FT::new(5.0).unwrap();
        s.step(0.0, None).unwrap();
        let o = s.step(5.0, None).unwrap();
        assert!(o.reinforced);
        assert_eq!(o.reinforcer.as_ref().unwrap().time, 5.0);
    }

    #[test]
    fn ft_single_fire_per_step_even_when_late() {
        let mut s = FT::new(5.0).unwrap();
        s.step(0.0, None).unwrap();
        // Skip ahead by >3 intervals. Only ONE reinforcer should fire.
        let o = s.step(20.0, None).unwrap();
        assert!(o.reinforced);
        assert_eq!(o.reinforcer.as_ref().unwrap().time, 20.0);
        // Next reinforcer requires a full fresh interval from t=20.
        assert!(!s.step(24.0, None).unwrap().reinforced);
        assert!(s.step(25.0, None).unwrap().reinforced);
    }

    #[test]
    fn ft_periodic_firing() {
        let mut s = FT::new(5.0).unwrap();
        s.step(0.0, None).unwrap();
        let mut fires = 0;
        let mut last_fire = 0.0;
        for i in 1..=20 {
            let t = i as f64;
            let o = s.step(t, None).unwrap();
            if o.reinforced {
                fires += 1;
                assert!((t - last_fire - 5.0).abs() < 1e-9 || last_fire == 0.0 && t == 5.0);
                last_fire = t;
            }
        }
        // fires at t = 5, 10, 15, 20 → 4 reinforcers
        assert_eq!(fires, 4);
    }

    #[test]
    fn ft_response_does_not_affect_timing() {
        let mut s = FT::new(5.0).unwrap();
        s.step(0.0, None).unwrap();
        // Responses at every tick — should make no difference.
        for i in 1..5 {
            let t = i as f64;
            let ev = ResponseEvent::new(t);
            assert!(!s.step(t, Some(&ev)).unwrap().reinforced);
        }
        let ev = ResponseEvent::new(5.0);
        assert!(s.step(5.0, Some(&ev)).unwrap().reinforced);
    }

    #[test]
    fn ft_reset_clears_anchor() {
        let mut s = FT::new(5.0).unwrap();
        s.step(0.0, None).unwrap();
        s.step(5.0, None).unwrap();
        s.reset();
        // After reset the next step anchors afresh; no reinforcer.
        assert!(!s.step(100.0, None).unwrap().reinforced);
        assert!(!s.step(104.0, None).unwrap().reinforced);
        assert!(s.step(105.0, None).unwrap().reinforced);
    }

    // --- VT -----------------------------------------------------------

    #[test]
    fn vt_rejects_invalid_params() {
        assert!(VT::new(0.0, 12, Some(0)).is_err());
        assert!(VT::new(-1.0, 12, Some(0)).is_err());
        assert!(VT::new(30.0, 0, Some(0)).is_err());
        assert!(VT::new(f64::NAN, 12, Some(0)).is_err());
    }

    #[test]
    fn vt_constructs_with_seed() {
        let s = VT::new(30.0, 12, Some(42)).unwrap();
        assert_eq!(s.mean_interval(), 30.0);
        assert_eq!(s.n_intervals(), 12);
    }

    #[test]
    fn vt_first_step_does_not_reinforce() {
        let mut s = VT::new(30.0, 12, Some(42)).unwrap();
        assert!(!s.step(0.0, None).unwrap().reinforced);
    }

    #[test]
    fn vt_mean_rate_matches_parameter() {
        // Simulate tightly-spaced steps and count reinforcers. The
        // empirical mean inter-reinforcement interval should be close
        // to `mean_interval`.
        let mean = 5.0;
        let mut s = VT::new(mean, 12, Some(7)).unwrap();
        let dt = 0.01;
        let horizon = 5000.0; // → ~1000 expected reinforcers
        let mut t = 0.0;
        let mut fires = 0u64;
        while t < horizon {
            let o = s.step(t, None).unwrap();
            if o.reinforced {
                fires += 1;
            }
            t += dt;
        }
        let observed_mean = horizon / fires as f64;
        // 5% tolerance band for ~1000 samples, mean 5.0.
        assert!(
            (observed_mean - mean).abs() / mean < 0.05,
            "observed={observed_mean}, expected={mean}, fires={fires}"
        );
    }

    #[test]
    fn vt_deterministic_under_seed() {
        let run = |seed: u64| -> Vec<f64> {
            let mut s = VT::new(5.0, 12, Some(seed)).unwrap();
            let mut out = Vec::new();
            let dt = 0.01;
            let mut t = 0.0;
            while t < 200.0 {
                if s.step(t, None).unwrap().reinforced {
                    out.push(t);
                }
                t += dt;
            }
            out
        };
        let a = run(123);
        let b = run(123);
        assert_eq!(a, b);
        let c = run(124);
        assert_ne!(a, c);
    }

    #[test]
    fn vt_reset_replays_trajectory() {
        let mut s = VT::new(5.0, 12, Some(99)).unwrap();
        let capture = |s: &mut VT| -> Vec<f64> {
            let dt = 0.01;
            let mut t = 0.0;
            let mut out = Vec::new();
            while t < 200.0 {
                if s.step(t, None).unwrap().reinforced {
                    out.push(t);
                }
                t += dt;
            }
            out
        };
        let first = capture(&mut s);
        s.reset();
        let second = capture(&mut s);
        assert_eq!(first, second);
        assert!(!first.is_empty());
    }

    // --- RT -----------------------------------------------------------

    #[test]
    fn rt_rejects_non_positive_mean() {
        assert!(RT::new(0.0, Some(0)).is_err());
        assert!(RT::new(-1.0, Some(0)).is_err());
        assert!(RT::new(f64::NAN, Some(0)).is_err());
    }

    #[test]
    fn rt_first_step_does_not_reinforce() {
        let mut s = RT::new(2.0, Some(1)).unwrap();
        assert!(!s.step(0.0, None).unwrap().reinforced);
    }

    #[test]
    fn rt_iri_distribution_is_approximately_exponential() {
        // Fine time grid, long horizon. The empirical mean IRI should
        // be within a few percent of the parameter.
        let mean = 2.0;
        let mut s = RT::new(mean, Some(17)).unwrap();
        let dt = 0.01;
        let horizon = 20_000.0; // ~10,000 expected reinforcers
        let mut t = 0.0;
        let mut fires = 0u64;
        let mut last_fire: Option<f64> = None;
        let mut iris: Vec<f64> = Vec::new();
        while t < horizon {
            let o = s.step(t, None).unwrap();
            if o.reinforced {
                fires += 1;
                if let Some(lf) = last_fire {
                    iris.push(t - lf);
                }
                last_fire = Some(t);
            }
            t += dt;
        }
        assert!(fires > 5_000, "too few reinforcers: {fires}");
        let obs_mean: f64 = iris.iter().copied().sum::<f64>() / iris.len() as f64;
        assert!(
            (obs_mean - mean).abs() / mean < 0.05,
            "obs mean IRI = {obs_mean}, target = {mean}"
        );
        // For exponential(λ=1/mean), variance = mean^2. Check that the
        // empirical variance is of the right order (loose bound — the
        // discretisation dt biases variance slightly downward).
        let var: f64 = iris.iter().map(|x| (x - obs_mean).powi(2)).sum::<f64>() / iris.len() as f64;
        assert!(
            var > 0.5 * mean * mean && var < 2.0 * mean * mean,
            "var = {var}, expected ~{}",
            mean * mean
        );
    }

    #[test]
    fn rt_deterministic_under_seed() {
        let run = |seed: u64| -> Vec<f64> {
            let mut s = RT::new(2.0, Some(seed)).unwrap();
            let mut out = Vec::new();
            let dt = 0.01;
            let mut t = 0.0;
            while t < 100.0 {
                if s.step(t, None).unwrap().reinforced {
                    out.push(t);
                }
                t += dt;
            }
            out
        };
        let a = run(5);
        let b = run(5);
        assert_eq!(a, b);
        let c = run(6);
        assert_ne!(a, c);
    }

    #[test]
    fn rt_reset_replays_trajectory() {
        let mut s = RT::new(2.0, Some(11)).unwrap();
        let capture = |s: &mut RT| -> Vec<f64> {
            let dt = 0.01;
            let mut t = 0.0;
            let mut out = Vec::new();
            while t < 100.0 {
                if s.step(t, None).unwrap().reinforced {
                    out.push(t);
                }
                t += dt;
            }
            out
        };
        let first = capture(&mut s);
        s.reset();
        let second = capture(&mut s);
        assert_eq!(first, second);
        assert!(!first.is_empty());
    }

    // --- EXT ----------------------------------------------------------

    #[test]
    fn ext_never_reinforces_without_events() {
        let mut s = EXT::new();
        for i in 0..100 {
            let t = i as f64 * 0.1;
            assert!(!s.step(t, None).unwrap().reinforced);
        }
    }

    #[test]
    fn ext_never_reinforces_with_events() {
        let mut s = EXT::new();
        for i in 0..100 {
            let t = i as f64 * 0.1;
            let ev = ResponseEvent::new(t);
            assert!(!s.step(t, Some(&ev)).unwrap().reinforced);
        }
    }

    #[test]
    fn ext_reset_clears_last_now() {
        let mut s = EXT::new();
        s.step(10.0, None).unwrap();
        // Non-monotonic before reset → error.
        assert!(matches!(s.step(5.0, None), Err(ContingencyError::State(_))));
        s.reset();
        // Post-reset, t=5.0 is fine.
        assert!(s.step(5.0, None).is_ok());
    }

    #[test]
    fn ext_non_monotonic_time_error() {
        let mut s = EXT::new();
        s.step(1.0, None).unwrap();
        assert!(matches!(s.step(0.5, None), Err(ContingencyError::State(_))));
    }

    #[test]
    fn ext_event_mismatch_error() {
        let mut s = EXT::new();
        let ev = ResponseEvent::new(1.5);
        assert!(matches!(
            s.step(1.0, Some(&ev)),
            Err(ContingencyError::State(_))
        ));
    }

    // --- Shared contract checks ---------------------------------------

    #[test]
    fn ft_non_monotonic_time_rejected() {
        let mut s = FT::new(5.0).unwrap();
        s.step(2.0, None).unwrap();
        assert!(matches!(s.step(1.0, None), Err(ContingencyError::State(_))));
    }

    #[test]
    fn ft_event_time_must_match_now() {
        let mut s = FT::new(5.0).unwrap();
        let ev = ResponseEvent::new(0.5);
        assert!(matches!(
            s.step(1.0, Some(&ev)),
            Err(ContingencyError::State(_))
        ));
    }

    #[test]
    fn vt_non_monotonic_time_rejected() {
        let mut s = VT::new(5.0, 12, Some(1)).unwrap();
        s.step(2.0, None).unwrap();
        assert!(matches!(s.step(1.0, None), Err(ContingencyError::State(_))));
    }

    #[test]
    fn vt_event_time_must_match_now() {
        let mut s = VT::new(5.0, 12, Some(1)).unwrap();
        let ev = ResponseEvent::new(0.5);
        assert!(matches!(
            s.step(1.0, Some(&ev)),
            Err(ContingencyError::State(_))
        ));
    }

    #[test]
    fn rt_non_monotonic_time_rejected() {
        let mut s = RT::new(2.0, Some(1)).unwrap();
        s.step(2.0, None).unwrap();
        assert!(matches!(s.step(1.0, None), Err(ContingencyError::State(_))));
    }

    #[test]
    fn rt_event_time_must_match_now() {
        let mut s = RT::new(2.0, Some(1)).unwrap();
        let ev = ResponseEvent::new(0.5);
        assert!(matches!(
            s.step(1.0, Some(&ev)),
            Err(ContingencyError::State(_))
        ));
    }

    // --- Outcome shape ------------------------------------------------

    #[test]
    fn ft_reinforcer_is_sr_plus_magnitude_one() {
        let mut s = FT::new(1.0).unwrap();
        s.step(0.0, None).unwrap();
        let o = s.step(1.0, None).unwrap();
        let r = o.reinforcer.unwrap();
        assert_eq!(r.label, "SR+");
        assert_eq!(r.magnitude, 1.0);
        assert_eq!(r.time, 1.0);
    }
}

//! Interval-family reinforcement schedules: FI, VI, RI, LimitedHold.
//!
//! In interval schedules a reinforcer becomes available only after a
//! specified amount of time has elapsed since the last reinforcement.
//! The subject must emit at least one response after the interval
//! elapses to actually obtain the reinforcer — this distinguishes
//! interval schedules from *time* schedules (FT/VT/RT), where the
//! reinforcer is delivered automatically on interval expiry regardless
//! of behaviour.
//!
//! Port of `contingency.schedules.interval` in Python. The condition
//! preserved from the Swift original is
//! `numOfResponses > previousNumOfResponses AND elapsed >= interval`:
//! reinforcement is produced only on the *response* that is both
//! emitted at `now` *and* past the currently armed target time. A
//! plain time tick with `event = None` never produces reinforcement.
//!
//! # LimitedHold wrapping
//!
//! Classical FI/VI/RI have no "missed opportunity" notion: any
//! response after the interval qualifies, no matter how late. The
//! [`LimitedHold`] decorator adds a bounded availability window: if
//! no qualifying response is emitted within `hold` time units of the
//! interval elapsing, the opportunity is withdrawn and the inner
//! schedule is re-armed from scratch (a new interval is drawn without
//! any reinforcer being delivered).
//!
//! The wrapping is expressed through the [`ArmableSchedule`] trait
//! (see `schedule.rs`) which FI, VI, and RI implement. `LimitedHold`
//! itself is only `Schedule`, not `ArmableSchedule`, so it cannot be
//! double-wrapped.
//!
//! # References
//!
//! - Ferster, C. B., & Skinner, B. F. (1957). *Schedules of
//!   reinforcement*. Appleton-Century-Crofts.
//! - Fleshler, M., & Hoffman, H. S. (1962). A progression for
//!   generating variable-interval schedules. *Journal of the
//!   Experimental Analysis of Behavior*, 5(4), 529-530.
//!   <https://doi.org/10.1901/jeab.1962.5-529>
//! - Catania, A. C., & Reynolds, G. S. (1968). A quantitative
//!   analysis of the responding maintained by interval schedules of
//!   reinforcement. *Journal of the Experimental Analysis of
//!   Behavior*, 11(3 Pt 2), 327-383.
//!   <https://doi.org/10.1901/jeab.1968.11-s327>
//! - Nevin, J. A. (1974). Response strength in multiple schedules.
//!   *Journal of the Experimental Analysis of Behavior*, 21(3),
//!   389-408. <https://doi.org/10.1901/jeab.1974.21-389>

use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};

use crate::constants::TIME_TOL;
use crate::errors::ContingencyError;
use crate::helpers::checks::{check_event, check_time};
use crate::helpers::fleshler_hoffman::generate_intervals;
use crate::schedule::{ArmableSchedule, Schedule};
use crate::types::{Outcome, Reinforcer, ResponseEvent};
use crate::Result;

// ---------------------------------------------------------------------------
// FI — Fixed Interval
// ---------------------------------------------------------------------------

/// Fixed-Interval schedule.
///
/// The first response emitted at or after `interval` time units since
/// the last reinforcement (or since construction / the last call to
/// [`Schedule::reset`]) produces a reinforcer. Responses during the
/// interval do not reset the timer and do not produce reinforcement.
///
/// # Construction
///
/// ```
/// use contingency::schedules::FI;
/// let _fi = FI::new(10.0).unwrap();
/// ```
#[derive(Debug, Clone)]
pub struct FI {
    interval: f64,
    arm_time: f64,
    last_now: Option<f64>,
}

impl FI {
    /// Construct a fixed-interval schedule with the given `interval`.
    ///
    /// # Errors
    ///
    /// Returns [`ContingencyError::Config`] when `interval <= 0`.
    pub fn new(interval: f64) -> Result<Self> {
        if interval.is_nan() || interval <= 0.0 {
            return Err(ContingencyError::Config(format!(
                "interval must be > 0, got {interval}"
            )));
        }
        Ok(Self {
            interval,
            arm_time: interval,
            last_now: None,
        })
    }
}

impl Schedule for FI {
    fn step(&mut self, now: f64, event: Option<&ResponseEvent>) -> Result<Outcome> {
        check_time(now, self.last_now)?;
        check_event(now, event)?;
        self.last_now = Some(now);

        if event.is_none() {
            return Ok(Outcome::empty());
        }

        if now + TIME_TOL >= self.arm_time {
            let reinforcer = Reinforcer::at(now);
            self.arm_time = now + self.interval;
            return Ok(Outcome::reinforced(reinforcer));
        }

        Ok(Outcome::empty())
    }

    fn reset(&mut self) {
        self.arm_time = self.interval;
        self.last_now = None;
    }
}

impl ArmableSchedule for FI {
    fn arm_time(&self) -> f64 {
        self.arm_time
    }

    fn withdraw_and_rearm(&mut self, now: f64) {
        self.arm_time = now + self.interval;
    }
}

// ---------------------------------------------------------------------------
// VI — Variable Interval
// ---------------------------------------------------------------------------

/// Variable-Interval schedule with a Fleshler-Hoffman sequence.
///
/// Intervals are drawn from a shuffled Fleshler-Hoffman progression
/// with arithmetic mean equal to `mean_interval`. When the sequence
/// is exhausted a fresh sequence is generated deterministically from
/// the internal master RNG so that a single seed pins the whole
/// trajectory of the schedule.
#[derive(Debug, Clone)]
pub struct VI {
    mean_interval: f64,
    n_intervals: usize,
    master_seed: Option<u64>,
    master_rng: SmallRng,
    sequence: Vec<f64>,
    cursor: usize,
    arm_time: f64,
    last_now: Option<f64>,
}

impl VI {
    /// Construct a variable-interval schedule with the given
    /// `mean_interval`, Fleshler-Hoffman cycle size `n_intervals`,
    /// and optional RNG `seed`.
    ///
    /// # Errors
    ///
    /// Returns [`ContingencyError::Config`] when `mean_interval <= 0`
    /// or `n_intervals == 0`.
    pub fn new(mean_interval: f64, n_intervals: usize, seed: Option<u64>) -> Result<Self> {
        if mean_interval.is_nan() || mean_interval <= 0.0 {
            return Err(ContingencyError::Config(format!(
                "mean_interval must be > 0, got {mean_interval}"
            )));
        }
        if n_intervals == 0 {
            return Err(ContingencyError::Config(format!(
                "n_intervals must be >= 1, got {n_intervals}"
            )));
        }
        let mut master_rng = make_master_rng(seed);
        let sequence = Self::generate(mean_interval, n_intervals, &mut master_rng);
        let arm_time = sequence[0];
        Ok(Self {
            mean_interval,
            n_intervals,
            master_seed: seed,
            master_rng,
            sequence,
            cursor: 1,
            arm_time,
            last_now: None,
        })
    }

    fn generate(mean_interval: f64, n_intervals: usize, master_rng: &mut SmallRng) -> Vec<f64> {
        // Derive a per-cycle sub-seed from the master RNG so the
        // full trajectory is deterministic given the constructor
        // seed. Mirrors the Python `random.Random.getrandbits(64)`
        // approach.
        let subseed: u64 = master_rng.gen();
        generate_intervals(mean_interval, n_intervals, Some(subseed))
    }

    fn next_interval(&mut self) -> f64 {
        if self.cursor >= self.sequence.len() {
            self.sequence =
                Self::generate(self.mean_interval, self.n_intervals, &mut self.master_rng);
            self.cursor = 0;
        }
        let value = self.sequence[self.cursor];
        self.cursor += 1;
        value
    }
}

impl Schedule for VI {
    fn step(&mut self, now: f64, event: Option<&ResponseEvent>) -> Result<Outcome> {
        check_time(now, self.last_now)?;
        check_event(now, event)?;
        self.last_now = Some(now);

        if event.is_none() {
            return Ok(Outcome::empty());
        }

        if now + TIME_TOL >= self.arm_time {
            let reinforcer = Reinforcer::at(now);
            let next = self.next_interval();
            self.arm_time = now + next;
            return Ok(Outcome::reinforced(reinforcer));
        }

        Ok(Outcome::empty())
    }

    fn reset(&mut self) {
        self.master_rng = make_master_rng(self.master_seed);
        self.sequence = Self::generate(self.mean_interval, self.n_intervals, &mut self.master_rng);
        self.cursor = 1;
        self.arm_time = self.sequence[0];
        self.last_now = None;
    }
}

impl ArmableSchedule for VI {
    fn arm_time(&self) -> f64 {
        self.arm_time
    }

    fn withdraw_and_rearm(&mut self, now: f64) {
        let next = self.next_interval();
        self.arm_time = now + next;
    }
}

// ---------------------------------------------------------------------------
// RI — Random Interval
// ---------------------------------------------------------------------------

/// Random-Interval schedule with exponentially distributed intervals.
///
/// Each interval is drawn independently from `Exp(1 / mean_interval)`,
/// giving a memoryless reinforcement process with constant hazard
/// rate `1 / mean_interval`.
///
/// The RNG's state at construction is captured (via clone) so that
/// [`Schedule::reset`] restores bit-identical behaviour of subsequent
/// draws.
#[derive(Debug, Clone)]
pub struct RI {
    mean_interval: f64,
    initial_rng: SmallRng,
    current_rng: SmallRng,
    arm_time: f64,
    last_now: Option<f64>,
}

impl RI {
    /// Construct a random-interval schedule with the given
    /// `mean_interval` and optional RNG `seed`.
    ///
    /// # Errors
    ///
    /// Returns [`ContingencyError::Config`] when `mean_interval <= 0`.
    pub fn new(mean_interval: f64, seed: Option<u64>) -> Result<Self> {
        if mean_interval.is_nan() || mean_interval <= 0.0 {
            return Err(ContingencyError::Config(format!(
                "mean_interval must be > 0, got {mean_interval}"
            )));
        }
        let rng = make_master_rng(seed);
        let initial_rng = rng.clone();
        let mut current_rng = rng;
        let arm_time = draw_exp(&mut current_rng, mean_interval);
        Ok(Self {
            mean_interval,
            initial_rng,
            current_rng,
            arm_time,
            last_now: None,
        })
    }
}

impl Schedule for RI {
    fn step(&mut self, now: f64, event: Option<&ResponseEvent>) -> Result<Outcome> {
        check_time(now, self.last_now)?;
        check_event(now, event)?;
        self.last_now = Some(now);

        if event.is_none() {
            return Ok(Outcome::empty());
        }

        if now + TIME_TOL >= self.arm_time {
            let reinforcer = Reinforcer::at(now);
            let next = draw_exp(&mut self.current_rng, self.mean_interval);
            self.arm_time = now + next;
            return Ok(Outcome::reinforced(reinforcer));
        }

        Ok(Outcome::empty())
    }

    fn reset(&mut self) {
        self.current_rng = self.initial_rng.clone();
        self.arm_time = draw_exp(&mut self.current_rng, self.mean_interval);
        self.last_now = None;
    }
}

impl ArmableSchedule for RI {
    fn arm_time(&self) -> f64 {
        self.arm_time
    }

    fn withdraw_and_rearm(&mut self, now: f64) {
        let next = draw_exp(&mut self.current_rng, self.mean_interval);
        self.arm_time = now + next;
    }
}

// ---------------------------------------------------------------------------
// LimitedHold — decorator
// ---------------------------------------------------------------------------

/// Limited-Hold wrapper around an interval-family schedule.
///
/// Once the inner schedule's interval elapses (i.e. `now >=
/// inner.arm_time()`) a reinforcement opportunity becomes available.
/// The opportunity remains available for `hold` time units. If the
/// subject emits a response within that window, the inner schedule
/// delivers its reinforcer normally. If the window closes before any
/// response, the opportunity is *withdrawn* — the inner schedule is
/// re-armed at `now` with a freshly sampled interval, and no
/// reinforcer is produced.
///
/// `LimitedHold` implements only [`Schedule`], not
/// [`ArmableSchedule`], so nesting `LimitedHold<LimitedHold<...>>`
/// will not compile.
#[derive(Debug, Clone)]
pub struct LimitedHold<S: ArmableSchedule> {
    inner: S,
    hold: f64,
    last_now: Option<f64>,
}

impl<S: ArmableSchedule> LimitedHold<S> {
    /// Construct a limited-hold decorator wrapping `inner` with the
    /// given `hold` window.
    ///
    /// # Errors
    ///
    /// Returns [`ContingencyError::Config`] when `hold <= 0`.
    pub fn new(inner: S, hold: f64) -> Result<Self> {
        if hold.is_nan() || hold <= 0.0 {
            return Err(ContingencyError::Config(format!(
                "hold must be > 0, got {hold}"
            )));
        }
        Ok(Self {
            inner,
            hold,
            last_now: None,
        })
    }

    fn expire_if_needed(&mut self, now: f64) {
        let arm_time = self.inner.arm_time();
        if now > arm_time + self.hold + TIME_TOL {
            self.inner.withdraw_and_rearm(now);
        }
    }
}

impl<S: ArmableSchedule> Schedule for LimitedHold<S> {
    fn step(&mut self, now: f64, event: Option<&ResponseEvent>) -> Result<Outcome> {
        check_time(now, self.last_now)?;
        check_event(now, event)?;
        self.last_now = Some(now);

        // On any step (tick or response) first check whether the
        // previously-armed opportunity has expired; if so, withdraw
        // it and re-arm before evaluating the current event.
        self.expire_if_needed(now);

        if event.is_none() {
            return Ok(Outcome::empty());
        }

        let arm_time = self.inner.arm_time();
        if now + TIME_TOL >= arm_time && now <= arm_time + self.hold + TIME_TOL {
            // Delegate to the inner schedule so it updates its own
            // state (arm_time resample, sequence cursor, etc.) and
            // emits the canonical Outcome.
            return self.inner.step(now, event);
        }

        Ok(Outcome::empty())
    }

    fn reset(&mut self) {
        self.inner.reset();
        self.last_now = None;
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn make_master_rng(seed: Option<u64>) -> SmallRng {
    match seed {
        Some(s) => SmallRng::seed_from_u64(s),
        None => SmallRng::from_entropy(),
    }
}

/// Exponential draw with mean `mean` using the inverse-CDF method.
///
/// `r = rng.gen::<f64>()` is uniform on `[0, 1)`, so `1.0 - r` is in
/// `(0, 1]` and `ln(1.0 - r)` is finite non-positive. Result:
/// `-mean * ln(1.0 - r)` is `>= 0`, matching `Exp(1 / mean)`.
fn draw_exp(rng: &mut SmallRng, mean: f64) -> f64 {
    let r: f64 = rng.gen();
    -mean * (1.0 - r).ln()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ----- FI --------------------------------------------------------------

    #[test]
    fn fi_rejects_non_positive_interval() {
        assert!(matches!(
            FI::new(0.0).unwrap_err(),
            ContingencyError::Config(_)
        ));
        assert!(matches!(
            FI::new(-1.0).unwrap_err(),
            ContingencyError::Config(_)
        ));
    }

    #[test]
    fn fi_does_not_reinforce_before_interval() {
        let mut fi = FI::new(10.0).unwrap();
        for &t in &[0.0, 5.0, 9.0, 9.999] {
            let ev = ResponseEvent::new(t);
            let out = fi.step(t, Some(&ev)).unwrap();
            assert!(!out.reinforced, "unexpected reinforcement at t={t}");
        }
    }

    #[test]
    fn fi_reinforces_first_response_after_interval() {
        let mut fi = FI::new(10.0).unwrap();
        // Response during the interval is consumed but not reinforced.
        let e1 = ResponseEvent::new(5.0);
        assert!(!fi.step(5.0, Some(&e1)).unwrap().reinforced);
        // First response at/after t = interval produces SR+.
        let e2 = ResponseEvent::new(10.0);
        let out = fi.step(10.0, Some(&e2)).unwrap();
        assert!(out.reinforced);
        let r = out.reinforcer.unwrap();
        assert_eq!(r.time, 10.0);
        assert_eq!(r.label, "SR+");
    }

    #[test]
    fn fi_response_during_interval_does_not_reset_timer() {
        let mut fi = FI::new(10.0).unwrap();
        // Lots of responding in the first interval; none reinforce.
        for &t in &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0] {
            let ev = ResponseEvent::new(t);
            assert!(!fi.step(t, Some(&ev)).unwrap().reinforced);
        }
        // The very next post-interval response reinforces — proving
        // the early responses did not push the arm_time forward.
        let e = ResponseEvent::new(10.0);
        assert!(fi.step(10.0, Some(&e)).unwrap().reinforced);
    }

    #[test]
    fn fi_arm_time_resets_after_reinforcement() {
        let mut fi = FI::new(10.0).unwrap();
        let e1 = ResponseEvent::new(10.0);
        assert!(fi.step(10.0, Some(&e1)).unwrap().reinforced);
        // Immediately after, the schedule is NOT ready.
        let e2 = ResponseEvent::new(15.0);
        assert!(!fi.step(15.0, Some(&e2)).unwrap().reinforced);
        // At t = 20 (10 after the last reinforcement) it IS ready.
        let e3 = ResponseEvent::new(20.0);
        assert!(fi.step(20.0, Some(&e3)).unwrap().reinforced);
    }

    #[test]
    fn fi_tick_only_never_reinforces() {
        let mut fi = FI::new(5.0).unwrap();
        // Well past the interval, but no event emitted.
        for &t in &[1.0, 5.0, 10.0, 100.0] {
            let out = fi.step(t, None).unwrap();
            assert!(!out.reinforced);
        }
    }

    #[test]
    fn fi_rejects_non_monotonic_time() {
        let mut fi = FI::new(10.0).unwrap();
        fi.step(5.0, None).unwrap();
        let err = fi.step(4.0, None).unwrap_err();
        assert!(matches!(err, ContingencyError::State(_)));
    }

    #[test]
    fn fi_rejects_event_now_mismatch() {
        let mut fi = FI::new(10.0).unwrap();
        let bad = ResponseEvent::new(5.0);
        let err = fi.step(4.0, Some(&bad)).unwrap_err();
        assert!(matches!(err, ContingencyError::State(_)));
    }

    #[test]
    fn fi_reset_restores_initial_arm_time() {
        let mut fi = FI::new(10.0).unwrap();
        let e1 = ResponseEvent::new(10.0);
        fi.step(10.0, Some(&e1)).unwrap();
        fi.reset();
        // After reset, a response at t=5 must not reinforce, but
        // t=10 should (arm_time is back at 10 relative to clock 0).
        let e2 = ResponseEvent::new(5.0);
        assert!(!fi.step(5.0, Some(&e2)).unwrap().reinforced);
        let e3 = ResponseEvent::new(10.0);
        assert!(fi.step(10.0, Some(&e3)).unwrap().reinforced);
    }

    // ----- VI --------------------------------------------------------------

    #[test]
    fn vi_rejects_non_positive_mean() {
        assert!(matches!(
            VI::new(0.0, 12, Some(1)).unwrap_err(),
            ContingencyError::Config(_)
        ));
    }

    #[test]
    fn vi_rejects_zero_n_intervals() {
        assert!(matches!(
            VI::new(30.0, 0, Some(1)).unwrap_err(),
            ContingencyError::Config(_)
        ));
    }

    #[test]
    fn vi_mean_rate_approx_reciprocal_of_mean() {
        // Drive VI(10, n=12) with a response every 0.01 time units so
        // the response rate vastly exceeds the reinforcement rate,
        // making reinforcement rate ≈ 1/mean_interval.
        let mut vi = VI::new(10.0, 12, Some(7)).unwrap();
        let dt = 0.01_f64;
        let target_reinforcements = 1000;
        let mut reinforced = 0usize;
        let mut t = 0.0_f64;
        let mut last_rf_time = 0.0_f64;
        // Safety cap: mean_interval * target * 10 generous bound.
        let t_cap = 10.0 * target_reinforcements as f64 * 10.0;
        while reinforced < target_reinforcements && t < t_cap {
            t += dt;
            let ev = ResponseEvent::new(t);
            let out = vi.step(t, Some(&ev)).unwrap();
            if out.reinforced {
                reinforced += 1;
                last_rf_time = t;
            }
        }
        assert_eq!(reinforced, target_reinforcements);
        let avg_interval = last_rf_time / target_reinforcements as f64;
        // Each empirical IRI should be close to mean_interval; allow
        // generous tolerance as FH is mean-preserving per cycle and
        // 1000 reinforcements cover many cycles.
        assert!(
            (avg_interval - 10.0).abs() < 1.5,
            "avg_interval={avg_interval}"
        );
    }

    #[test]
    fn vi_deterministic_under_seed() {
        let a = collect_reinforcement_times_vi(VI::new(10.0, 12, Some(123)).unwrap(), 50);
        let b = collect_reinforcement_times_vi(VI::new(10.0, 12, Some(123)).unwrap(), 50);
        assert_eq!(a, b);
    }

    #[test]
    fn vi_reset_replays_trajectory() {
        let mut vi = VI::new(10.0, 12, Some(42)).unwrap();
        let first = collect_reinforcement_times_vi_mut(&mut vi, 30);
        vi.reset();
        let second = collect_reinforcement_times_vi_mut(&mut vi, 30);
        assert_eq!(first, second);
    }

    fn collect_reinforcement_times_vi(mut vi: VI, count: usize) -> Vec<f64> {
        collect_reinforcement_times_vi_mut(&mut vi, count)
    }

    fn collect_reinforcement_times_vi_mut(vi: &mut VI, count: usize) -> Vec<f64> {
        let mut out = Vec::with_capacity(count);
        let dt = 0.01_f64;
        let mut t = 0.0_f64;
        let cap = 1e7_f64;
        while out.len() < count && t < cap {
            t += dt;
            let ev = ResponseEvent::new(t);
            if vi.step(t, Some(&ev)).unwrap().reinforced {
                out.push(t);
            }
        }
        out
    }

    // ----- RI --------------------------------------------------------------

    #[test]
    fn ri_rejects_non_positive_mean() {
        assert!(matches!(
            RI::new(0.0, Some(1)).unwrap_err(),
            ContingencyError::Config(_)
        ));
    }

    #[test]
    fn ri_deterministic_under_seed() {
        let a = collect_reinforcement_times_ri(RI::new(10.0, Some(999)).unwrap(), 50);
        let b = collect_reinforcement_times_ri(RI::new(10.0, Some(999)).unwrap(), 50);
        assert_eq!(a, b);
    }

    #[test]
    fn ri_reset_replays() {
        let mut ri = RI::new(10.0, Some(7)).unwrap();
        let first = collect_reinforcement_times_ri_mut(&mut ri, 30);
        ri.reset();
        let second = collect_reinforcement_times_ri_mut(&mut ri, 30);
        assert_eq!(first, second);
    }

    #[test]
    fn ri_inter_reinforcement_intervals_approx_exponential() {
        // With continuous responding (dt tiny relative to mean) the
        // observed IRIs approximate draws from Exp(1/mean).
        let mean = 5.0_f64;
        let mut ri = RI::new(mean, Some(1337)).unwrap();
        let dt = 0.01_f64;
        let mut t = 0.0_f64;
        let mut last_rf = 0.0_f64;
        let mut iris: Vec<f64> = Vec::new();
        let target = 2000usize;
        let cap = mean * target as f64 * 10.0;
        while iris.len() < target && t < cap {
            t += dt;
            let ev = ResponseEvent::new(t);
            if ri.step(t, Some(&ev)).unwrap().reinforced {
                iris.push(t - last_rf);
                last_rf = t;
            }
        }
        assert_eq!(iris.len(), target);
        let sample_mean: f64 = iris.iter().sum::<f64>() / iris.len() as f64;
        // Standard error of the mean for Exp is mean / sqrt(N). With
        // N=2000 and mean=5.0, SE ≈ 0.11, so 5σ ≈ 0.56 is safe.
        assert!(
            (sample_mean - mean).abs() < 0.5,
            "sample_mean={sample_mean}"
        );
    }

    fn collect_reinforcement_times_ri(mut ri: RI, count: usize) -> Vec<f64> {
        collect_reinforcement_times_ri_mut(&mut ri, count)
    }

    fn collect_reinforcement_times_ri_mut(ri: &mut RI, count: usize) -> Vec<f64> {
        let mut out = Vec::with_capacity(count);
        let dt = 0.01_f64;
        let mut t = 0.0_f64;
        let cap = 1e7_f64;
        while out.len() < count && t < cap {
            t += dt;
            let ev = ResponseEvent::new(t);
            if ri.step(t, Some(&ev)).unwrap().reinforced {
                out.push(t);
            }
        }
        out
    }

    // ----- LimitedHold -----------------------------------------------------

    #[test]
    fn limited_hold_rejects_non_positive_hold() {
        let fi = FI::new(5.0).unwrap();
        assert!(matches!(
            LimitedHold::new(fi.clone(), 0.0).unwrap_err(),
            ContingencyError::Config(_)
        ));
        assert!(matches!(
            LimitedHold::new(fi, -1.0).unwrap_err(),
            ContingencyError::Config(_)
        ));
    }

    #[test]
    fn limited_hold_reinforces_within_window() {
        // FI 5, hold 2: opportunity open during [5, 7].
        let mut lh = LimitedHold::new(FI::new(5.0).unwrap(), 2.0).unwrap();
        let e1 = ResponseEvent::new(6.0);
        let out = lh.step(6.0, Some(&e1)).unwrap();
        assert!(out.reinforced);
        assert_eq!(out.reinforcer.unwrap().time, 6.0);
    }

    #[test]
    fn limited_hold_does_not_reinforce_past_window() {
        // FI 5, hold 2: opportunity closes at t=7. A response at
        // t=8 is past the hold and must NOT reinforce.
        let mut lh = LimitedHold::new(FI::new(5.0).unwrap(), 2.0).unwrap();
        let e = ResponseEvent::new(8.0);
        let out = lh.step(8.0, Some(&e)).unwrap();
        assert!(!out.reinforced);
    }

    #[test]
    fn limited_hold_rearms_after_missed_opportunity() {
        // FI 5, hold 2: window [5, 7] expires without a response.
        // A tick at t=8 withdraws and re-arms at now=8, so new
        // arm_time = 13. A response at 10 must NOT reinforce
        // (elapsed only 2 since re-arm); a response at 13 must.
        let mut lh = LimitedHold::new(FI::new(5.0).unwrap(), 2.0).unwrap();
        // Tick past the hold to trigger withdrawal.
        let out_tick = lh.step(8.0, None).unwrap();
        assert!(!out_tick.reinforced);
        // Response at 10 — only 2 s since re-arm; no reinforcement.
        let e1 = ResponseEvent::new(10.0);
        assert!(!lh.step(10.0, Some(&e1)).unwrap().reinforced);
        // Response at 13 — 5 s since re-arm; reinforces.
        let e2 = ResponseEvent::new(13.0);
        assert!(lh.step(13.0, Some(&e2)).unwrap().reinforced);
    }

    #[test]
    fn limited_hold_conformance_fi5_hold2_trajectory() {
        // Mirrors conformance/atomic/limited_hold_fi.json.
        let mut lh = LimitedHold::new(FI::new(5.0).unwrap(), 2.0).unwrap();
        let steps: Vec<(f64, Option<f64>, bool)> = vec![
            (0.0, Some(0.0), false),
            (1.0, Some(1.0), false),
            (4.0, Some(4.0), false),
            (6.0, Some(6.0), true),
            (10.0, Some(10.0), false), // window after 6 is [11, 13], still inside pre-arm
            (11.5, Some(11.5), true),
            (20.0, None, false),
            (30.0, Some(30.0), false),
        ];
        for (now, ev_time, expect_rf) in steps {
            let ev = ev_time.map(ResponseEvent::new);
            let out = lh.step(now, ev.as_ref()).unwrap();
            assert_eq!(out.reinforced, expect_rf, "unexpected outcome at now={now}");
            if expect_rf {
                assert_eq!(out.reinforcer.as_ref().unwrap().time, now);
            }
        }
    }

    #[test]
    fn limited_hold_generic_bound_compiles_for_all_armable() {
        // Type-level check that FI / VI / RI each satisfy the bound.
        let _a: LimitedHold<FI> = LimitedHold::new(FI::new(5.0).unwrap(), 2.0).unwrap();
        let _b: LimitedHold<VI> =
            LimitedHold::new(VI::new(10.0, 12, Some(1)).unwrap(), 2.0).unwrap();
        let _c: LimitedHold<RI> = LimitedHold::new(RI::new(5.0, Some(1)).unwrap(), 2.0).unwrap();
    }

    #[test]
    fn limited_hold_rejects_non_monotonic_time() {
        let mut lh = LimitedHold::new(FI::new(5.0).unwrap(), 2.0).unwrap();
        lh.step(3.0, None).unwrap();
        let err = lh.step(2.0, None).unwrap_err();
        assert!(matches!(err, ContingencyError::State(_)));
    }

    #[test]
    fn limited_hold_rejects_event_now_mismatch() {
        let mut lh = LimitedHold::new(FI::new(5.0).unwrap(), 2.0).unwrap();
        let bad = ResponseEvent::new(6.0);
        let err = lh.step(5.0, Some(&bad)).unwrap_err();
        assert!(matches!(err, ContingencyError::State(_)));
    }

    #[test]
    fn limited_hold_reset_restores_inner_and_clock() {
        let mut lh = LimitedHold::new(FI::new(5.0).unwrap(), 2.0).unwrap();
        let e = ResponseEvent::new(6.0);
        lh.step(6.0, Some(&e)).unwrap();
        lh.reset();
        // After reset, step at earlier time is allowed (clock cleared).
        let e2 = ResponseEvent::new(5.0);
        assert!(lh.step(5.0, Some(&e2)).unwrap().reinforced);
    }
}

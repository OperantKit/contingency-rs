//! Progressive-Ratio (PR) schedule.
//!
//! Under a progressive-ratio schedule, each successive reinforcer
//! requires more responses than the previous one. The ratio "steps up"
//! after every delivery, and the rule governing the step-up is called
//! the *step function*. PR is canonically used to measure reinforcer
//! efficacy by locating a *breakpoint* — the ratio at which responding
//! ceases in a session. Breakpoint detection is an experimental
//! procedure and is not enforced by the schedule itself:
//! [`ProgressiveRatio`] advances indefinitely.
//!
//! The module factors the step rule out of the schedule via the
//! [`StepFn`] trait so that new step series can be added without
//! touching [`ProgressiveRatio`]:
//!
//! * [`arithmetic`] — linear progression `r_n = start + n * step`
//!   (Hodos, 1961, used the simplest arithmetic form).
//! * [`geometric`] — multiplicative progression
//!   `r_n = round(start * ratio.powi(n))`.
//! * [`richardson_roberts`] — the 30-term Richardson-Roberts series
//!   popularised for drug self-administration (Richardson & Roberts,
//!   1996, Table 2), inspired by Hursh's (1980) demand-curve analyses.
//!   Values beyond index 29 are extrapolated geometrically using the
//!   final observed step ratio.
//!
//! # References
//!
//! Hodos, W. (1961). Progressive ratio as a measure of reward strength.
//! *Science*, 134(3483), 943-944.
//! <https://doi.org/10.1126/science.134.3483.943>
//!
//! Hursh, S. R. (1980). Economic concepts for the analysis of behavior.
//! *Journal of the Experimental Analysis of Behavior*, 34(2), 219-238.
//! <https://doi.org/10.1901/jeab.1980.34-219>
//!
//! Richardson, N. R., & Roberts, D. C. S. (1996). Progressive ratio
//! schedules in drug self-administration studies in rats: A method to
//! evaluate reinforcing efficacy. *Journal of Neuroscience Methods*,
//! 66(1), 1-11. <https://doi.org/10.1016/0165-0270(95)00153-0>

use crate::errors::ContingencyError;
use crate::helpers::checks::{check_event, check_time};
use crate::schedule::Schedule;
use crate::types::{Outcome, Reinforcer, ResponseEvent};
use crate::Result;

/// Step-function trait: maps a 0-based reinforcement index `n` to the
/// ratio requirement that must be met to earn *that* reinforcer.
///
/// The returned value must be `>= 1`. Returning `0` is a programming
/// error and is detected lazily when [`ProgressiveRatio`] first
/// consults the step function for the offending index.
pub trait StepFn: Send + Sync {
    /// Return the required response count for the given 0-based
    /// reinforcement index. Must be `>= 1`.
    fn at(&self, n: usize) -> u32;
}

/// Convenience alias for boxed step functions.
pub type BoxedStepFn = Box<dyn StepFn>;

// ---------------------------------------------------------------------------
// Arithmetic progression
// ---------------------------------------------------------------------------

struct Arithmetic {
    start: u32,
    step: u32,
}

impl StepFn for Arithmetic {
    fn at(&self, n: usize) -> u32 {
        // `start + n * step`; saturate on overflow to preserve
        // monotonicity rather than wrap silently.
        let increment = (n as u64).saturating_mul(self.step as u64);
        let total = (self.start as u64).saturating_add(increment);
        u32::try_from(total).unwrap_or(u32::MAX)
    }
}

/// Arithmetic progression `r_n = start + n * step`.
///
/// # Errors
///
/// Returns [`ContingencyError::Config`] if `start < 1` or `step < 1`.
///
/// # References
///
/// Hodos, W. (1961). Progressive ratio as a measure of reward strength.
/// *Science*, 134(3483), 943-944.
/// <https://doi.org/10.1126/science.134.3483.943>
pub fn arithmetic(start: u32, step: u32) -> Result<Box<dyn StepFn>> {
    if start < 1 {
        return Err(ContingencyError::Config(format!(
            "arithmetic requires start >= 1, got {start}"
        )));
    }
    if step < 1 {
        return Err(ContingencyError::Config(format!(
            "arithmetic requires step >= 1, got {step}"
        )));
    }
    Ok(Box::new(Arithmetic { start, step }))
}

// ---------------------------------------------------------------------------
// Geometric progression
// ---------------------------------------------------------------------------

struct Geometric {
    start: u32,
    ratio: f64,
}

impl StepFn for Geometric {
    fn at(&self, n: usize) -> u32 {
        let value = (self.start as f64) * self.ratio.powi(n as i32);
        let rounded = value.round();
        if !rounded.is_finite() || rounded >= u32::MAX as f64 {
            return u32::MAX;
        }
        // Defensive floor at 1 — cannot actually occur for `start >= 1`
        // and `ratio > 1.0`, but keeps the contract `at(..) >= 1`.
        let as_u32 = rounded as u32;
        as_u32.max(1)
    }
}

/// Geometric progression `r_n = round(start * ratio.powi(n))`.
///
/// # Errors
///
/// Returns [`ContingencyError::Config`] if `start < 1`, `ratio` is not
/// finite, or `ratio <= 1.0`.
pub fn geometric(start: u32, ratio: f64) -> Result<Box<dyn StepFn>> {
    if start < 1 {
        return Err(ContingencyError::Config(format!(
            "geometric requires start >= 1, got {start}"
        )));
    }
    if !ratio.is_finite() || ratio <= 1.0 {
        return Err(ContingencyError::Config(format!(
            "geometric requires ratio > 1.0, got {ratio}"
        )));
    }
    Ok(Box::new(Geometric { start, ratio }))
}

// ---------------------------------------------------------------------------
// Richardson-Roberts (1996) series
// ---------------------------------------------------------------------------

/// Richardson & Roberts (1996) series, Table 2 — 30 hardcoded values.
const RICHARDSON_ROBERTS_SERIES: [u32; 30] = [
    1, 2, 4, 6, 9, 12, 16, 20, 25, 32, 40, 50, 62, 77, 95, 118, 145, 178, 219, 268, 328, 402, 492,
    603, 737, 901, 1102, 1347, 1647, 2012,
];

struct RichardsonRoberts;

impl StepFn for RichardsonRoberts {
    fn at(&self, n: usize) -> u32 {
        let series = &RICHARDSON_ROBERTS_SERIES;
        if n < series.len() {
            return series[n];
        }
        // Geometric extrapolation from the last tabulated value, using
        // the final observed step ratio (2012 / 1647 ≈ 1.2217).
        let last = series[series.len() - 1] as f64;
        let penult = series[series.len() - 2] as f64;
        let ratio = last / penult;
        let exponent = (n - (series.len() - 1)) as i32;
        let value = (last * ratio.powi(exponent)).round();
        if !value.is_finite() || value >= u32::MAX as f64 {
            return u32::MAX;
        }
        (value as u32).max(1)
    }
}

/// Return the canonical Richardson-Roberts progressive-ratio series.
///
/// Values 0-29 are the canonical series reported by Richardson and
/// Roberts (1996), derived from Hursh's (1980) demand-curve framework:
/// `1, 2, 4, 6, 9, 12, 16, 20, 25, 32, 40, 50, 62, 77, 95, 118, 145,
/// 178, 219, 268, 328, 402, 492, 603, 737, 901, 1102, 1347, 1647,
/// 2012`.
///
/// Beyond index 29 the series is extrapolated geometrically using the
/// ratio between the final two tabulated values
/// (`2012 / 1647 ≈ 1.2217`). This keeps the series continuous for long
/// sessions without fabricating a different growth rule.
///
/// # References
///
/// Hursh, S. R. (1980). Economic concepts for the analysis of behavior.
/// *Journal of the Experimental Analysis of Behavior*, 34(2), 219-238.
/// <https://doi.org/10.1901/jeab.1980.34-219>
///
/// Richardson, N. R., & Roberts, D. C. S. (1996). Progressive ratio
/// schedules in drug self-administration studies in rats: A method to
/// evaluate reinforcing efficacy. *Journal of Neuroscience Methods*,
/// 66(1), 1-11. <https://doi.org/10.1016/0165-0270(95)00153-0>
pub fn richardson_roberts() -> Box<dyn StepFn> {
    Box::new(RichardsonRoberts)
}

// ---------------------------------------------------------------------------
// ProgressiveRatio schedule
// ---------------------------------------------------------------------------

/// Progressive-Ratio schedule parameterised by a boxed [`StepFn`].
///
/// Each [`ResponseEvent`] increments an internal counter. When the
/// counter reaches the current requirement (`step_fn.at(index)`, where
/// `index` is the 0-based count of reinforcers *already* delivered), a
/// [`Reinforcer`] is emitted, the counter is reset to zero, and `index`
/// advances by one. The next requirement is `step_fn.at(index + 1)`,
/// and so on.
///
/// **No breakpoint termination.** Progressive-ratio procedures are
/// often paired with a *breakpoint* criterion (e.g. no reinforcer
/// earned within some interval) to identify the ratio at which
/// responding ceases. That criterion is a *procedural* layer and does
/// not belong in the schedule itself — `ProgressiveRatio` keeps
/// stepping up indefinitely. Break-point detection should be
/// implemented by the session runner.
///
/// Invalid step-function outputs are detected lazily: if
/// `step_fn.at(index)` returns `0` the first time it is consulted on a
/// response, [`ContingencyError::Config`] is raised.
///
/// # References
///
/// Hodos, W. (1961). Progressive ratio as a measure of reward strength.
/// *Science*, 134(3483), 943-944.
/// <https://doi.org/10.1126/science.134.3483.943>
///
/// Hursh, S. R. (1980). Economic concepts for the analysis of behavior.
/// *Journal of the Experimental Analysis of Behavior*, 34(2), 219-238.
/// <https://doi.org/10.1901/jeab.1980.34-219>
///
/// Richardson, N. R., & Roberts, D. C. S. (1996). Progressive ratio
/// schedules in drug self-administration studies in rats: A method to
/// evaluate reinforcing efficacy. *Journal of Neuroscience Methods*,
/// 66(1), 1-11. <https://doi.org/10.1016/0165-0270(95)00153-0>
pub struct ProgressiveRatio {
    step_fn: Box<dyn StepFn>,
    current_index: usize,
    counter: u32,
    current_requirement: u32,
    last_now: Option<f64>,
}

impl std::fmt::Debug for ProgressiveRatio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProgressiveRatio")
            .field("current_index", &self.current_index)
            .field("counter", &self.counter)
            .field("current_requirement", &self.current_requirement)
            .field("last_now", &self.last_now)
            .finish()
    }
}

impl ProgressiveRatio {
    /// Construct a progressive-ratio schedule driven by `step_fn`.
    ///
    /// The first requirement is cached eagerly at construction via
    /// `step_fn.at(0)`. If that value is `0`, the fault is surfaced the
    /// first time [`ProgressiveRatio::step`] is called with a response
    /// (lazy validation, mirroring the Python reference).
    pub fn new(step_fn: Box<dyn StepFn>) -> Self {
        let first = step_fn.at(0);
        Self {
            step_fn,
            current_index: 0,
            counter: 0,
            current_requirement: first,
            last_now: None,
        }
    }

    /// Response count needed to earn the next reinforcer.
    pub fn current_requirement(&self) -> u32 {
        self.current_requirement
    }

    /// 0-based index of the *next* reinforcer to be earned.
    pub fn current_reinforcement_index(&self) -> usize {
        self.current_index
    }

    fn validate_requirement(&self, requirement: u32) -> Result<()> {
        if requirement < 1 {
            return Err(ContingencyError::Config(format!(
                "step_fn.at({}) must return a positive value, got {}",
                self.current_index, requirement
            )));
        }
        Ok(())
    }
}

impl Schedule for ProgressiveRatio {
    fn step(&mut self, now: f64, event: Option<&ResponseEvent>) -> Result<Outcome> {
        check_time(now, self.last_now)?;
        check_event(now, event)?;
        self.last_now = Some(now);
        if event.is_none() {
            return Ok(Outcome::empty());
        }
        // Lazy-validate the currently cached requirement the first
        // time it is consulted against a response.
        self.validate_requirement(self.current_requirement)?;
        self.counter += 1;
        if self.counter >= self.current_requirement {
            self.counter = 0;
            self.current_index += 1;
            self.current_requirement = self.step_fn.at(self.current_index);
            return Ok(Outcome::reinforced(Reinforcer::at(now)));
        }
        Ok(Outcome::empty())
    }

    fn reset(&mut self) {
        self.current_index = 0;
        self.counter = 0;
        self.current_requirement = self.step_fn.at(0);
        self.last_now = None;
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn respond<S: Schedule>(s: &mut S, now: f64) -> Outcome {
        let ev = ResponseEvent::new(now);
        s.step(now, Some(&ev)).expect("step should succeed")
    }

    // --- Step functions: arithmetic -------------------------------------

    #[test]
    fn arithmetic_1_1_series() {
        let fn_ = arithmetic(1, 1).unwrap();
        let values: Vec<u32> = (0..5).map(|n| fn_.at(n)).collect();
        assert_eq!(values, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn arithmetic_2_3_series() {
        let fn_ = arithmetic(2, 3).unwrap();
        let values: Vec<u32> = (0..5).map(|n| fn_.at(n)).collect();
        assert_eq!(values, vec![2, 5, 8, 11, 14]);
    }

    #[test]
    fn arithmetic_start_zero_raises() {
        assert!(matches!(
            arithmetic(0, 1),
            Err(ContingencyError::Config(_))
        ));
    }

    #[test]
    fn arithmetic_step_zero_raises() {
        assert!(matches!(
            arithmetic(1, 0),
            Err(ContingencyError::Config(_))
        ));
    }

    #[test]
    fn arithmetic_strictly_monotonic() {
        let fn_ = arithmetic(3, 7).unwrap();
        let values: Vec<u32> = (0..200).map(|n| fn_.at(n)).collect();
        for pair in values.windows(2) {
            assert!(pair[0] < pair[1]);
        }
    }

    // --- Step functions: geometric --------------------------------------

    #[test]
    fn geometric_doubling_series() {
        let fn_ = geometric(1, 2.0).unwrap();
        let values: Vec<u32> = (0..5).map(|n| fn_.at(n)).collect();
        assert_eq!(values, vec![1, 2, 4, 8, 16]);
    }

    #[test]
    fn geometric_start4_ratio_1_5_rounded() {
        let fn_ = geometric(4, 1.5).unwrap();
        // 4, 6, 9, 13.5 -> 14, 20.25 -> 20
        let values: Vec<u32> = (0..5).map(|n| fn_.at(n)).collect();
        assert_eq!(values, vec![4, 6, 9, 14, 20]);
    }

    #[test]
    fn geometric_start_zero_raises() {
        assert!(matches!(
            geometric(0, 2.0),
            Err(ContingencyError::Config(_))
        ));
    }

    #[test]
    fn geometric_ratio_one_raises() {
        assert!(matches!(
            geometric(1, 1.0),
            Err(ContingencyError::Config(_))
        ));
    }

    #[test]
    fn geometric_ratio_below_one_raises() {
        assert!(matches!(
            geometric(1, 0.5),
            Err(ContingencyError::Config(_))
        ));
    }

    #[test]
    fn geometric_ratio_nan_raises() {
        assert!(matches!(
            geometric(1, f64::NAN),
            Err(ContingencyError::Config(_))
        ));
    }

    #[test]
    fn geometric_strictly_monotonic() {
        let fn_ = geometric(1, 1.2).unwrap();
        let values: Vec<u32> = (0..50).map(|n| fn_.at(n)).collect();
        // Strictly monotonic over meaningful range — after integer
        // rounding crosses above 1, differences never flatten because
        // ratio > 1.1 separates consecutive rounded values.
        for pair in values.windows(2) {
            assert!(pair[0] <= pair[1]);
        }
        // And overall strictly increasing across the tested window.
        assert!(values[0] < *values.last().unwrap());
    }

    // --- Step functions: richardson_roberts ------------------------------

    #[test]
    fn rr_first_five_values() {
        let fn_ = richardson_roberts();
        let values: Vec<u32> = (0..5).map(|n| fn_.at(n)).collect();
        assert_eq!(values, vec![1, 2, 4, 6, 9]);
    }

    #[test]
    fn rr_full_canonical_series() {
        let fn_ = richardson_roberts();
        let values: Vec<u32> = (0..30).map(|n| fn_.at(n)).collect();
        assert_eq!(values, RICHARDSON_ROBERTS_SERIES.to_vec());
    }

    #[test]
    fn rr_extrapolation_beyond_29() {
        let fn_ = richardson_roberts();
        let v30 = fn_.at(30);
        let v31 = fn_.at(31);
        assert!(v30 > 2012);
        assert!(v31 > v30);
        // Expected values: round(2012 * 2012/1647), round(2012 * (2012/1647)^2)
        let ratio = 2012.0_f64 / 1647.0_f64;
        assert_eq!(v30, (2012.0 * ratio).round() as u32);
        assert_eq!(v31, (2012.0 * ratio.powi(2)).round() as u32);
    }

    #[test]
    fn rr_extrapolation_far_index_still_positive_and_monotonic() {
        let fn_ = richardson_roberts();
        for n in [40usize, 60, 100] {
            let value = fn_.at(n);
            assert!(value > fn_.at(n - 1));
        }
    }

    #[test]
    fn rr_strictly_monotonic_over_range() {
        let fn_ = richardson_roberts();
        let values: Vec<u32> = (0..60).map(|n| fn_.at(n)).collect();
        for pair in values.windows(2) {
            assert!(pair[0] < pair[1]);
        }
    }

    // --- ProgressiveRatio: construction ----------------------------------

    #[test]
    fn pr_initial_requirement_matches_step_fn_zero() {
        let pr = ProgressiveRatio::new(arithmetic(3, 2).unwrap());
        assert_eq!(pr.current_requirement(), 3);
    }

    #[test]
    fn pr_initial_index_is_zero() {
        let pr = ProgressiveRatio::new(arithmetic(1, 1).unwrap());
        assert_eq!(pr.current_reinforcement_index(), 0);
    }

    // --- ProgressiveRatio: arithmetic progression ------------------------

    #[test]
    fn pr_arithmetic_1_1_reinforcer_positions() {
        // Requirements 1, 2, 3, 4, 5 → reinforced at responses
        // 1, 3, 6, 10, 15 (cumulative sums).
        let mut pr = ProgressiveRatio::new(arithmetic(1, 1).unwrap());
        let mut reinforced_at: Vec<usize> = Vec::new();
        for i in 1..=20usize {
            let o = respond(&mut pr, i as f64);
            if o.reinforced {
                reinforced_at.push(i);
            }
        }
        assert_eq!(reinforced_at, vec![1, 3, 6, 10, 15]);
    }

    #[test]
    fn pr_arithmetic_reinforcer_timestamp_equals_now() {
        let mut pr = ProgressiveRatio::new(arithmetic(1, 1).unwrap());
        let o = respond(&mut pr, 1.0);
        assert!(o.reinforced);
        let r = o.reinforcer.expect("reinforcer");
        assert_eq!(r.time, 1.0);
        assert_eq!(r.label, "SR+");
        assert_eq!(r.magnitude, 1.0);
    }

    #[test]
    fn pr_index_advances_after_reinforcement() {
        let mut pr = ProgressiveRatio::new(arithmetic(1, 1).unwrap());
        assert_eq!(pr.current_reinforcement_index(), 0);
        respond(&mut pr, 1.0); // meets criterion 1 → reinforce, index→1
        assert_eq!(pr.current_reinforcement_index(), 1);
        assert_eq!(pr.current_requirement(), 2);
        respond(&mut pr, 2.0);
        assert_eq!(pr.current_reinforcement_index(), 1);
        respond(&mut pr, 3.0); // second reinforcer
        assert_eq!(pr.current_reinforcement_index(), 2);
    }

    #[test]
    fn pr_non_response_steps_never_reinforce() {
        let mut pr = ProgressiveRatio::new(arithmetic(1, 1).unwrap());
        for t in [0.1f64, 0.5, 1.0, 1.5, 2.0] {
            let o = pr.step(t, None).unwrap();
            assert!(!o.reinforced);
            assert!(o.reinforcer.is_none());
        }
    }

    // --- ProgressiveRatio: geometric progression -------------------------

    #[test]
    fn pr_geometric_2x_pattern() {
        // geometric(1, 2.0): 1, 2, 4, 8 → reinforced at 1, 3, 7, 15.
        let mut pr = ProgressiveRatio::new(geometric(1, 2.0).unwrap());
        let mut reinforced_at: Vec<usize> = Vec::new();
        for i in 1..=16usize {
            let o = respond(&mut pr, i as f64);
            if o.reinforced {
                reinforced_at.push(i);
            }
        }
        assert_eq!(reinforced_at, vec![1, 3, 7, 15]);
    }

    // --- ProgressiveRatio: Richardson-Roberts ----------------------------

    #[test]
    fn pr_richardson_roberts_first_requirements_match_series() {
        let mut pr = ProgressiveRatio::new(richardson_roberts());
        let expected = vec![1u32, 2, 4, 6, 9];
        let mut observed: Vec<u32> = Vec::new();
        for i in 1..=25usize {
            let requirement_before = pr.current_requirement();
            let o = respond(&mut pr, i as f64);
            if o.reinforced {
                observed.push(requirement_before);
            }
            if observed.len() == 5 {
                break;
            }
        }
        assert_eq!(observed, expected);
        // And the total responses consumed = 1 + 2 + 4 + 6 + 9 = 22.
        // (Sanity check via index.)
        assert_eq!(pr.current_reinforcement_index(), 5);
    }

    // --- ProgressiveRatio: reset -----------------------------------------

    #[test]
    fn pr_reset_restarts_from_index_zero() {
        let mut pr = ProgressiveRatio::new(arithmetic(1, 1).unwrap());
        for t in [1.0f64, 2.0, 3.0, 4.0, 5.0, 6.0] {
            respond(&mut pr, t);
        }
        assert!(pr.current_reinforcement_index() > 0);
        pr.reset();
        assert_eq!(pr.current_reinforcement_index(), 0);
        assert_eq!(pr.current_requirement(), 1);
        let o = respond(&mut pr, 7.0);
        assert!(o.reinforced);
    }

    #[test]
    fn pr_reset_clears_last_time() {
        let mut pr = ProgressiveRatio::new(arithmetic(1, 1).unwrap());
        respond(&mut pr, 100.0);
        pr.reset();
        // After reset, stepping "earlier" again is valid.
        let o = respond(&mut pr, 0.0);
        assert!(o.reinforced);
    }

    // --- ProgressiveRatio: invalid step-function values ------------------

    struct ZeroStep;
    impl StepFn for ZeroStep {
        fn at(&self, _n: usize) -> u32 {
            0
        }
    }

    #[test]
    fn pr_step_fn_returning_zero_raises_on_trigger() {
        let mut pr = ProgressiveRatio::new(Box::new(ZeroStep));
        let err = respond_err(&mut pr, 1.0);
        assert!(matches!(err, ContingencyError::Config(_)));
    }

    fn respond_err(pr: &mut ProgressiveRatio, now: f64) -> ContingencyError {
        let ev = ResponseEvent::new(now);
        pr.step(now, Some(&ev)).expect_err("expected error")
    }

    // --- State errors ----------------------------------------------------

    #[test]
    fn pr_rejects_non_monotonic_time() {
        let mut pr = ProgressiveRatio::new(arithmetic(2, 1).unwrap());
        respond(&mut pr, 10.0);
        let ev = ResponseEvent::new(5.0);
        let err = pr.step(5.0, Some(&ev)).unwrap_err();
        assert!(matches!(err, ContingencyError::State(_)));
    }

    #[test]
    fn pr_rejects_mismatched_event_time() {
        let mut pr = ProgressiveRatio::new(arithmetic(1, 1).unwrap());
        let ev = ResponseEvent::new(0.5);
        let err = pr.step(1.0, Some(&ev)).unwrap_err();
        assert!(matches!(err, ContingencyError::State(_)));
    }

    #[test]
    fn pr_monotonic_within_tolerance_allowed() {
        let mut pr = ProgressiveRatio::new(arithmetic(2, 1).unwrap());
        respond(&mut pr, 10.0);
        // Small backward step within tolerance should not raise.
        let ev = ResponseEvent::new(10.0 - 1e-12);
        pr.step(10.0 - 1e-12, Some(&ev)).unwrap();
    }

    // --- Integration -----------------------------------------------------

    #[test]
    fn pr_integration_ten_reinforcers_arithmetic_1_1() {
        // On arithmetic(1,1), response counts between reinforcers = [1..10].
        let mut pr = ProgressiveRatio::new(arithmetic(1, 1).unwrap());
        let mut response_counts: Vec<u32> = Vec::new();
        let mut count: u32 = 0;
        let total_needed: usize = (1..=10).sum(); // 55
        for i in 1..=total_needed {
            count += 1;
            let o = respond(&mut pr, i as f64);
            if o.reinforced {
                response_counts.push(count);
                count = 0;
            }
        }
        assert_eq!(response_counts, vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
        assert_eq!(pr.current_reinforcement_index(), 10);
    }

    #[test]
    fn pr_integration_ten_reinforcers_arithmetic_2_3() {
        let mut pr = ProgressiveRatio::new(arithmetic(2, 3).unwrap());
        let expected: Vec<u32> = (0..10).map(|n| 2 + 3 * n as u32).collect();
        let total: u32 = expected.iter().sum();
        let mut response_counts: Vec<u32> = Vec::new();
        let mut count: u32 = 0;
        for i in 1..=total as usize {
            count += 1;
            let o = respond(&mut pr, i as f64);
            if o.reinforced {
                response_counts.push(count);
                count = 0;
            }
        }
        assert_eq!(response_counts, expected);
    }
}

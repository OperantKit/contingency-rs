//! Percentile-shaped reinforcement schedule — Platt (1973); Galbicka
//! (1994).
//!
//! Reinforces a response whose measured dimension falls in a
//! specified percentile of a recent response window. This
//! implementation realises only the IRT (inter-response-time)
//! dimension; other dimensions construct successfully (for DSL
//! round-trip) but raise [`ContingencyError::Config`] on `step()`
//! with an event.
//!
//! The window uses a **compare-then-update** order: the percentile is
//! computed from the window *before* the current IRT is appended, so
//! a response is never part of its own reference distribution. The
//! percentile is computed via explicit sort + linear interpolation
//! (matching `numpy.percentile` "linear" / Hyndman-Fan Type 7).
//!
//! # References
//!
//! Galbicka, G. (1994). Shaping in the 21st century: Moving
//! percentile schedules into applied settings. *Journal of Applied
//! Behavior Analysis*, 27(4), 739-760.
//! <https://doi.org/10.1901/jaba.1994.27-739>
//!
//! Platt, J. R. (1973). Percentile reinforcement: Paradigms for
//! experimental analysis of response shaping. In G. H. Bower (Ed.),
//! *The Psychology of Learning and Motivation* (Vol. 7, pp. 271-296).
//! Academic Press. <https://doi.org/10.1016/S0079-7421(08)60471-7>
//!
//! Hyndman, R. J., & Fan, Y. (1996). Sample quantiles in statistical
//! packages. *The American Statistician*, 50(4), 361-365.
//! <https://doi.org/10.1080/00031305.1996.10473566>

use std::collections::VecDeque;

use crate::constants::TIME_TOL;
use crate::errors::ContingencyError;
use crate::helpers::checks::{check_event, check_time};
use crate::schedule::Schedule;
use crate::types::{MetaValue, Outcome, Reinforcer, ResponseEvent};
use crate::Result;

/// Which response dimension a [`Percentile`] schedule tracks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PercentileTarget {
    /// Inter-response time (executable).
    Irt,
}

/// Which tail of the window triggers reinforcement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PercentileDirection {
    /// Reinforce when the IRT is ≥ the rank-th percentile.
    Above,
    /// Reinforce when the IRT is ≤ the rank-th percentile.
    Below,
}

/// Percentile-shaped schedule.
#[derive(Debug)]
pub struct Percentile {
    target: PercentileTarget,
    rank: u8,
    window_size: usize,
    direction: PercentileDirection,
    window: VecDeque<f64>,
    last_response_time: Option<f64>,
    last_now: Option<f64>,
}

impl Percentile {
    /// Construct a percentile schedule.
    ///
    /// # Errors
    ///
    /// - `rank` outside `[1, 100]` (0-th percentile is degenerate)
    /// - `window < 1`
    pub fn new(
        target: PercentileTarget,
        rank: u8,
        window: usize,
        direction: PercentileDirection,
    ) -> Result<Self> {
        if !(1..=100).contains(&rank) {
            return Err(ContingencyError::Config(format!(
                "Percentile rank must be in [1, 100], got {rank}"
            )));
        }
        if window < 1 {
            return Err(ContingencyError::Config(format!(
                "Percentile window must be >= 1, got {window}"
            )));
        }
        Ok(Self {
            target,
            rank,
            window_size: window,
            direction,
            window: VecDeque::with_capacity(window),
            last_response_time: None,
            last_now: None,
        })
    }

    /// Configured response dimension.
    pub fn target(&self) -> PercentileTarget {
        self.target
    }

    /// Configured percentile rank.
    pub fn rank(&self) -> u8 {
        self.rank
    }

    /// Configured window size.
    pub fn window(&self) -> usize {
        self.window_size
    }

    /// Configured direction.
    pub fn direction(&self) -> PercentileDirection {
        self.direction
    }

    /// Current window contents (oldest first).
    pub fn samples(&self) -> Vec<f64> {
        self.window.iter().copied().collect()
    }
}

fn percentile_linear(sorted_samples: &[f64], rank: u8) -> f64 {
    let n = sorted_samples.len();
    if n == 1 {
        return sorted_samples[0];
    }
    let q = (rank as f64).clamp(0.0, 100.0) / 100.0;
    let h = q * (n as f64 - 1.0);
    let lo = h.floor() as usize;
    let hi = (lo + 1).min(n - 1);
    let frac = h - lo as f64;
    sorted_samples[lo] + (sorted_samples[hi] - sorted_samples[lo]) * frac
}

impl Schedule for Percentile {
    fn step(&mut self, now: f64, event: Option<&ResponseEvent>) -> Result<Outcome> {
        check_time(now, self.last_now)?;
        check_event(now, event)?;
        self.last_now = Some(now);

        if event.is_none() {
            return Ok(Outcome::empty());
        }

        // Non-IRT targets accepted for DSL round-trip but not executable.
        match self.target {
            PercentileTarget::Irt => {}
        }

        // First response ever: anchor the IRT baseline, do not reinforce.
        let Some(prev) = self.last_response_time else {
            self.last_response_time = Some(now);
            return Ok(Outcome::empty());
        };
        let irt = now - prev;
        self.last_response_time = Some(now);

        if self.window.len() < self.window_size {
            self.window.push_back(irt);
            return Ok(Outcome::empty());
        }

        // Compare first, then update (response not in its own reference).
        let mut sorted: Vec<f64> = self.window.iter().copied().collect();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let threshold = percentile_linear(&sorted, self.rank);

        let eligible = match self.direction {
            PercentileDirection::Above => irt >= threshold - TIME_TOL,
            PercentileDirection::Below => irt <= threshold + TIME_TOL,
        };

        // FIFO update.
        self.window.push_back(irt);
        if self.window.len() > self.window_size {
            self.window.pop_front();
        }

        if eligible {
            let mut out = Outcome::reinforced(Reinforcer::at(now));
            out.meta.insert(
                "percentile_threshold".to_string(),
                MetaValue::Float(threshold),
            );
            out.meta
                .insert("percentile_irt".to_string(), MetaValue::Float(irt));
            out.meta.insert(
                "percentile_rank".to_string(),
                MetaValue::Int(self.rank as i64),
            );
            out.meta.insert(
                "percentile_direction".to_string(),
                MetaValue::Str(
                    match self.direction {
                        PercentileDirection::Above => "above",
                        PercentileDirection::Below => "below",
                    }
                    .to_string(),
                ),
            );
            return Ok(out);
        }
        Ok(Outcome::empty())
    }

    fn reset(&mut self) {
        self.window.clear();
        self.last_response_time = None;
        self.last_now = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn respond(s: &mut Percentile, now: f64) -> Outcome {
        let ev = ResponseEvent::new(now);
        s.step(now, Some(&ev)).expect("step should succeed")
    }

    #[test]
    fn construct_rejects_rank_zero() {
        assert!(matches!(
            Percentile::new(PercentileTarget::Irt, 0, 5, PercentileDirection::Above),
            Err(ContingencyError::Config(_))
        ));
    }

    #[test]
    fn construct_rejects_rank_over_hundred() {
        assert!(matches!(
            Percentile::new(PercentileTarget::Irt, 101, 5, PercentileDirection::Above),
            Err(ContingencyError::Config(_))
        ));
    }

    #[test]
    fn construct_rejects_zero_window() {
        assert!(matches!(
            Percentile::new(PercentileTarget::Irt, 50, 0, PercentileDirection::Above),
            Err(ContingencyError::Config(_))
        ));
    }

    #[test]
    fn first_response_never_reinforces() {
        let mut s =
            Percentile::new(PercentileTarget::Irt, 50, 3, PercentileDirection::Above).unwrap();
        assert!(!respond(&mut s, 1.0).reinforced);
    }

    #[test]
    fn window_baseline_never_reinforces() {
        // Window size 3: first response anchors IRT, next 3 fill the
        // window — none should reinforce.
        let mut s =
            Percentile::new(PercentileTarget::Irt, 50, 3, PercentileDirection::Above).unwrap();
        for t in [1.0, 2.0, 3.0, 4.0] {
            assert!(!respond(&mut s, t).reinforced);
        }
    }

    #[test]
    fn above_median_reinforces_after_baseline() {
        // Feed IRTs 1, 1, 1 as baseline. Then IRT=5 >> median(1,1,1) → reinforce.
        let mut s =
            Percentile::new(PercentileTarget::Irt, 50, 3, PercentileDirection::Above).unwrap();
        assert!(!respond(&mut s, 0.0).reinforced);
        assert!(!respond(&mut s, 1.0).reinforced);
        assert!(!respond(&mut s, 2.0).reinforced);
        assert!(!respond(&mut s, 3.0).reinforced);
        // Now window = [1,1,1]. Next IRT = 5.
        let o = respond(&mut s, 8.0);
        assert!(o.reinforced);
        assert_eq!(
            o.meta.get("percentile_rank"),
            Some(&MetaValue::Int(50))
        );
    }

    #[test]
    fn below_median_reinforces_for_short_irt() {
        let mut s =
            Percentile::new(PercentileTarget::Irt, 50, 3, PercentileDirection::Below).unwrap();
        // Fill window with long IRTs: 5, 5, 5.
        assert!(!respond(&mut s, 0.0).reinforced);
        assert!(!respond(&mut s, 5.0).reinforced);
        assert!(!respond(&mut s, 10.0).reinforced);
        assert!(!respond(&mut s, 15.0).reinforced);
        // IRT=1 < median(5,5,5) → below reinforces.
        let o = respond(&mut s, 16.0);
        assert!(o.reinforced);
    }

    #[test]
    fn percentile_linear_matches_numpy_convention() {
        // numpy.percentile([1,2,3,4], 50) == 2.5
        let v = vec![1.0, 2.0, 3.0, 4.0];
        assert!((percentile_linear(&v, 50) - 2.5).abs() < 1e-12);
        // numpy.percentile([1,2,3,4], 25) == 1.75
        assert!((percentile_linear(&v, 25) - 1.75).abs() < 1e-12);
        // Single-element degenerate case.
        assert_eq!(percentile_linear(&[7.0], 50), 7.0);
    }

    #[test]
    fn reset_clears_window_and_anchor() {
        let mut s =
            Percentile::new(PercentileTarget::Irt, 50, 3, PercentileDirection::Above).unwrap();
        respond(&mut s, 1.0);
        respond(&mut s, 2.0);
        s.reset();
        assert!(s.samples().is_empty());
        // After reset, first response cannot reinforce.
        assert!(!respond(&mut s, 3.0).reinforced);
    }

    #[test]
    fn rejects_non_monotonic_time() {
        let mut s =
            Percentile::new(PercentileTarget::Irt, 50, 3, PercentileDirection::Above).unwrap();
        respond(&mut s, 5.0);
        let ev = ResponseEvent::new(4.0);
        assert!(matches!(
            s.step(4.0, Some(&ev)),
            Err(ContingencyError::State(_))
        ));
    }

    #[test]
    fn tick_without_event_never_advances_irt() {
        let mut s =
            Percentile::new(PercentileTarget::Irt, 50, 3, PercentileDirection::Above).unwrap();
        // Ticks alone should do nothing.
        for t in [0.1, 0.2, 0.3] {
            let o = s.step(t, None).unwrap();
            assert!(!o.reinforced);
        }
        assert!(s.samples().is_empty());
    }
}

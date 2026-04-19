//! Timeout (TO) schedule wrapper — Leitenberg (1965).
//!
//! A timeout (TO) is a period following reinforcement during which the
//! underlying reinforcement schedule is suspended: responses do not
//! advance the inner schedule, and no reinforcement is available. When
//! `reset_on_response` is true, any response during the timeout
//! extends the window (punishment contingency for responding in
//! timeout).
//!
//! # References
//!
//! Leitenberg, H. (1965). Is time-out from positive reinforcement an
//! aversive event? A review of the experimental evidence.
//! *Psychological Bulletin*, 64(6), 428-441.
//! <https://doi.org/10.1037/h0022657>

use crate::constants::TIME_TOL;
use crate::errors::ContingencyError;
use crate::helpers::checks::{check_event, check_time};
use crate::schedule::Schedule;
use crate::types::{MetaValue, Outcome, ResponseEvent};
use crate::Result;

/// Timeout (TO) wrapper.
///
/// After the inner schedule reinforces, the wrapper enters a timeout
/// window of `duration` during which responses are ignored and no
/// reinforcement is possible. If `reset_on_response` is true, a
/// response within the window restarts the timeout clock.
///
/// # References
///
/// Leitenberg, H. (1965). Is time-out from positive reinforcement an
/// aversive event? A review of the experimental evidence.
/// *Psychological Bulletin*, 64(6), 428-441.
pub struct Timeout {
    inner: Box<dyn Schedule>,
    duration: f64,
    reset_on_response: bool,
    timeout_ends: Option<f64>,
    last_now: Option<f64>,
}

impl std::fmt::Debug for Timeout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Timeout")
            .field("duration", &self.duration)
            .field("reset_on_response", &self.reset_on_response)
            .field("timeout_ends", &self.timeout_ends)
            .field("last_now", &self.last_now)
            .finish()
    }
}

impl Timeout {
    /// Construct a Timeout wrapper.
    ///
    /// # Errors
    ///
    /// Returns [`ContingencyError::Config`] if `duration <= 0` or not finite.
    pub fn new(
        inner: Box<dyn Schedule>,
        duration: f64,
        reset_on_response: bool,
    ) -> Result<Self> {
        if !duration.is_finite() || duration <= 0.0 {
            return Err(ContingencyError::Config(format!(
                "Timeout requires duration > 0, got {duration}"
            )));
        }
        Ok(Self {
            inner,
            duration,
            reset_on_response,
            timeout_ends: None,
            last_now: None,
        })
    }

    /// The configured timeout duration.
    pub fn duration(&self) -> f64 {
        self.duration
    }

    /// Whether a response during the timeout restarts its clock.
    pub fn reset_on_response(&self) -> bool {
        self.reset_on_response
    }

    /// Whether the wrapper is currently in a timeout window.
    pub fn in_timeout(&self) -> bool {
        self.timeout_ends.is_some()
    }
}

impl Schedule for Timeout {
    fn step(&mut self, now: f64, event: Option<&ResponseEvent>) -> Result<Outcome> {
        check_time(now, self.last_now)?;
        check_event(now, event)?;
        self.last_now = Some(now);

        // Clear the timeout if the clock has advanced past its end.
        if let Some(ends) = self.timeout_ends {
            if now + TIME_TOL >= ends {
                self.timeout_ends = None;
            }
        }

        // Still in timeout: optionally extend on response, never step inner.
        if self.timeout_ends.is_some() {
            if event.is_some() && self.reset_on_response {
                self.timeout_ends = Some(now + self.duration);
            }
            let mut out = Outcome::empty();
            out.meta
                .insert("timeout_active".to_string(), MetaValue::Bool(true));
            return Ok(out);
        }

        // Not in timeout — step the inner schedule normally.
        let outcome = self.inner.step(now, event)?;
        if outcome.reinforced {
            self.timeout_ends = Some(now + self.duration);
            let mut meta = outcome.meta;
            meta.insert("timeout_started".to_string(), MetaValue::Bool(true));
            return Ok(Outcome {
                reinforced: true,
                reinforcer: outcome.reinforcer,
                meta,
            });
        }
        Ok(outcome)
    }

    fn reset(&mut self) {
        self.inner.reset();
        self.timeout_ends = None;
        self.last_now = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schedules::FR;

    fn respond<S: Schedule>(s: &mut S, now: f64) -> Outcome {
        let ev = ResponseEvent::new(now);
        s.step(now, Some(&ev)).expect("step should succeed")
    }

    #[test]
    fn construct_rejects_non_positive_duration() {
        let inner = Box::new(FR::new(1).unwrap());
        assert!(matches!(
            Timeout::new(inner, 0.0, false),
            Err(ContingencyError::Config(_))
        ));
        let inner = Box::new(FR::new(1).unwrap());
        assert!(matches!(
            Timeout::new(inner, -1.0, false),
            Err(ContingencyError::Config(_))
        ));
    }

    #[test]
    fn suspends_inner_during_timeout_window() {
        // FR(1) would reinforce every response; Timeout(2.0) gates it.
        let mut s = Timeout::new(Box::new(FR::new(1).unwrap()), 2.0, false).unwrap();
        let o1 = respond(&mut s, 0.0);
        assert!(o1.reinforced);
        assert_eq!(o1.meta.get("timeout_started"), Some(&MetaValue::Bool(true)));
        // Within timeout: response returns unreinforced with active flag.
        let o2 = respond(&mut s, 1.0);
        assert!(!o2.reinforced);
        assert_eq!(o2.meta.get("timeout_active"), Some(&MetaValue::Bool(true)));
        // Exactly at end of timeout: clock advances past end; inner stepped.
        let o3 = respond(&mut s, 2.0);
        assert!(o3.reinforced);
    }

    #[test]
    fn reset_on_response_extends_window() {
        let mut s = Timeout::new(Box::new(FR::new(1).unwrap()), 2.0, true).unwrap();
        let o0 = respond(&mut s, 0.0);
        assert!(o0.reinforced);
        // Responding at t=1.0 within the timeout extends it to 3.0.
        let o1 = respond(&mut s, 1.0);
        assert!(!o1.reinforced);
        assert_eq!(o1.meta.get("timeout_active"), Some(&MetaValue::Bool(true)));
        // Now at t=2.0 we would have exited the original window but the
        // window was extended to 3.0 — still active.
        let o2 = respond(&mut s, 2.0);
        assert!(!o2.reinforced);
        assert_eq!(o2.meta.get("timeout_active"), Some(&MetaValue::Bool(true)));
        // After the extended window ends (t=3.0 now >= extended_ends=3.0
        // because the last extension was at t=1.0 → ends=3.0, and the
        // tick at t=2.0 also extended → ends=4.0). Use a silent tick to
        // simply let the clock pass the end without further extension.
        let o_tick = s.step(5.0, None).unwrap();
        assert!(!o_tick.reinforced);
        // Now the timeout is cleared; the next response reinforces.
        let o3 = respond(&mut s, 5.0);
        assert!(o3.reinforced);
    }

    #[test]
    fn reset_clears_timeout() {
        let mut s = Timeout::new(Box::new(FR::new(1).unwrap()), 10.0, false).unwrap();
        respond(&mut s, 0.0);
        assert!(s.in_timeout());
        s.reset();
        assert!(!s.in_timeout());
        // Fresh schedule — next response reinforces immediately.
        let o = respond(&mut s, 0.0);
        assert!(o.reinforced);
    }

    #[test]
    fn rejects_non_monotonic_time() {
        let mut s = Timeout::new(Box::new(FR::new(1).unwrap()), 1.0, false).unwrap();
        respond(&mut s, 5.0);
        let ev = ResponseEvent::new(4.0);
        assert!(matches!(
            s.step(4.0, Some(&ev)),
            Err(ContingencyError::State(_))
        ));
    }

    #[test]
    fn rejects_mismatched_event_time() {
        let mut s = Timeout::new(Box::new(FR::new(1).unwrap()), 1.0, false).unwrap();
        let ev = ResponseEvent::new(0.5);
        assert!(matches!(
            s.step(1.0, Some(&ev)),
            Err(ContingencyError::State(_))
        ));
    }
}

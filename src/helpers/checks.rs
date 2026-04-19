//! Monotonic-time and event-time validation helpers.
//!
//! Centralised here so every schedule family uses identical
//! tolerance semantics. Python port uses one `TIME_TOL = 1e-9` across
//! five places (monotonic check, event/now match, elapsed check, DRH
//! eviction, LimitedHold expire) — this module covers the first two.

use crate::{constants::TIME_TOL, errors::ContingencyError, types::ResponseEvent, Result};

/// Assert that `now` does not move backwards relative to `last_now`
/// (within `TIME_TOL`).
#[inline]
pub fn check_time(now: f64, last_now: Option<f64>) -> Result<()> {
    if let Some(prev) = last_now {
        if now < prev - TIME_TOL {
            return Err(ContingencyError::State(format!(
                "non-monotonic time: now={now} < last_now={prev}"
            )));
        }
    }
    Ok(())
}

/// Assert that a provided `ResponseEvent`'s `time` equals `now` within
/// `TIME_TOL`.
#[inline]
pub fn check_event(now: f64, event: Option<&ResponseEvent>) -> Result<()> {
    if let Some(ev) = event {
        if (ev.time - now).abs() > TIME_TOL {
            return Err(ContingencyError::State(format!(
                "event.time ({}) does not match now ({})",
                ev.time, now
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_time_allows_equal() {
        assert!(check_time(1.0, Some(1.0)).is_ok());
    }

    #[test]
    fn check_time_allows_forward() {
        assert!(check_time(2.0, Some(1.0)).is_ok());
    }

    #[test]
    fn check_time_rejects_backward() {
        assert!(check_time(0.5, Some(1.0)).is_err());
    }

    #[test]
    fn check_time_allows_tolerance_underage() {
        assert!(check_time(1.0 - 1e-12, Some(1.0)).is_ok());
    }

    #[test]
    fn check_time_none_last_ok() {
        assert!(check_time(1.0, None).is_ok());
    }

    #[test]
    fn check_event_matches() {
        let e = ResponseEvent::new(1.0);
        assert!(check_event(1.0, Some(&e)).is_ok());
    }

    #[test]
    fn check_event_within_tolerance() {
        let e = ResponseEvent::new(1.0 + 1e-12);
        assert!(check_event(1.0, Some(&e)).is_ok());
    }

    #[test]
    fn check_event_mismatch() {
        let e = ResponseEvent::new(1.5);
        assert!(check_event(1.0, Some(&e)).is_err());
    }

    #[test]
    fn check_event_none_ok() {
        assert!(check_event(1.0, None).is_ok());
    }
}

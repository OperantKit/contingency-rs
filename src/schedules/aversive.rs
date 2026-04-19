//! Aversive-control schedules — shock-based contingencies.
//!
//! Rust port of `contingency.schedules.aversive`. Implements three
//! canonical aversive-control paradigms:
//!
//! * [`Sidman`] — non-discriminated (free-operant) avoidance.
//! * [`DiscriminatedAvoidance`] — signalled avoidance.
//! * [`Escape`] — aversive-termination paradigm (continuous shock emission
//!   on every tick until the subject responds or `trial_duration` elapses).
//!
//! All three deliver **negative-magnitude** [`Reinforcer`] events with
//! `label = "SR-"`. The `shock_magnitude` constructor parameter is given
//! as a positive number and internally negated when populating the
//! reinforcer's `magnitude` field.
//!
//! # References
//!
//! Keller, F. S. (1941). Light-aversion in the white rat. *Psychological
//! Record*, 4, 235-250.
//!
//! Sidman, M. (1953). Avoidance conditioning with brief shock and no
//! exteroceptive warning signal. *Science*, 118(3058), 157-158.
//! <https://doi.org/10.1126/science.118.3058.157>
//!
//! Solomon, R. L., & Wynne, L. C. (1953). Traumatic avoidance learning:
//! Acquisition in normal dogs. *Psychological Monographs: General and
//! Applied*, 67(4), 1-19. <https://doi.org/10.1037/h0093649>

use crate::constants::TIME_TOL;
use crate::errors::ContingencyError;
use crate::helpers::checks::{check_event, check_time};
use crate::schedule::Schedule;
use crate::types::{MetaValue, Outcome, Reinforcer, ResponseEvent};
use crate::Result;

fn validate_positive(name: &str, value: f64) -> Result<f64> {
    if !value.is_finite() || value <= 0.0 {
        return Err(ContingencyError::Config(format!(
            "{name} must be > 0, got {value}"
        )));
    }
    Ok(value)
}

fn validate_magnitude(value: f64) -> Result<f64> {
    if !value.is_finite() || value <= 0.0 {
        return Err(ContingencyError::Config(format!(
            "shock_magnitude must be > 0 (supply as positive; internally negated), got {value}"
        )));
    }
    Ok(value)
}

// ---------------------------------------------------------------------------
// Sidman
// ---------------------------------------------------------------------------

/// Sidman (non-discriminated) avoidance schedule.
///
/// `next_shock_time` is initialised to `shock_shock_interval`. Each
/// response resets `next_shock_time = now + response_shock_interval`.
/// When `now >= next_shock_time`, a shock is delivered (with
/// negative-magnitude [`Reinforcer`]). After a shock, the next shock is
/// scheduled using S-S (or R-S if a response is coincident with the
/// shock tick).
///
/// # References
///
/// Sidman, M. (1953). Avoidance conditioning with brief shock and no
/// exteroceptive warning signal. *Science*, 118(3058), 157-158.
/// <https://doi.org/10.1126/science.118.3058.157>
#[derive(Debug)]
pub struct Sidman {
    ssi: f64,
    rsi: f64,
    magnitude: f64,
    next_shock_time: f64,
    last_now: Option<f64>,
}

impl Sidman {
    /// Construct a Sidman avoidance schedule.
    pub fn new(
        shock_shock_interval: f64,
        response_shock_interval: f64,
        shock_magnitude: f64,
    ) -> Result<Self> {
        let ssi = validate_positive("shock_shock_interval", shock_shock_interval)?;
        let rsi = validate_positive("response_shock_interval", response_shock_interval)?;
        let mag = validate_magnitude(shock_magnitude)?;
        Ok(Self {
            ssi,
            rsi,
            magnitude: mag,
            next_shock_time: ssi,
            last_now: None,
        })
    }

    /// Shock-shock interval (time between shocks without responding).
    pub fn shock_shock_interval(&self) -> f64 {
        self.ssi
    }

    /// Response-shock interval (post-response shock delay).
    pub fn response_shock_interval(&self) -> f64 {
        self.rsi
    }

    /// Magnitude (positive number; delivered as `-magnitude`).
    pub fn shock_magnitude(&self) -> f64 {
        self.magnitude
    }

    /// Absolute time of the currently scheduled shock.
    pub fn next_shock_time(&self) -> f64 {
        self.next_shock_time
    }
}

impl Schedule for Sidman {
    fn step(&mut self, now: f64, event: Option<&ResponseEvent>) -> Result<Outcome> {
        check_time(now, self.last_now)?;
        check_event(now, event)?;
        self.last_now = Some(now);

        if event.is_some() {
            self.next_shock_time = now + self.rsi;
        }

        if now + TIME_TOL >= self.next_shock_time {
            let new_next = if event.is_some() {
                now + self.rsi
            } else {
                now + self.ssi
            };
            self.next_shock_time = new_next;
            let mut out = Outcome::reinforced(Reinforcer {
                time: now,
                magnitude: -self.magnitude,
                label: "SR-".into(),
            });
            out.meta.insert(
                "aversive_event".into(),
                MetaValue::Str("shock".into()),
            );
            return Ok(out);
        }
        Ok(Outcome::empty())
    }

    fn reset(&mut self) {
        self.next_shock_time = self.ssi;
        self.last_now = None;
    }
}

// ---------------------------------------------------------------------------
// DiscriminatedAvoidance
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AvPhase {
    Iti,
    Warning,
}

/// Discriminated avoidance schedule (Solomon & Wynne, 1953).
///
/// Phase machine: `iti -> warning -> (avoid | shock) -> iti`. Begins in
/// the `iti` phase; the first warning begins at `now == iti`.
///
/// # References
///
/// Solomon, R. L., & Wynne, L. C. (1953). Traumatic avoidance learning:
/// Acquisition in normal dogs. *Psychological Monographs: General and
/// Applied*, 67(4), 1-19. <https://doi.org/10.1037/h0093649>
#[derive(Debug)]
pub struct DiscriminatedAvoidance {
    warning_duration: f64,
    iti: f64,
    magnitude: f64,
    phase: AvPhase,
    phase_end: f64,
    last_now: Option<f64>,
}

impl DiscriminatedAvoidance {
    /// Construct a discriminated-avoidance schedule.
    pub fn new(warning_duration: f64, iti: f64, shock_magnitude: f64) -> Result<Self> {
        let wd = validate_positive("warning_duration", warning_duration)?;
        let it = validate_positive("iti", iti)?;
        let mag = validate_magnitude(shock_magnitude)?;
        Ok(Self {
            warning_duration: wd,
            iti: it,
            magnitude: mag,
            phase: AvPhase::Iti,
            phase_end: it,
            last_now: None,
        })
    }

    /// Warning-stimulus duration.
    pub fn warning_duration(&self) -> f64 {
        self.warning_duration
    }

    /// Inter-trial interval.
    pub fn iti(&self) -> f64 {
        self.iti
    }

    /// Magnitude (positive; delivered as `-magnitude`).
    pub fn shock_magnitude(&self) -> f64 {
        self.magnitude
    }

    /// Current phase as a string (`"iti"` or `"warning"`).
    pub fn phase(&self) -> &'static str {
        match self.phase {
            AvPhase::Iti => "iti",
            AvPhase::Warning => "warning",
        }
    }
}

impl Schedule for DiscriminatedAvoidance {
    fn step(&mut self, now: f64, event: Option<&ResponseEvent>) -> Result<Outcome> {
        check_time(now, self.last_now)?;
        check_event(now, event)?;
        self.last_now = Some(now);

        if matches!(self.phase, AvPhase::Iti) {
            if now + TIME_TOL >= self.phase_end {
                self.phase = AvPhase::Warning;
                self.phase_end = now + self.warning_duration;
                // fall through
            } else {
                let mut out = Outcome::empty();
                out.meta
                    .insert("phase".into(), MetaValue::Str("iti".into()));
                return Ok(out);
            }
        }

        if matches!(self.phase, AvPhase::Warning) {
            if event.is_some() {
                self.phase = AvPhase::Iti;
                self.phase_end = now + self.iti;
                let mut out = Outcome::empty();
                out.meta
                    .insert("phase".into(), MetaValue::Str("warning".into()));
                out.meta.insert(
                    "aversive_event".into(),
                    MetaValue::Str("avoidance".into()),
                );
                out.meta
                    .insert("trial_outcome".into(), MetaValue::Str("avoid".into()));
                return Ok(out);
            }
            if now + TIME_TOL >= self.phase_end {
                self.phase = AvPhase::Iti;
                self.phase_end = now + self.iti;
                let mut out = Outcome::reinforced(Reinforcer {
                    time: now,
                    magnitude: -self.magnitude,
                    label: "SR-".into(),
                });
                out.meta
                    .insert("phase".into(), MetaValue::Str("warning".into()));
                out.meta
                    .insert("aversive_event".into(), MetaValue::Str("shock".into()));
                out.meta
                    .insert("trial_outcome".into(), MetaValue::Str("shock".into()));
                return Ok(out);
            }
            let mut out = Outcome::empty();
            out.meta
                .insert("phase".into(), MetaValue::Str("warning".into()));
            return Ok(out);
        }

        // Unreachable — only Iti and Warning exist.
        Ok(Outcome::empty())
    }

    fn reset(&mut self) {
        self.phase = AvPhase::Iti;
        self.phase_end = self.iti;
        self.last_now = None;
    }
}

// ---------------------------------------------------------------------------
// Escape
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EscPhase {
    Shock,
    Iti,
}

/// Escape schedule — aversive-termination paradigm (Keller, 1941).
///
/// Trial starts with shock ON. Every tick during shock emits a
/// continuous-negative [`Outcome`] with `meta["aversive_event"]="shock"`
/// (the first tick is labelled `"shock_onset"` instead). A response
/// terminates shock (`"shock_termination"`); timeout ends with
/// `"trial_end"`.
///
/// # References
///
/// Keller, F. S. (1941). Light-aversion in the white rat. *Psychological
/// Record*, 4, 235-250.
#[derive(Debug)]
pub struct Escape {
    trial_duration: f64,
    iti: f64,
    magnitude: f64,
    phase: EscPhase,
    phase_end: f64,
    shock_onset_emitted: bool,
    last_now: Option<f64>,
}

impl Escape {
    /// Construct an Escape schedule.
    pub fn new(trial_duration: f64, iti: f64, shock_magnitude: f64) -> Result<Self> {
        let td = validate_positive("trial_duration", trial_duration)?;
        let it = validate_positive("iti", iti)?;
        let mag = validate_magnitude(shock_magnitude)?;
        Ok(Self {
            trial_duration: td,
            iti: it,
            magnitude: mag,
            phase: EscPhase::Shock,
            phase_end: td,
            shock_onset_emitted: false,
            last_now: None,
        })
    }

    /// Maximum shock duration per trial.
    pub fn trial_duration(&self) -> f64 {
        self.trial_duration
    }

    /// Inter-trial interval.
    pub fn iti(&self) -> f64 {
        self.iti
    }

    /// Magnitude (positive; delivered as `-magnitude`).
    pub fn shock_magnitude(&self) -> f64 {
        self.magnitude
    }

    /// Current phase (`"shock"` or `"iti"`).
    pub fn phase(&self) -> &'static str {
        match self.phase {
            EscPhase::Shock => "shock",
            EscPhase::Iti => "iti",
        }
    }
}

impl Schedule for Escape {
    fn step(&mut self, now: f64, event: Option<&ResponseEvent>) -> Result<Outcome> {
        check_time(now, self.last_now)?;
        check_event(now, event)?;
        self.last_now = Some(now);

        if matches!(self.phase, EscPhase::Iti) {
            if now + TIME_TOL >= self.phase_end {
                self.phase = EscPhase::Shock;
                self.phase_end = now + self.trial_duration;
                self.shock_onset_emitted = false;
                // fall through to shock handling
            } else {
                let mut out = Outcome::empty();
                out.meta
                    .insert("phase".into(), MetaValue::Str("iti".into()));
                return Ok(out);
            }
        }

        if matches!(self.phase, EscPhase::Shock) {
            if event.is_some() {
                self.phase = EscPhase::Iti;
                self.phase_end = now + self.iti;
                self.shock_onset_emitted = false;
                let mut out = Outcome::empty();
                out.meta
                    .insert("phase".into(), MetaValue::Str("shock".into()));
                out.meta.insert(
                    "aversive_event".into(),
                    MetaValue::Str("shock_termination".into()),
                );
                out.meta.insert(
                    "trial_outcome".into(),
                    MetaValue::Str("escape".into()),
                );
                return Ok(out);
            }
            if now + TIME_TOL >= self.phase_end {
                self.phase = EscPhase::Iti;
                self.phase_end = now + self.iti;
                self.shock_onset_emitted = false;
                let mut out = Outcome::empty();
                out.meta
                    .insert("phase".into(), MetaValue::Str("shock".into()));
                out.meta.insert(
                    "aversive_event".into(),
                    MetaValue::Str("trial_end".into()),
                );
                out.meta.insert(
                    "trial_outcome".into(),
                    MetaValue::Str("failed".into()),
                );
                return Ok(out);
            }
            let is_onset = !self.shock_onset_emitted;
            self.shock_onset_emitted = true;
            let label = if is_onset { "shock_onset" } else { "shock" };
            let mut out = Outcome::reinforced(Reinforcer {
                time: now,
                magnitude: -self.magnitude,
                label: "SR-".into(),
            });
            out.meta
                .insert("phase".into(), MetaValue::Str("shock".into()));
            out.meta
                .insert("aversive_event".into(), MetaValue::Str(label.into()));
            return Ok(out);
        }

        Ok(Outcome::empty())
    }

    fn reset(&mut self) {
        self.phase = EscPhase::Shock;
        self.phase_end = self.trial_duration;
        self.shock_onset_emitted = false;
        self.last_now = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(now: f64) -> ResponseEvent {
        ResponseEvent::new(now)
    }

    // ---- Sidman ----------------------------------------------------------

    #[test]
    fn sidman_construction_valid() {
        let s = Sidman::new(20.0, 5.0, 1.0).unwrap();
        assert_eq!(s.shock_shock_interval(), 20.0);
        assert_eq!(s.response_shock_interval(), 5.0);
        assert_eq!(s.shock_magnitude(), 1.0);
        assert_eq!(s.next_shock_time(), 20.0);
    }

    #[test]
    fn sidman_rejects_nonpositive_ssi() {
        assert!(Sidman::new(0.0, 5.0, 1.0).is_err());
        assert!(Sidman::new(-1.0, 5.0, 1.0).is_err());
    }

    #[test]
    fn sidman_rejects_nonpositive_rsi() {
        assert!(Sidman::new(20.0, 0.0, 1.0).is_err());
    }

    #[test]
    fn sidman_rejects_nonpositive_magnitude() {
        assert!(Sidman::new(20.0, 5.0, 0.0).is_err());
        assert!(Sidman::new(20.0, 5.0, -1.0).is_err());
    }

    #[test]
    fn sidman_ssi_shock_no_response() {
        let mut s = Sidman::new(10.0, 5.0, 1.0).unwrap();
        assert!(!s.step(0.0, None).unwrap().reinforced);
        assert!(!s.step(5.0, None).unwrap().reinforced);
        let o = s.step(10.0, None).unwrap();
        assert!(o.reinforced);
        let r = o.reinforcer.as_ref().unwrap();
        assert_eq!(r.magnitude, -1.0);
        assert_eq!(r.label, "SR-");
        assert_eq!(
            o.meta.get("aversive_event"),
            Some(&MetaValue::Str("shock".into()))
        );
    }

    #[test]
    fn sidman_successive_shocks_use_ssi() {
        let mut s = Sidman::new(10.0, 5.0, 1.0).unwrap();
        assert!(s.step(10.0, None).unwrap().reinforced);
        assert!(!s.step(15.0, None).unwrap().reinforced);
        assert!(s.step(20.0, None).unwrap().reinforced);
    }

    #[test]
    fn sidman_rsi_override_postpones_shock() {
        let mut s = Sidman::new(10.0, 10.0, 1.0).unwrap();
        let r = ev(5.0);
        assert!(!s.step(5.0, Some(&r)).unwrap().reinforced);
        assert!(!s.step(10.0, None).unwrap().reinforced);
        assert!(!s.step(14.0, None).unwrap().reinforced);
        assert!(s.step(15.0, None).unwrap().reinforced);
    }

    #[test]
    fn sidman_continuous_responding_suppresses_shocks() {
        let mut s = Sidman::new(10.0, 5.0, 1.0).unwrap();
        let mut t = 0.0;
        while t < 30.0 {
            t += 2.0;
            let r = ev(t);
            let o = s.step(t, Some(&r)).unwrap();
            assert!(!o.reinforced, "unexpected shock at t={t}");
        }
    }

    #[test]
    fn sidman_reset() {
        let mut s = Sidman::new(10.0, 5.0, 1.0).unwrap();
        let r = ev(5.0);
        s.step(5.0, Some(&r)).unwrap();
        assert_eq!(s.next_shock_time(), 10.0);
        s.reset();
        assert_eq!(s.next_shock_time(), 10.0);
        assert!(!s.step(0.0, None).unwrap().reinforced);
        assert!(s.step(10.0, None).unwrap().reinforced);
    }

    #[test]
    fn sidman_non_monotonic_time_raises() {
        let mut s = Sidman::new(10.0, 5.0, 1.0).unwrap();
        s.step(5.0, None).unwrap();
        assert!(s.step(4.0, None).is_err());
    }

    #[test]
    fn sidman_event_time_mismatch_raises() {
        let mut s = Sidman::new(10.0, 5.0, 1.0).unwrap();
        let r = ev(4.0);
        assert!(s.step(5.0, Some(&r)).is_err());
    }

    #[test]
    fn sidman_custom_magnitude() {
        let mut s = Sidman::new(5.0, 2.0, 3.5).unwrap();
        let o = s.step(5.0, None).unwrap();
        assert!(o.reinforced);
        assert_eq!(o.reinforcer.as_ref().unwrap().magnitude, -3.5);
    }

    // ---- DiscriminatedAvoidance -----------------------------------------

    #[test]
    fn discrimav_construction_valid() {
        let d = DiscriminatedAvoidance::new(5.0, 10.0, 1.0).unwrap();
        assert_eq!(d.warning_duration(), 5.0);
        assert_eq!(d.iti(), 10.0);
        assert_eq!(d.phase(), "iti");
    }

    #[test]
    fn discrimav_rejects_invalid_params() {
        assert!(DiscriminatedAvoidance::new(0.0, 10.0, 1.0).is_err());
        assert!(DiscriminatedAvoidance::new(5.0, -1.0, 1.0).is_err());
        assert!(DiscriminatedAvoidance::new(5.0, 10.0, 0.0).is_err());
    }

    #[test]
    fn discrimav_iti_phase_reports_iti() {
        let mut d = DiscriminatedAvoidance::new(5.0, 10.0, 1.0).unwrap();
        let o = d.step(2.0, None).unwrap();
        assert!(!o.reinforced);
        assert_eq!(
            o.meta.get("phase"),
            Some(&MetaValue::Str("iti".into()))
        );
    }

    #[test]
    fn discrimav_warning_starts_after_iti() {
        let mut d = DiscriminatedAvoidance::new(5.0, 10.0, 1.0).unwrap();
        d.step(9.0, None).unwrap();
        assert_eq!(d.phase(), "iti");
        let o = d.step(10.0, None).unwrap();
        assert_eq!(d.phase(), "warning");
        assert!(!o.reinforced);
    }

    #[test]
    fn discrimav_avoidance_response_in_warning() {
        let mut d = DiscriminatedAvoidance::new(5.0, 10.0, 1.0).unwrap();
        d.step(10.0, None).unwrap();
        let r = ev(12.0);
        let o = d.step(12.0, Some(&r)).unwrap();
        assert!(!o.reinforced);
        assert_eq!(
            o.meta.get("aversive_event"),
            Some(&MetaValue::Str("avoidance".into()))
        );
        assert_eq!(
            o.meta.get("trial_outcome"),
            Some(&MetaValue::Str("avoid".into()))
        );
        assert_eq!(d.phase(), "iti");
    }

    #[test]
    fn discrimav_shock_on_warning_timeout() {
        let mut d = DiscriminatedAvoidance::new(5.0, 10.0, 1.0).unwrap();
        d.step(10.0, None).unwrap();
        let o = d.step(15.0, None).unwrap();
        assert!(o.reinforced);
        let r = o.reinforcer.as_ref().unwrap();
        assert_eq!(r.magnitude, -1.0);
        assert_eq!(r.label, "SR-");
        assert_eq!(
            o.meta.get("aversive_event"),
            Some(&MetaValue::Str("shock".into()))
        );
        assert_eq!(d.phase(), "iti");
    }

    #[test]
    fn discrimav_second_trial_after_shock() {
        let mut d = DiscriminatedAvoidance::new(5.0, 10.0, 1.0).unwrap();
        d.step(10.0, None).unwrap();
        d.step(15.0, None).unwrap();
        d.step(20.0, None).unwrap();
        assert_eq!(d.phase(), "iti");
        d.step(25.0, None).unwrap();
        assert_eq!(d.phase(), "warning");
    }

    #[test]
    fn discrimav_reset() {
        let mut d = DiscriminatedAvoidance::new(5.0, 10.0, 1.0).unwrap();
        d.step(10.0, None).unwrap();
        let r = ev(12.0);
        d.step(12.0, Some(&r)).unwrap();
        d.reset();
        assert_eq!(d.phase(), "iti");
        let o = d.step(2.0, None).unwrap();
        assert_eq!(
            o.meta.get("phase"),
            Some(&MetaValue::Str("iti".into()))
        );
    }

    #[test]
    fn discrimav_non_monotonic_raises() {
        let mut d = DiscriminatedAvoidance::new(5.0, 10.0, 1.0).unwrap();
        d.step(5.0, None).unwrap();
        assert!(d.step(4.0, None).is_err());
    }

    // ---- Escape ----------------------------------------------------------

    #[test]
    fn escape_construction_valid() {
        let e = Escape::new(5.0, 3.0, 1.0).unwrap();
        assert_eq!(e.trial_duration(), 5.0);
        assert_eq!(e.iti(), 3.0);
        assert_eq!(e.phase(), "shock");
    }

    #[test]
    fn escape_rejects_invalid_params() {
        assert!(Escape::new(0.0, 3.0, 1.0).is_err());
        assert!(Escape::new(5.0, 0.0, 1.0).is_err());
        assert!(Escape::new(5.0, 3.0, -2.0).is_err());
    }

    #[test]
    fn escape_emits_shock_on_first_tick() {
        let mut e = Escape::new(5.0, 3.0, 1.0).unwrap();
        let o = e.step(0.0, None).unwrap();
        assert!(o.reinforced);
        assert_eq!(o.reinforcer.as_ref().unwrap().magnitude, -1.0);
        assert_eq!(
            o.meta.get("aversive_event"),
            Some(&MetaValue::Str("shock_onset".into()))
        );
        assert_eq!(
            o.meta.get("phase"),
            Some(&MetaValue::Str("shock".into()))
        );
    }

    #[test]
    fn escape_emits_shock_every_tick() {
        let mut e = Escape::new(10.0, 3.0, 1.0).unwrap();
        let outs: Vec<Outcome> = [0.0, 1.0, 2.0, 3.0]
            .iter()
            .map(|t| e.step(*t, None).unwrap())
            .collect();
        assert!(outs.iter().all(|o| o.reinforced));
        assert_eq!(
            outs[0].meta.get("aversive_event"),
            Some(&MetaValue::Str("shock_onset".into()))
        );
        for o in &outs[1..] {
            assert_eq!(
                o.meta.get("aversive_event"),
                Some(&MetaValue::Str("shock".into()))
            );
        }
    }

    #[test]
    fn escape_response_terminates_shock() {
        let mut e = Escape::new(10.0, 3.0, 1.0).unwrap();
        e.step(0.0, None).unwrap();
        e.step(1.0, None).unwrap();
        let r = ev(2.0);
        let o = e.step(2.0, Some(&r)).unwrap();
        assert!(!o.reinforced);
        assert_eq!(
            o.meta.get("aversive_event"),
            Some(&MetaValue::Str("shock_termination".into()))
        );
        assert_eq!(
            o.meta.get("trial_outcome"),
            Some(&MetaValue::Str("escape".into()))
        );
        assert_eq!(e.phase(), "iti");
    }

    #[test]
    fn escape_trial_duration_bound_on_failure() {
        let mut e = Escape::new(3.0, 2.0, 1.0).unwrap();
        e.step(0.0, None).unwrap();
        e.step(1.0, None).unwrap();
        e.step(2.0, None).unwrap();
        let o = e.step(3.0, None).unwrap();
        assert!(!o.reinforced);
        assert_eq!(
            o.meta.get("aversive_event"),
            Some(&MetaValue::Str("trial_end".into()))
        );
        assert_eq!(
            o.meta.get("trial_outcome"),
            Some(&MetaValue::Str("failed".into()))
        );
        assert_eq!(e.phase(), "iti");
    }

    #[test]
    fn escape_iti_then_new_trial() {
        let mut e = Escape::new(10.0, 3.0, 1.0).unwrap();
        e.step(0.0, None).unwrap();
        let r = ev(1.0);
        e.step(1.0, Some(&r)).unwrap();
        let o_iti = e.step(3.0, None).unwrap();
        assert_eq!(
            o_iti.meta.get("phase"),
            Some(&MetaValue::Str("iti".into()))
        );
        assert!(!o_iti.reinforced);
        let o_new = e.step(4.0, None).unwrap();
        assert!(o_new.reinforced);
        assert_eq!(
            o_new.meta.get("aversive_event"),
            Some(&MetaValue::Str("shock_onset".into()))
        );
    }

    #[test]
    fn escape_reset() {
        let mut e = Escape::new(5.0, 2.0, 1.0).unwrap();
        e.step(0.0, None).unwrap();
        let r = ev(1.0);
        e.step(1.0, Some(&r)).unwrap();
        e.reset();
        assert_eq!(e.phase(), "shock");
        let o = e.step(0.0, None).unwrap();
        assert!(o.reinforced);
        assert_eq!(
            o.meta.get("aversive_event"),
            Some(&MetaValue::Str("shock_onset".into()))
        );
    }

    #[test]
    fn escape_non_monotonic_raises() {
        let mut e = Escape::new(5.0, 2.0, 1.0).unwrap();
        e.step(1.0, None).unwrap();
        assert!(e.step(0.5, None).is_err());
    }

    #[test]
    fn escape_event_time_mismatch_raises() {
        let mut e = Escape::new(5.0, 2.0, 1.0).unwrap();
        let r = ev(0.5);
        assert!(e.step(1.0, Some(&r)).is_err());
    }

    #[test]
    fn escape_custom_magnitude() {
        let mut e = Escape::new(5.0, 2.0, 2.5).unwrap();
        let o = e.step(0.0, None).unwrap();
        assert_eq!(o.reinforcer.as_ref().unwrap().magnitude, -2.5);
    }
}

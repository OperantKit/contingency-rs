//! Trial-based (discrete-trial) schedules — MTS and Go/NoGo.
//!
//! Rust port of `contingency.schedules.trial_based`. Implements two
//! canonical trial-based paradigms:
//!
//! * [`MatchingToSample`] (MTS) — after a sample stimulus is presented,
//!   the subject chooses among `N` comparison stimuli; the choice matching
//!   the sample is correct.
//! * [`GoNoGo`] — on each trial a Go or NoGo stimulus is presented;
//!   responding on Go is correct, withholding on NoGo is correct.
//!
//! Both drive two kinds of input: ticks (`step(now, None)`) that advance
//! the internal clock, and response events evaluated during
//! response-eligible phases. During ITI and sample presentation responses
//! are ignored but ticks drive phase transitions.
//!
//! # References
//!
//! Cumming, W. W., & Berryman, R. (1965). The complex discriminated
//! operant: Studies of matching-to-sample and related problems. In D. I.
//! Mostofsky (Ed.), *Stimulus Generalization* (pp. 284-330). Stanford
//! University Press.
//!
//! Nevin, J. A. (1969). Signal detection theory and operant behavior.
//! *Journal of the Experimental Analysis of Behavior*, 12(3), 475-480.
//! <https://doi.org/10.1901/jeab.1969.12-475>

use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};

use crate::constants::TIME_TOL;
use crate::errors::ContingencyError;
use crate::helpers::checks::{check_event, check_time};
use crate::schedule::Schedule;
use crate::types::{MetaValue, Outcome, Reinforcer, ResponseEvent};
use crate::Result;

fn require_nonneg(name: &str, value: f64) -> Result<f64> {
    if !value.is_finite() || value < 0.0 {
        return Err(ContingencyError::Config(format!(
            "{name} must be >= 0, got {value}"
        )));
    }
    Ok(value)
}

fn require_positive(name: &str, value: f64) -> Result<f64> {
    if !value.is_finite() || value <= 0.0 {
        return Err(ContingencyError::Config(format!(
            "{name} must be > 0, got {value}"
        )));
    }
    Ok(value)
}

fn make_rng(seed: Option<u64>) -> SmallRng {
    match seed {
        Some(s) => SmallRng::seed_from_u64(s),
        None => SmallRng::from_entropy(),
    }
}

// ---------------------------------------------------------------------------
// MatchingToSample
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MtsPhase {
    Sample,
    Choice,
    Iti,
}

/// Matching-to-sample (MTS) discrete-trial schedule.
///
/// A trial unfolds as `sample -> choice -> iti`. Comparison stimuli are
/// exposed as operanda `choice_0 ... choice_{N-1}`; exactly one is
/// designated correct (sampled uniformly at random at trial start).
///
/// # References
///
/// Cumming, W. W., & Berryman, R. (1965). The complex discriminated
/// operant: Studies of matching-to-sample and related problems. In
/// D. I. Mostofsky (Ed.), *Stimulus Generalization* (pp. 284-330).
/// Stanford University Press.
pub struct MatchingToSample {
    n_comparisons: u32,
    sample_duration: f64,
    choice_timeout: f64,
    iti: f64,
    consequence: Option<Box<dyn Schedule>>,
    incorrect: Option<Box<dyn Schedule>>,
    seed: Option<u64>,
    rng: SmallRng,
    phase: MtsPhase,
    phase_started: Option<f64>,
    trial_index: u64,
    correct_operandum: String,
    last_now: Option<f64>,
    pending_outcome_label: Option<&'static str>,
}

impl std::fmt::Debug for MatchingToSample {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MatchingToSample")
            .field("n_comparisons", &self.n_comparisons)
            .field("sample_duration", &self.sample_duration)
            .field("choice_timeout", &self.choice_timeout)
            .field("iti", &self.iti)
            .field("phase", &self.phase)
            .field("trial_index", &self.trial_index)
            .field("correct_operandum", &self.correct_operandum)
            .finish()
    }
}

impl MatchingToSample {
    /// Construct a matching-to-sample schedule.
    pub fn new(
        n_comparisons: u32,
        sample_duration: f64,
        choice_timeout: f64,
        consequence: Option<Box<dyn Schedule>>,
        incorrect: Option<Box<dyn Schedule>>,
        iti: f64,
        seed: Option<u64>,
    ) -> Result<Self> {
        if n_comparisons < 2 {
            return Err(ContingencyError::Config(format!(
                "n_comparisons must be >= 2, got {n_comparisons}"
            )));
        }
        let sd = require_nonneg("sample_duration", sample_duration)?;
        let ct = require_positive("choice_timeout", choice_timeout)?;
        let it = require_nonneg("iti", iti)?;
        let mut rng = make_rng(seed);
        let correct = format!("choice_{}", rng.gen_range(0..n_comparisons));
        let phase = if sd > 0.0 {
            MtsPhase::Sample
        } else {
            MtsPhase::Choice
        };
        Ok(Self {
            n_comparisons,
            sample_duration: sd,
            choice_timeout: ct,
            iti: it,
            consequence,
            incorrect,
            seed,
            rng,
            phase,
            phase_started: None,
            trial_index: 0,
            correct_operandum: correct,
            last_now: None,
            pending_outcome_label: None,
        })
    }

    /// Number of comparison stimuli per trial.
    pub fn n_comparisons(&self) -> u32 {
        self.n_comparisons
    }

    /// Current 0-based trial index.
    pub fn trial_index(&self) -> u64 {
        self.trial_index
    }

    /// Current phase as a string (`"sample"`, `"choice"`, or `"iti"`).
    pub fn phase(&self) -> &'static str {
        match self.phase {
            MtsPhase::Sample => "sample",
            MtsPhase::Choice => "choice",
            MtsPhase::Iti => "iti",
        }
    }

    /// Operandum designated as correct for the current trial.
    pub fn correct_operandum(&self) -> &str {
        &self.correct_operandum
    }

    /// Inter-trial interval.
    pub fn iti(&self) -> f64 {
        self.iti
    }

    fn pick_correct(&mut self) -> String {
        let idx = self.rng.gen_range(0..self.n_comparisons);
        format!("choice_{idx}")
    }

    fn begin_trial(&mut self, now: f64) {
        self.phase = if self.sample_duration > 0.0 {
            MtsPhase::Sample
        } else {
            MtsPhase::Choice
        };
        self.phase_started = Some(now);
        self.correct_operandum = self.pick_correct();
    }

    fn enter_choice(&mut self, now: f64) {
        self.phase = MtsPhase::Choice;
        self.phase_started = Some(now);
    }

    fn enter_iti(&mut self, now: f64, label: &'static str) {
        self.phase = MtsPhase::Iti;
        self.phase_started = Some(now);
        self.pending_outcome_label = Some(label);
    }

    fn finish_iti(&mut self, now: f64) {
        self.trial_index += 1;
        self.begin_trial(now);
    }

    fn phase_only_outcome(&mut self) -> Outcome {
        let mut out = Outcome::empty();
        out.meta
            .insert("phase".into(), MetaValue::Str(self.phase().into()));
        out.meta.insert(
            "trial_index".into(),
            MetaValue::Int(self.trial_index as i64),
        );
        if let Some(lbl) = self.pending_outcome_label.take() {
            out.meta
                .insert("trial_outcome".into(), MetaValue::Str(lbl.into()));
        }
        out
    }

    fn incorrect_timeout_outcome(&mut self, now: f64) -> Result<Outcome> {
        if self.incorrect.is_none() {
            return Ok(self.phase_only_outcome());
        }
        let sub = self.incorrect.as_mut().unwrap().step(now, None)?;
        self.pending_outcome_label = None;
        let mut meta = std::collections::BTreeMap::new();
        meta.insert("phase".into(), MetaValue::Str("iti".into()));
        meta.insert(
            "trial_index".into(),
            MetaValue::Int(self.trial_index as i64),
        );
        meta.insert(
            "trial_outcome".into(),
            MetaValue::Str("incorrect".into()),
        );
        Ok(Outcome {
            reinforced: sub.reinforced,
            reinforcer: sub.reinforcer,
            meta,
        })
    }

    fn handle_choice(&mut self, now: f64, event: &ResponseEvent) -> Result<Outcome> {
        let correct = event.operandum == self.correct_operandum;
        let label = if correct { "correct" } else { "incorrect" };
        self.enter_iti(now, label);
        self.pending_outcome_label = None;

        let use_consequence = correct;
        let sched_slot = if use_consequence {
            &mut self.consequence
        } else {
            &mut self.incorrect
        };

        if correct && sched_slot.is_none() {
            let mut meta = std::collections::BTreeMap::new();
            meta.insert("phase".into(), MetaValue::Str("iti".into()));
            meta.insert(
                "trial_index".into(),
                MetaValue::Int(self.trial_index as i64),
            );
            meta.insert(
                "trial_outcome".into(),
                MetaValue::Str(label.into()),
            );
            return Ok(Outcome {
                reinforced: true,
                reinforcer: Some(Reinforcer::at(now)),
                meta,
            });
        }
        if sched_slot.is_none() {
            let mut meta = std::collections::BTreeMap::new();
            meta.insert("phase".into(), MetaValue::Str("iti".into()));
            meta.insert(
                "trial_index".into(),
                MetaValue::Int(self.trial_index as i64),
            );
            meta.insert(
                "trial_outcome".into(),
                MetaValue::Str(label.into()),
            );
            return Ok(Outcome {
                reinforced: false,
                reinforcer: None,
                meta,
            });
        }
        let sub = sched_slot.as_mut().unwrap().step(now, Some(event))?;
        let mut meta = std::collections::BTreeMap::new();
        meta.insert("phase".into(), MetaValue::Str("iti".into()));
        meta.insert(
            "trial_index".into(),
            MetaValue::Int(self.trial_index as i64),
        );
        meta.insert(
            "trial_outcome".into(),
            MetaValue::Str(label.into()),
        );
        Ok(Outcome {
            reinforced: sub.reinforced,
            reinforcer: sub.reinforcer,
            meta,
        })
    }
}

impl Schedule for MatchingToSample {
    fn step(&mut self, now: f64, event: Option<&ResponseEvent>) -> Result<Outcome> {
        check_time(now, self.last_now)?;
        check_event(now, event)?;
        self.last_now = Some(now);

        if self.phase_started.is_none() {
            self.phase_started = Some(now);
        }

        loop {
            let mut progressed = false;
            match self.phase {
                MtsPhase::Sample => {
                    let started = self.phase_started.unwrap();
                    if now - started >= self.sample_duration - TIME_TOL {
                        self.enter_choice(started + self.sample_duration);
                        progressed = true;
                    }
                }
                MtsPhase::Choice => {
                    let started = self.phase_started.unwrap();
                    if now - started > self.choice_timeout + TIME_TOL {
                        // Timeout scored incorrect.
                        self.enter_iti(
                            started + self.choice_timeout,
                            "incorrect",
                        );
                        return self.incorrect_timeout_outcome(now);
                    }
                }
                MtsPhase::Iti => {
                    let started = self.phase_started.unwrap();
                    if now - started >= self.iti - TIME_TOL {
                        self.finish_iti(started + self.iti);
                        progressed = true;
                    }
                }
            }
            if !progressed {
                break;
            }
        }

        let Some(ev) = event else {
            return Ok(self.phase_only_outcome());
        };

        if matches!(self.phase, MtsPhase::Choice) {
            return self.handle_choice(now, ev);
        }

        Ok(self.phase_only_outcome())
    }

    fn reset(&mut self) {
        self.rng = make_rng(self.seed);
        self.phase = if self.sample_duration > 0.0 {
            MtsPhase::Sample
        } else {
            MtsPhase::Choice
        };
        self.phase_started = None;
        self.trial_index = 0;
        let idx = self.rng.gen_range(0..self.n_comparisons);
        self.correct_operandum = format!("choice_{idx}");
        self.last_now = None;
        self.pending_outcome_label = None;
        if let Some(c) = self.consequence.as_mut() {
            c.reset();
        }
        if let Some(i) = self.incorrect.as_mut() {
            i.reset();
        }
    }
}

// ---------------------------------------------------------------------------
// GoNoGo
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GngPhase {
    Go,
    NoGo,
    Iti,
}

/// Go/NoGo discrimination schedule.
///
/// # References
///
/// Nevin, J. A. (1969). Signal detection theory and operant behavior.
/// *Journal of the Experimental Analysis of Behavior*, 12(3), 475-480.
/// <https://doi.org/10.1901/jeab.1969.12-475>
pub struct GoNoGo {
    go_probability: f64,
    response_window: f64,
    iti: f64,
    correct_go_schedule: Option<Box<dyn Schedule>>,
    correct_nogo_schedule: Option<Box<dyn Schedule>>,
    false_alarm: Option<Box<dyn Schedule>>,
    seed: Option<u64>,
    rng: SmallRng,
    phase: GngPhase,
    phase_started: Option<f64>,
    trial_index: u64,
    current_is_go: bool,
    last_now: Option<f64>,
    pending_outcome_label: Option<&'static str>,
}

impl std::fmt::Debug for GoNoGo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GoNoGo")
            .field("go_probability", &self.go_probability)
            .field("response_window", &self.response_window)
            .field("iti", &self.iti)
            .field("phase", &self.phase)
            .field("trial_index", &self.trial_index)
            .field("current_is_go", &self.current_is_go)
            .finish()
    }
}

impl GoNoGo {
    /// Construct a Go/NoGo schedule.
    pub fn new(
        go_probability: f64,
        response_window: f64,
        iti: f64,
        correct_go_schedule: Option<Box<dyn Schedule>>,
        correct_nogo_schedule: Option<Box<dyn Schedule>>,
        false_alarm: Option<Box<dyn Schedule>>,
        seed: Option<u64>,
    ) -> Result<Self> {
        if !(go_probability.is_finite() && go_probability > 0.0 && go_probability < 1.0) {
            return Err(ContingencyError::Config(format!(
                "go_probability must satisfy 0 < p < 1, got {go_probability}"
            )));
        }
        let rw = require_positive("response_window", response_window)?;
        let it = require_nonneg("iti", iti)?;
        let mut rng = make_rng(seed);
        let current_is_go = rng.gen::<f64>() < go_probability;
        let phase = if current_is_go {
            GngPhase::Go
        } else {
            GngPhase::NoGo
        };
        Ok(Self {
            go_probability,
            response_window: rw,
            iti: it,
            correct_go_schedule,
            correct_nogo_schedule,
            false_alarm,
            seed,
            rng,
            phase,
            phase_started: None,
            trial_index: 0,
            current_is_go,
            last_now: None,
            pending_outcome_label: None,
        })
    }

    /// Probability a trial is Go.
    pub fn go_probability(&self) -> f64 {
        self.go_probability
    }

    /// Response-eligibility window duration.
    pub fn response_window(&self) -> f64 {
        self.response_window
    }

    /// Inter-trial interval.
    pub fn iti(&self) -> f64 {
        self.iti
    }

    /// Current 0-based trial index.
    pub fn trial_index(&self) -> u64 {
        self.trial_index
    }

    /// Whether the currently armed trial is a Go trial.
    pub fn current_is_go(&self) -> bool {
        self.current_is_go
    }

    /// Current phase (`"go"`, `"nogo"`, or `"iti"`).
    pub fn phase(&self) -> &'static str {
        match self.phase {
            GngPhase::Go => "go",
            GngPhase::NoGo => "nogo",
            GngPhase::Iti => "iti",
        }
    }

    fn draw_trial_type(&mut self) -> bool {
        self.rng.gen::<f64>() < self.go_probability
    }

    fn begin_trial(&mut self, now: f64) {
        self.current_is_go = self.draw_trial_type();
        self.phase = if self.current_is_go {
            GngPhase::Go
        } else {
            GngPhase::NoGo
        };
        self.phase_started = Some(now);
    }

    fn enter_iti(&mut self, now: f64, label: &'static str) {
        self.phase = GngPhase::Iti;
        self.phase_started = Some(now);
        self.pending_outcome_label = Some(label);
    }

    fn finish_iti(&mut self, now: f64) {
        self.trial_index += 1;
        self.begin_trial(now);
    }

    fn phase_only_outcome(&mut self) -> Outcome {
        let mut out = Outcome::empty();
        out.meta
            .insert("phase".into(), MetaValue::Str(self.phase().into()));
        out.meta.insert(
            "trial_index".into(),
            MetaValue::Int(self.trial_index as i64),
        );
        if let Some(lbl) = self.pending_outcome_label.take() {
            out.meta
                .insert("trial_outcome".into(), MetaValue::Str(lbl.into()));
        }
        out
    }

    fn handle_response(&mut self, now: f64, event: &ResponseEvent) -> Result<Outcome> {
        let is_go = self.current_is_go;
        let label = if is_go { "correct" } else { "false_alarm" };
        let phase_snapshot = self.phase();
        self.enter_iti(now, label);
        self.pending_outcome_label = None;

        let target_slot = if is_go {
            &mut self.correct_go_schedule
        } else {
            &mut self.false_alarm
        };

        if is_go && target_slot.is_none() {
            let mut meta = std::collections::BTreeMap::new();
            meta.insert(
                "phase".into(),
                MetaValue::Str(phase_snapshot.into()),
            );
            meta.insert(
                "trial_index".into(),
                MetaValue::Int(self.trial_index as i64),
            );
            meta.insert(
                "trial_outcome".into(),
                MetaValue::Str(label.into()),
            );
            return Ok(Outcome {
                reinforced: true,
                reinforcer: Some(Reinforcer::at(now)),
                meta,
            });
        }
        if target_slot.is_none() {
            let mut meta = std::collections::BTreeMap::new();
            meta.insert(
                "phase".into(),
                MetaValue::Str(phase_snapshot.into()),
            );
            meta.insert(
                "trial_index".into(),
                MetaValue::Int(self.trial_index as i64),
            );
            meta.insert(
                "trial_outcome".into(),
                MetaValue::Str(label.into()),
            );
            return Ok(Outcome {
                reinforced: false,
                reinforcer: None,
                meta,
            });
        }
        let sub = target_slot.as_mut().unwrap().step(now, Some(event))?;
        let mut meta = std::collections::BTreeMap::new();
        meta.insert(
            "phase".into(),
            MetaValue::Str(phase_snapshot.into()),
        );
        meta.insert(
            "trial_index".into(),
            MetaValue::Int(self.trial_index as i64),
        );
        meta.insert("trial_outcome".into(), MetaValue::Str(label.into()));
        Ok(Outcome {
            reinforced: sub.reinforced,
            reinforcer: sub.reinforcer,
            meta,
        })
    }
}

impl Schedule for GoNoGo {
    fn step(&mut self, now: f64, event: Option<&ResponseEvent>) -> Result<Outcome> {
        check_time(now, self.last_now)?;
        check_event(now, event)?;
        self.last_now = Some(now);

        if self.phase_started.is_none() {
            self.phase_started = Some(now);
        }

        let mut resolution_outcome: Option<Outcome> = None;
        loop {
            let mut progressed = false;
            match self.phase {
                GngPhase::Go | GngPhase::NoGo => {
                    let started = self.phase_started.unwrap();
                    if now - started >= self.response_window - TIME_TOL {
                        let boundary = started + self.response_window;
                        if self.current_is_go {
                            self.enter_iti(boundary, "incorrect");
                        } else {
                            // NoGo withhold.
                            if let Some(sched) = self.correct_nogo_schedule.as_mut() {
                                let sub = sched.step(boundary, None)?;
                                let trial_idx = self.trial_index;
                                let mut meta = std::collections::BTreeMap::new();
                                meta.insert(
                                    "phase".into(),
                                    MetaValue::Str("nogo".into()),
                                );
                                meta.insert(
                                    "trial_index".into(),
                                    MetaValue::Int(trial_idx as i64),
                                );
                                meta.insert(
                                    "trial_outcome".into(),
                                    MetaValue::Str("correct_withhold".into()),
                                );
                                resolution_outcome = Some(Outcome {
                                    reinforced: sub.reinforced,
                                    reinforcer: sub.reinforcer,
                                    meta,
                                });
                            }
                            self.enter_iti(boundary, "correct_withhold");
                        }
                        progressed = true;
                    }
                }
                GngPhase::Iti => {
                    let started = self.phase_started.unwrap();
                    if now - started >= self.iti - TIME_TOL {
                        self.finish_iti(started + self.iti);
                        progressed = true;
                    }
                }
            }
            if !progressed {
                break;
            }
        }

        if let Some(out) = resolution_outcome {
            if event.is_none() {
                self.pending_outcome_label = None;
                return Ok(out);
            }
        }

        let Some(ev) = event else {
            return Ok(self.phase_only_outcome());
        };

        if matches!(self.phase, GngPhase::Go | GngPhase::NoGo) {
            return self.handle_response(now, ev);
        }

        Ok(self.phase_only_outcome())
    }

    fn reset(&mut self) {
        self.rng = make_rng(self.seed);
        self.phase_started = None;
        self.trial_index = 0;
        self.current_is_go = self.draw_trial_type();
        self.phase = if self.current_is_go {
            GngPhase::Go
        } else {
            GngPhase::NoGo
        };
        self.last_now = None;
        self.pending_outcome_label = None;
        if let Some(s) = self.correct_go_schedule.as_mut() {
            s.reset();
        }
        if let Some(s) = self.correct_nogo_schedule.as_mut() {
            s.reset();
        }
        if let Some(s) = self.false_alarm.as_mut() {
            s.reset();
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schedules::{FR, FT};

    fn ev(op: &str, now: f64) -> ResponseEvent {
        ResponseEvent::on(op, now)
    }

    fn main_ev(now: f64) -> ResponseEvent {
        ResponseEvent::new(now)
    }

    fn find_go_seed() -> u64 {
        for s in 0u64..500 {
            let g = GoNoGo::new(0.5, 2.0, 1.0, None, None, None, Some(s)).unwrap();
            if g.current_is_go() {
                return s;
            }
        }
        panic!("no go seed found");
    }

    fn find_nogo_seed() -> u64 {
        for s in 0u64..500 {
            let g = GoNoGo::new(0.5, 2.0, 1.0, None, None, None, Some(s)).unwrap();
            if !g.current_is_go() {
                return s;
            }
        }
        panic!("no nogo seed found");
    }

    // ---- MTS construction -----------------------------------------------

    #[test]
    fn mts_rejects_n_below_two() {
        assert!(MatchingToSample::new(1, 0.0, 5.0, None, None, 1.0, None).is_err());
    }

    #[test]
    fn mts_rejects_negative_sample_duration() {
        assert!(MatchingToSample::new(2, -1.0, 5.0, None, None, 1.0, None).is_err());
    }

    #[test]
    fn mts_rejects_zero_choice_timeout() {
        assert!(MatchingToSample::new(2, 0.0, 0.0, None, None, 1.0, None).is_err());
    }

    #[test]
    fn mts_rejects_negative_iti() {
        assert!(MatchingToSample::new(2, 0.0, 5.0, None, None, -1.0, None).is_err());
    }

    #[test]
    fn mts_construction_ok() {
        let mts = MatchingToSample::new(3, 0.5, 2.0, None, None, 1.0, Some(7)).unwrap();
        assert_eq!(mts.n_comparisons(), 3);
        assert_eq!(mts.trial_index(), 0);
        assert_eq!(mts.iti(), 1.0);
        assert!(mts.correct_operandum().starts_with("choice_"));
    }

    // ---- MTS sample phase ------------------------------------------------

    #[test]
    fn mts_response_ignored_during_sample() {
        let mut mts = MatchingToSample::new(2, 1.0, 2.0, None, None, 1.0, Some(1)).unwrap();
        let correct = mts.correct_operandum().to_string();
        let r = ev(&correct, 0.1);
        let o = mts.step(0.1, Some(&r)).unwrap();
        assert_eq!(
            o.meta.get("phase"),
            Some(&MetaValue::Str("sample".into()))
        );
        assert!(!o.reinforced);
        assert!(!o.meta.contains_key("trial_outcome"));
    }

    #[test]
    fn mts_tick_advances_into_choice() {
        let mut mts = MatchingToSample::new(2, 1.0, 2.0, None, None, 1.0, Some(1)).unwrap();
        mts.step(0.0, None).unwrap();
        let o = mts.step(1.5, None).unwrap();
        assert_eq!(
            o.meta.get("phase"),
            Some(&MetaValue::Str("choice".into()))
        );
    }

    // ---- MTS choice phase ------------------------------------------------

    #[test]
    fn mts_correct_choice_defaults_to_crf() {
        let mut mts =
            MatchingToSample::new(3, 0.0, 5.0, None, None, 1.0, Some(123)).unwrap();
        let correct = mts.correct_operandum().to_string();
        let r = ev(&correct, 0.0);
        let o = mts.step(0.0, Some(&r)).unwrap();
        assert!(o.reinforced);
        assert_eq!(
            o.meta.get("trial_outcome"),
            Some(&MetaValue::Str("correct".into()))
        );
        assert_eq!(o.reinforcer.as_ref().unwrap().time, 0.0);
    }

    #[test]
    fn mts_incorrect_choice_no_consequence() {
        let mut mts =
            MatchingToSample::new(3, 0.0, 5.0, None, None, 1.0, Some(123)).unwrap();
        let correct = mts.correct_operandum().to_string();
        let other = (0..3)
            .map(|i| format!("choice_{i}"))
            .find(|s| *s != correct)
            .unwrap();
        let r = ev(&other, 0.0);
        let o = mts.step(0.0, Some(&r)).unwrap();
        assert!(!o.reinforced);
        assert_eq!(
            o.meta.get("trial_outcome"),
            Some(&MetaValue::Str("incorrect".into()))
        );
    }

    #[test]
    fn mts_consequence_schedule_passthrough() {
        let consequence: Box<dyn Schedule> = Box::new(FR::new(1).unwrap());
        let mut mts = MatchingToSample::new(
            2,
            0.0,
            5.0,
            Some(consequence),
            None,
            1.0,
            Some(42),
        )
        .unwrap();
        let correct = mts.correct_operandum().to_string();
        let r = ev(&correct, 0.0);
        let o = mts.step(0.0, Some(&r)).unwrap();
        assert!(o.reinforced);
        assert_eq!(
            o.meta.get("trial_outcome"),
            Some(&MetaValue::Str("correct".into()))
        );
    }

    #[test]
    fn mts_choice_timeout_scored_incorrect() {
        let mut mts =
            MatchingToSample::new(2, 0.0, 3.0, None, None, 1.0, Some(1)).unwrap();
        mts.step(0.0, None).unwrap();
        let o = mts.step(3.5, None).unwrap();
        assert_eq!(
            o.meta.get("trial_outcome"),
            Some(&MetaValue::Str("incorrect".into()))
        );
    }

    #[test]
    fn mts_timeout_with_incorrect_schedule() {
        let incorrect: Box<dyn Schedule> = Box::new(FT::new(1.0).unwrap());
        let mut mts = MatchingToSample::new(
            2,
            0.0,
            1.0,
            None,
            Some(incorrect),
            1.0,
            Some(1),
        )
        .unwrap();
        mts.step(0.0, None).unwrap();
        let o = mts.step(2.0, None).unwrap();
        assert_eq!(
            o.meta.get("trial_outcome"),
            Some(&MetaValue::Str("incorrect".into()))
        );
    }

    // ---- MTS ITI ---------------------------------------------------------

    #[test]
    fn mts_iti_transitions_to_next_trial() {
        let mut mts =
            MatchingToSample::new(2, 0.0, 5.0, None, None, 1.0, Some(1)).unwrap();
        let correct = mts.correct_operandum().to_string();
        let r = ev(&correct, 0.0);
        mts.step(0.0, Some(&r)).unwrap();
        // ITI: response ignored.
        let r2 = ev("choice_0", 0.5);
        let o = mts.step(0.5, Some(&r2)).unwrap();
        assert_eq!(
            o.meta.get("phase"),
            Some(&MetaValue::Str("iti".into()))
        );
        let o = mts.step(1.2, None).unwrap();
        assert_eq!(
            o.meta.get("phase"),
            Some(&MetaValue::Str("choice".into()))
        );
        assert_eq!(mts.trial_index(), 1);
    }

    #[test]
    fn mts_iti_response_ignored() {
        let mut mts =
            MatchingToSample::new(2, 0.0, 5.0, None, None, 2.0, Some(1)).unwrap();
        let correct = mts.correct_operandum().to_string();
        let r = ev(&correct, 0.0);
        mts.step(0.0, Some(&r)).unwrap();
        let r2 = ev("choice_0", 0.5);
        let o = mts.step(0.5, Some(&r2)).unwrap();
        assert_eq!(
            o.meta.get("phase"),
            Some(&MetaValue::Str("iti".into()))
        );
        assert!(!o.reinforced);
    }

    // ---- MTS seeding / reset --------------------------------------------

    #[test]
    fn mts_seeded_determinism() {
        let m1 = MatchingToSample::new(5, 0.0, 5.0, None, None, 1.0, Some(999)).unwrap();
        let m2 = MatchingToSample::new(5, 0.0, 5.0, None, None, 1.0, Some(999)).unwrap();
        assert_eq!(m1.correct_operandum(), m2.correct_operandum());
    }

    #[test]
    fn mts_reset_returns_to_trial_zero() {
        let mut mts =
            MatchingToSample::new(2, 0.0, 5.0, None, None, 1.0, Some(3)).unwrap();
        let original = mts.correct_operandum().to_string();
        let r = ev(&original, 0.0);
        mts.step(0.0, Some(&r)).unwrap();
        mts.step(2.0, None).unwrap();
        assert_eq!(mts.trial_index(), 1);
        mts.reset();
        assert_eq!(mts.trial_index(), 0);
        assert_eq!(mts.correct_operandum(), original);
    }

    // ---- MTS monotonicity ------------------------------------------------

    #[test]
    fn mts_non_monotonic_rejected() {
        let mut mts =
            MatchingToSample::new(2, 0.0, 5.0, None, None, 1.0, Some(1)).unwrap();
        mts.step(1.0, None).unwrap();
        assert!(mts.step(0.5, None).is_err());
    }

    #[test]
    fn mts_event_time_mismatch_rejected() {
        let mut mts =
            MatchingToSample::new(2, 0.0, 5.0, None, None, 1.0, Some(1)).unwrap();
        let r = ev("choice_0", 2.0);
        assert!(mts.step(1.0, Some(&r)).is_err());
    }

    // ---- GoNoGo construction --------------------------------------------

    #[test]
    fn gonogo_rejects_out_of_range_probability() {
        assert!(GoNoGo::new(0.0, 2.0, 1.0, None, None, None, None).is_err());
        assert!(GoNoGo::new(1.0, 2.0, 1.0, None, None, None, None).is_err());
    }

    #[test]
    fn gonogo_rejects_nonpositive_response_window() {
        assert!(GoNoGo::new(0.5, 0.0, 1.0, None, None, None, None).is_err());
    }

    #[test]
    fn gonogo_rejects_negative_iti() {
        assert!(GoNoGo::new(0.5, 2.0, -1.0, None, None, None, None).is_err());
    }

    // ---- GoNoGo Go trial -------------------------------------------------

    #[test]
    fn gonogo_go_response_default_crf() {
        let seed = find_go_seed();
        let mut g =
            GoNoGo::new(0.5, 2.0, 1.0, None, None, None, Some(seed)).unwrap();
        assert!(g.current_is_go());
        let r = main_ev(0.5);
        let o = g.step(0.5, Some(&r)).unwrap();
        assert!(o.reinforced);
        assert_eq!(
            o.meta.get("trial_outcome"),
            Some(&MetaValue::Str("correct".into()))
        );
    }

    #[test]
    fn gonogo_go_no_response_miss() {
        let seed = find_go_seed();
        let mut g =
            GoNoGo::new(0.5, 2.0, 1.0, None, None, None, Some(seed)).unwrap();
        g.step(0.0, None).unwrap();
        let o = g.step(2.5, None).unwrap();
        assert!(!o.reinforced);
        assert_eq!(
            o.meta.get("trial_outcome"),
            Some(&MetaValue::Str("incorrect".into()))
        );
    }

    // ---- GoNoGo NoGo trial ----------------------------------------------

    #[test]
    fn gonogo_nogo_response_is_false_alarm() {
        let seed = find_nogo_seed();
        let mut g =
            GoNoGo::new(0.5, 2.0, 1.0, None, None, None, Some(seed)).unwrap();
        let r = main_ev(0.5);
        let o = g.step(0.5, Some(&r)).unwrap();
        assert!(!o.reinforced);
        assert_eq!(
            o.meta.get("trial_outcome"),
            Some(&MetaValue::Str("false_alarm".into()))
        );
    }

    #[test]
    fn gonogo_nogo_response_fires_false_alarm_schedule() {
        let mut seed = None;
        for s in 0u64..500 {
            let fa: Box<dyn Schedule> = Box::new(FR::new(1).unwrap());
            let g = GoNoGo::new(0.5, 2.0, 1.0, None, None, Some(fa), Some(s)).unwrap();
            if !g.current_is_go() {
                seed = Some(s);
                break;
            }
        }
        let seed = seed.expect("no nogo seed");
        let fa: Box<dyn Schedule> = Box::new(FR::new(1).unwrap());
        let mut g =
            GoNoGo::new(0.5, 2.0, 1.0, None, None, Some(fa), Some(seed)).unwrap();
        let r = main_ev(0.5);
        let o = g.step(0.5, Some(&r)).unwrap();
        assert!(o.reinforced);
        assert_eq!(
            o.meta.get("trial_outcome"),
            Some(&MetaValue::Str("false_alarm".into()))
        );
    }

    #[test]
    fn gonogo_nogo_withhold_label() {
        let seed = find_nogo_seed();
        let mut g =
            GoNoGo::new(0.5, 2.0, 1.0, None, None, None, Some(seed)).unwrap();
        g.step(0.0, None).unwrap();
        let o = g.step(2.5, None).unwrap();
        assert_eq!(
            o.meta.get("trial_outcome"),
            Some(&MetaValue::Str("correct_withhold".into()))
        );
        assert!(!o.reinforced);
    }

    // ---- GoNoGo distribution / seeding / monotonicity -------------------

    #[test]
    fn gonogo_probability_distribution() {
        let trials = 2000u32;
        let p = 0.7f64;
        let mut go_count = 0u32;
        let mut g =
            GoNoGo::new(p, 1.0, 0.0, None, None, None, Some(42)).unwrap();
        let mut now = 0.0;
        for _ in 0..trials {
            if g.current_is_go() {
                go_count += 1;
            }
            now += 1.0;
            g.step(now, None).unwrap();
        }
        let freq = go_count as f64 / trials as f64;
        assert!((freq - p).abs() < 0.05, "freq={freq}, p={p}");
    }

    #[test]
    fn gonogo_seeded_determinism() {
        let g1 = GoNoGo::new(0.5, 2.0, 1.0, None, None, None, Some(777)).unwrap();
        let g2 = GoNoGo::new(0.5, 2.0, 1.0, None, None, None, Some(777)).unwrap();
        assert_eq!(g1.current_is_go(), g2.current_is_go());
    }

    #[test]
    fn gonogo_reset_returns_to_trial_zero() {
        let mut g =
            GoNoGo::new(0.5, 1.0, 0.5, None, None, None, Some(3)).unwrap();
        g.step(0.0, None).unwrap();
        g.step(2.0, None).unwrap();
        assert!(g.trial_index() >= 1);
        g.reset();
        assert_eq!(g.trial_index(), 0);
    }

    #[test]
    fn gonogo_non_monotonic_rejected() {
        let mut g =
            GoNoGo::new(0.5, 2.0, 1.0, None, None, None, Some(1)).unwrap();
        g.step(1.0, None).unwrap();
        assert!(g.step(0.5, None).is_err());
    }
}

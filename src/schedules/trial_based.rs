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

#[allow(unused_imports)]
use crate::prelude::*;

use crate::helpers::py_random::PyRandom;

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

fn make_rng(seed: Option<u64>) -> PyRandom {
    PyRandom::from_optional_seed(seed)
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

/// One MTS trial type: a sample, its correct comparison, and the full
/// comparison set (exactly `n_comparisons` distinct stimuli).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrialType {
    /// Sample stimulus name.
    pub sample: String,
    /// Comparison that is correct for this sample.
    pub correct: String,
    /// Comparison stimuli presented on the trial.
    pub comparisons: Vec<String>,
}

impl TrialType {
    /// Construct a trial type.
    pub fn new<S, C, I, T>(sample: S, correct: C, comparisons: I) -> Self
    where
        S: Into<String>,
        C: Into<String>,
        I: IntoIterator<Item = T>,
        T: Into<String>,
    {
        Self {
            sample: sample.into(),
            correct: correct.into(),
            comparisons: comparisons.into_iter().map(Into::into).collect(),
        }
    }
}

fn validate_trial_types(name: &str, types: &[TrialType], n_comparisons: u32) -> Result<()> {
    for tt in types {
        if tt.comparisons.len() != n_comparisons as usize {
            return Err(ContingencyError::Config(format!(
                "{name}: trial type for sample {:?} must have {n_comparisons} comparisons, got {}",
                tt.sample,
                tt.comparisons.len()
            )));
        }
        for (i, c) in tt.comparisons.iter().enumerate() {
            if tt.comparisons[..i].contains(c) {
                return Err(ContingencyError::Config(format!(
                    "{name}: duplicate comparison {c:?} for sample {:?}",
                    tt.sample
                )));
            }
        }
        if !tt.comparisons.contains(&tt.correct) {
            return Err(ContingencyError::Config(format!(
                "{name}: correct {:?} is not among the comparisons for sample {:?}",
                tt.correct, tt.sample
            )));
        }
    }
    Ok(())
}

type Meta = alloc::collections::BTreeMap<String, MetaValue>;

/// Matching-to-sample (MTS) discrete-trial schedule.
///
/// A trial unfolds as `sample -> choice -> iti`. Comparison stimuli are
/// exposed as operanda `choice_0 ... choice_{N-1}`; exactly one is
/// designated correct (sampled uniformly at random at trial start).
///
/// With [`MatchingToSample::with_trial_types`], each block presents every
/// baseline and probe trial type once in a shuffled order, and the
/// comparison positions are shuffled at the start of each trial. Probe
/// trials are scored but never consequated.
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
    rng: PyRandom,
    phase: MtsPhase,
    phase_started: Option<f64>,
    trial_index: u64,
    correct_operandum: String,
    last_now: Option<f64>,
    pending_outcome_label: Option<&'static str>,
    trial_types: Option<Vec<TrialType>>,
    n_baseline_types: usize,
    block_order: Vec<usize>,
    block_pos: usize,
    current_type_index: Option<usize>,
    current_positions: Vec<String>,
}

impl core::fmt::Debug for MatchingToSample {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("MatchingToSample")
            .field("n_comparisons", &self.n_comparisons)
            .field("sample_duration", &self.sample_duration)
            .field("choice_timeout", &self.choice_timeout)
            .field("iti", &self.iti)
            .field("phase", &self.phase)
            .field("trial_index", &self.trial_index)
            .field("correct_operandum", &self.correct_operandum)
            .field("trial_types", &self.trial_types)
            .field("n_baseline_types", &self.n_baseline_types)
            .field("current_type_index", &self.current_type_index)
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
        let correct = format!("choice_{}", rng.randbelow(n_comparisons as u64));
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
            trial_types: None,
            n_baseline_types: 0,
            block_order: Vec::new(),
            block_pos: 0,
            current_type_index: None,
            current_positions: Vec::new(),
        })
    }

    /// Attach baseline and probe trial types.
    ///
    /// `None` for `trial_types` keeps the default behaviour (one correct
    /// operandum drawn uniformly per trial); `probe_trial_types` then must
    /// also be `None`. The random stream restarts from the seed so the
    /// first trial matches a schedule constructed with trial types.
    pub fn with_trial_types(
        mut self,
        trial_types: Option<Vec<TrialType>>,
        probe_trial_types: Option<Vec<TrialType>>,
    ) -> Result<Self> {
        let Some(baseline) = trial_types else {
            if probe_trial_types.is_some() {
                return Err(ContingencyError::Config(
                    "probe_trial_types requires trial_types".into(),
                ));
            }
            return Ok(self);
        };
        if baseline.is_empty() {
            return Err(ContingencyError::Config(
                "trial_types must contain at least one trial type".into(),
            ));
        }
        validate_trial_types("trial_types", &baseline, self.n_comparisons)?;
        let probes = probe_trial_types.unwrap_or_default();
        validate_trial_types("probe_trial_types", &probes, self.n_comparisons)?;
        self.n_baseline_types = baseline.len();
        let mut all = baseline;
        all.extend(probes);
        self.trial_types = Some(all);
        self.rng = make_rng(self.seed);
        self.clear_block();
        self.correct_operandum = self.pick_correct();
        Ok(self)
    }

    /// Sample of the current trial; `None` without trial types.
    pub fn current_sample(&self) -> Option<&str> {
        let types = self.trial_types.as_ref()?;
        self.current_type_index.map(|i| types[i].sample.as_str())
    }

    /// Comparisons of the current trial in position order (`choice_0` first).
    pub fn current_comparisons(&self) -> &[String] {
        &self.current_positions
    }

    /// Whether the current trial is a probe (unconsequated) trial.
    pub fn is_probe_trial(&self) -> bool {
        self.current_type_index
            .is_some_and(|i| i >= self.n_baseline_types)
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

    fn clear_block(&mut self) {
        self.block_order.clear();
        self.block_pos = 0;
        self.current_type_index = None;
        self.current_positions.clear();
    }

    fn pick_correct(&mut self) -> String {
        let Some(types) = self.trial_types.as_ref() else {
            let idx = self.rng.randbelow(self.n_comparisons as u64);
            return format!("choice_{idx}");
        };
        if self.block_pos >= self.block_order.len() {
            let mut order: Vec<usize> = (0..types.len()).collect();
            self.rng.shuffle(&mut order);
            self.block_order = order;
            self.block_pos = 0;
        }
        let type_index = self.block_order[self.block_pos];
        self.block_pos += 1;
        let tt = &types[type_index];
        let mut positions = tt.comparisons.clone();
        self.rng.shuffle(&mut positions);
        // Validation guarantees `correct` is among the comparisons.
        let idx = positions.iter().position(|c| *c == tt.correct).unwrap_or(0);
        self.current_type_index = Some(type_index);
        self.current_positions = positions;
        format!("choice_{idx}")
    }

    fn choice_name(&self, operandum: &str) -> Option<String> {
        let suffix = operandum.strip_prefix("choice_")?;
        if suffix.is_empty() || !suffix.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let idx: usize = suffix.parse().ok()?;
        self.current_positions.get(idx).cloned()
    }

    fn meta(&self, trial_outcome: Option<&str>) -> Meta {
        let mut meta = Meta::new();
        meta.insert("phase".into(), MetaValue::Str(self.phase().into()));
        meta.insert(
            "trial_index".into(),
            MetaValue::Int(self.trial_index as i64),
        );
        if let Some(lbl) = trial_outcome {
            meta.insert("trial_outcome".into(), MetaValue::Str(lbl.into()));
        }
        if let (Some(types), Some(i)) = (self.trial_types.as_ref(), self.current_type_index) {
            let tt = &types[i];
            meta.insert("sample".into(), MetaValue::Str(tt.sample.clone()));
            meta.insert(
                "comparisons".into(),
                MetaValue::StrList(self.current_positions.clone()),
            );
            meta.insert("correct".into(), MetaValue::Str(tt.correct.clone()));
            meta.insert("probe".into(), MetaValue::Bool(self.is_probe_trial()));
        }
        meta
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
        let label = self.pending_outcome_label.take();
        Outcome {
            reinforced: false,
            reinforcer: None,
            meta: self.meta(label),
        }
    }

    fn unreinforced(meta: Meta) -> Outcome {
        Outcome {
            reinforced: false,
            reinforcer: None,
            meta,
        }
    }

    fn incorrect_timeout_outcome(&mut self, now: f64) -> Result<Outcome> {
        if self.trial_types.is_some() {
            self.pending_outcome_label = None;
            let meta = self.meta(Some("incorrect"));
            if self.is_probe_trial() || self.incorrect.is_none() {
                return Ok(Self::unreinforced(meta));
            }
        } else if self.incorrect.is_none() {
            return Ok(self.phase_only_outcome());
        }
        let meta = self.meta(Some("incorrect"));
        let sub = self.incorrect.as_mut().unwrap().step(now, None)?;
        self.pending_outcome_label = None;
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
        let mut meta = self.meta(Some(label));
        if self.trial_types.is_some() {
            if let Some(name) = self.choice_name(&event.operandum) {
                meta.insert("choice".into(), MetaValue::Str(name));
            }
            if self.is_probe_trial() {
                // Probe trials are scored but never consequated.
                return Ok(Self::unreinforced(meta));
            }
        }
        let sched_slot = if correct {
            &mut self.consequence
        } else {
            &mut self.incorrect
        };
        match sched_slot.as_mut() {
            None if correct => Ok(Outcome {
                reinforced: true,
                reinforcer: Some(Reinforcer::at(now)),
                meta,
            }),
            None => Ok(Self::unreinforced(meta)),
            Some(sched) => {
                let sub = sched.step(now, Some(event))?;
                Ok(Outcome {
                    reinforced: sub.reinforced,
                    reinforcer: sub.reinforcer,
                    meta,
                })
            }
        }
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
                        self.enter_iti(started + self.choice_timeout, "incorrect");
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
        self.clear_block();
        self.correct_operandum = self.pick_correct();
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
    rng: PyRandom,
    phase: GngPhase,
    phase_started: Option<f64>,
    trial_index: u64,
    current_is_go: bool,
    last_now: Option<f64>,
    pending_outcome_label: Option<&'static str>,
}

impl core::fmt::Debug for GoNoGo {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
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
        let current_is_go = rng.random() < go_probability;
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
        self.rng.random() < self.go_probability
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
            let mut meta = alloc::collections::BTreeMap::new();
            meta.insert("phase".into(), MetaValue::Str(phase_snapshot.into()));
            meta.insert(
                "trial_index".into(),
                MetaValue::Int(self.trial_index as i64),
            );
            meta.insert("trial_outcome".into(), MetaValue::Str(label.into()));
            return Ok(Outcome {
                reinforced: true,
                reinforcer: Some(Reinforcer::at(now)),
                meta,
            });
        }
        if target_slot.is_none() {
            let mut meta = alloc::collections::BTreeMap::new();
            meta.insert("phase".into(), MetaValue::Str(phase_snapshot.into()));
            meta.insert(
                "trial_index".into(),
                MetaValue::Int(self.trial_index as i64),
            );
            meta.insert("trial_outcome".into(), MetaValue::Str(label.into()));
            return Ok(Outcome {
                reinforced: false,
                reinforcer: None,
                meta,
            });
        }
        let sub = target_slot.as_mut().unwrap().step(now, Some(event))?;
        let mut meta = alloc::collections::BTreeMap::new();
        meta.insert("phase".into(), MetaValue::Str(phase_snapshot.into()));
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
                                let mut meta = alloc::collections::BTreeMap::new();
                                meta.insert("phase".into(), MetaValue::Str("nogo".into()));
                                meta.insert("trial_index".into(), MetaValue::Int(trial_idx as i64));
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
        assert_eq!(o.meta.get("phase"), Some(&MetaValue::Str("sample".into())));
        assert!(!o.reinforced);
        assert!(!o.meta.contains_key("trial_outcome"));
    }

    #[test]
    fn mts_tick_advances_into_choice() {
        let mut mts = MatchingToSample::new(2, 1.0, 2.0, None, None, 1.0, Some(1)).unwrap();
        mts.step(0.0, None).unwrap();
        let o = mts.step(1.5, None).unwrap();
        assert_eq!(o.meta.get("phase"), Some(&MetaValue::Str("choice".into())));
    }

    // ---- MTS choice phase ------------------------------------------------

    #[test]
    fn mts_correct_choice_defaults_to_crf() {
        let mut mts = MatchingToSample::new(3, 0.0, 5.0, None, None, 1.0, Some(123)).unwrap();
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
        let mut mts = MatchingToSample::new(3, 0.0, 5.0, None, None, 1.0, Some(123)).unwrap();
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
        let mut mts =
            MatchingToSample::new(2, 0.0, 5.0, Some(consequence), None, 1.0, Some(42)).unwrap();
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
        let mut mts = MatchingToSample::new(2, 0.0, 3.0, None, None, 1.0, Some(1)).unwrap();
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
        let mut mts =
            MatchingToSample::new(2, 0.0, 1.0, None, Some(incorrect), 1.0, Some(1)).unwrap();
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
        let mut mts = MatchingToSample::new(2, 0.0, 5.0, None, None, 1.0, Some(1)).unwrap();
        let correct = mts.correct_operandum().to_string();
        let r = ev(&correct, 0.0);
        mts.step(0.0, Some(&r)).unwrap();
        // ITI: response ignored.
        let r2 = ev("choice_0", 0.5);
        let o = mts.step(0.5, Some(&r2)).unwrap();
        assert_eq!(o.meta.get("phase"), Some(&MetaValue::Str("iti".into())));
        let o = mts.step(1.2, None).unwrap();
        assert_eq!(o.meta.get("phase"), Some(&MetaValue::Str("choice".into())));
        assert_eq!(mts.trial_index(), 1);
    }

    #[test]
    fn mts_iti_response_ignored() {
        let mut mts = MatchingToSample::new(2, 0.0, 5.0, None, None, 2.0, Some(1)).unwrap();
        let correct = mts.correct_operandum().to_string();
        let r = ev(&correct, 0.0);
        mts.step(0.0, Some(&r)).unwrap();
        let r2 = ev("choice_0", 0.5);
        let o = mts.step(0.5, Some(&r2)).unwrap();
        assert_eq!(o.meta.get("phase"), Some(&MetaValue::Str("iti".into())));
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
        let mut mts = MatchingToSample::new(2, 0.0, 5.0, None, None, 1.0, Some(3)).unwrap();
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
        let mut mts = MatchingToSample::new(2, 0.0, 5.0, None, None, 1.0, Some(1)).unwrap();
        mts.step(1.0, None).unwrap();
        assert!(mts.step(0.5, None).is_err());
    }

    #[test]
    fn mts_event_time_mismatch_rejected() {
        let mut mts = MatchingToSample::new(2, 0.0, 5.0, None, None, 1.0, Some(1)).unwrap();
        let r = ev("choice_0", 2.0);
        assert!(mts.step(1.0, Some(&r)).is_err());
    }

    // ---- MTS trial types -------------------------------------------------

    fn ab_types() -> Vec<TrialType> {
        let b = ["B1", "B2", "B3"];
        ["A1", "A2", "A3"]
            .iter()
            .zip(b.iter())
            .map(|(s, c)| TrialType::new(*s, *c, b))
            .collect()
    }

    fn ba_types() -> Vec<TrialType> {
        let a = ["A1", "A2", "A3"];
        ["B1", "B2", "B3"]
            .iter()
            .zip(a.iter())
            .map(|(s, c)| TrialType::new(*s, *c, a))
            .collect()
    }

    fn mts_ab_ba(seed: u64) -> MatchingToSample {
        MatchingToSample::new(3, 0.0, 2.0, None, None, 1.0, Some(seed))
            .unwrap()
            .with_trial_types(Some(ab_types()), Some(ba_types()))
            .unwrap()
    }

    #[test]
    fn mts_trial_types_block_order_matches_python() {
        // Expected values come from contingency-py with the same seed.
        let want: [(&str, [&str; 3], &str, bool); 7] = [
            ("A1", ["B2", "B1", "B3"], "choice_1", false),
            ("A3", ["B2", "B1", "B3"], "choice_2", false),
            ("B1", ["A1", "A3", "A2"], "choice_0", true),
            ("B3", ["A2", "A1", "A3"], "choice_2", true),
            ("B2", ["A3", "A2", "A1"], "choice_1", true),
            ("A2", ["B1", "B2", "B3"], "choice_1", false),
            ("B3", ["A1", "A2", "A3"], "choice_2", true),
        ];
        let mut mts = mts_ab_ba(3);
        let mut t = 0.0;
        for (sample, comps, correct, probe) in want {
            assert_eq!(mts.current_sample(), Some(sample));
            let got: Vec<&str> = mts.current_comparisons().iter().map(String::as_str).collect();
            assert_eq!(got, comps);
            assert_eq!(mts.correct_operandum(), correct);
            assert_eq!(mts.is_probe_trial(), probe);
            let r = ev(correct, t);
            let out = mts.step(t, Some(&r)).unwrap();
            assert_eq!(out.reinforced, !probe);
            t += 1.0;
            mts.step(t, None).unwrap();
        }
    }

    #[test]
    fn mts_trial_types_meta_keys() {
        let mut mts = mts_ab_ba(3);
        let r = ev("choice_0", 0.0);
        let out = mts.step(0.0, Some(&r)).unwrap();
        assert!(!out.reinforced);
        assert_eq!(out.meta.get("sample"), Some(&MetaValue::Str("A1".into())));
        assert_eq!(out.meta.get("correct"), Some(&MetaValue::Str("B1".into())));
        assert_eq!(out.meta.get("choice"), Some(&MetaValue::Str("B2".into())));
        assert_eq!(out.meta.get("probe"), Some(&MetaValue::Bool(false)));
        assert_eq!(
            out.meta.get("comparisons"),
            Some(&MetaValue::StrList(vec!["B2".into(), "B1".into(), "B3".into()]))
        );
        assert_eq!(
            out.meta.get("trial_outcome"),
            Some(&MetaValue::Str("incorrect".into()))
        );
    }

    #[test]
    fn mts_default_meta_has_no_trial_type_keys() {
        let mut mts = MatchingToSample::new(2, 0.0, 5.0, None, None, 1.0, Some(1)).unwrap();
        let op = mts.correct_operandum().to_string();
        let out = mts.step(0.0, Some(&ev(&op, 0.0))).unwrap();
        assert!(out.meta.get("sample").is_none());
        assert!(out.meta.get("choice").is_none());
        assert_eq!(mts.current_sample(), None);
        assert!(!mts.is_probe_trial());
    }

    #[test]
    fn mts_probe_does_not_step_nested_schedules() {
        // FR 2 consequence: if the probe trial stepped it, the next
        // baseline correct choice would be reinforced.
        let mut mts = MatchingToSample::new(3, 0.0, 2.0, Some(Box::new(FR::new(2).unwrap())), None, 1.0, Some(3))
            .unwrap()
            .with_trial_types(Some(ab_types()), Some(ba_types()))
            .unwrap();
        let mut t = 0.0;
        let mut reinforced = Vec::new();
        for _ in 0..6 {
            let op = mts.correct_operandum().to_string();
            let out = mts.step(t, Some(&ev(&op, t))).unwrap();
            reinforced.push(out.reinforced);
            t += 1.0;
            mts.step(t, None).unwrap();
        }
        // Baseline correct trials are 0, 1, 5; FR 2 fires on the second.
        assert_eq!(reinforced, [false, true, false, false, false, false]);
    }

    #[test]
    fn mts_probe_timeout_does_not_step_incorrect() {
        let mut mts = MatchingToSample::new(3, 0.0, 2.0, None, Some(Box::new(FT::new(0.0001).unwrap())), 1.0, Some(3))
            .unwrap()
            .with_trial_types(Some(ab_types()), Some(ba_types()))
            .unwrap();
        // Trial 0 and 1 are baseline; answer them, then let probe trial 2 time out.
        for t in [0.0, 2.0] {
            let op = mts.correct_operandum().to_string();
            mts.step(t, Some(&ev(&op, t))).unwrap();
        }
        mts.step(3.0, None).unwrap();
        assert!(mts.is_probe_trial());
        let out = mts.step(5.5, None).unwrap();
        assert!(!out.reinforced);
        assert_eq!(out.meta.get("trial_outcome"), Some(&MetaValue::Str("incorrect".into())));
        assert!(out.meta.get("choice").is_none());
    }

    #[test]
    fn mts_trial_types_reset_restarts_block() {
        let mut mts = mts_ab_ba(3);
        for t in [0.0, 2.0, 4.0] {
            let op = mts.correct_operandum().to_string();
            mts.step(t, Some(&ev(&op, t))).unwrap();
        }
        mts.reset();
        assert_eq!(mts.current_sample(), Some("A1"));
        assert_eq!(mts.correct_operandum(), "choice_1");
    }

    #[test]
    fn mts_trial_types_validation() {
        let base = || MatchingToSample::new(3, 0.0, 2.0, None, None, 1.0, Some(1)).unwrap();
        assert!(base().with_trial_types(Some(vec![]), None).is_err());
        assert!(base().with_trial_types(None, Some(ba_types())).is_err());
        assert!(base()
            .with_trial_types(Some(vec![TrialType::new("A1", "B1", ["B1", "B2"])]), None)
            .is_err());
        assert!(base()
            .with_trial_types(Some(vec![TrialType::new("A1", "B9", ["B1", "B2", "B3"])]), None)
            .is_err());
        assert!(base()
            .with_trial_types(Some(vec![TrialType::new("A1", "B1", ["B1", "B1", "B3"])]), None)
            .is_err());
        assert!(base()
            .with_trial_types(Some(ab_types()), Some(vec![TrialType::new("B1", "A1", ["A1", "A2"])]))
            .is_err());
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
        let mut g = GoNoGo::new(0.5, 2.0, 1.0, None, None, None, Some(seed)).unwrap();
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
        let mut g = GoNoGo::new(0.5, 2.0, 1.0, None, None, None, Some(seed)).unwrap();
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
        let mut g = GoNoGo::new(0.5, 2.0, 1.0, None, None, None, Some(seed)).unwrap();
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
        let mut g = GoNoGo::new(0.5, 2.0, 1.0, None, None, Some(fa), Some(seed)).unwrap();
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
        let mut g = GoNoGo::new(0.5, 2.0, 1.0, None, None, None, Some(seed)).unwrap();
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
        let mut g = GoNoGo::new(p, 1.0, 0.0, None, None, None, Some(42)).unwrap();
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
        let mut g = GoNoGo::new(0.5, 1.0, 0.5, None, None, None, Some(3)).unwrap();
        g.step(0.0, None).unwrap();
        g.step(2.0, None).unwrap();
        assert!(g.trial_index() >= 1);
        g.reset();
        assert_eq!(g.trial_index(), 0);
    }

    #[test]
    fn gonogo_non_monotonic_rejected() {
        let mut g = GoNoGo::new(0.5, 2.0, 1.0, None, None, None, Some(1)).unwrap();
        g.step(1.0, None).unwrap();
        assert!(g.step(0.5, None).is_err());
    }
}

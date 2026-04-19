//! Sequence-family compound schedules: [`Multiple`], [`Chained`], and
//! [`Tandem`].
//!
//! All three schedules compose a list of component schedules and expose
//! the same [`Schedule`] trait as their primitive constituents. They
//! differ in *what happens when the active component reinforces*:
//!
//! * [`Multiple`] — components alternate cyclically. Each carries its
//!   own discriminative stimulus (S^D). When the active component
//!   produces a reinforcer, the primary reinforcer is delivered and the
//!   next component becomes active.
//! * [`Chained`] — components form a chain with distinct S^Ds. Only the
//!   terminal component produces primary reinforcement. Completing a
//!   non-terminal component is a *conditioned* reinforcement event: the
//!   S^D changes but no [`Reinforcer`] is delivered.
//! * [`Tandem`] — the chained topology but **without** distinctive
//!   stimuli. The subject receives no external cue that a link has
//!   advanced; only the experimenter's record (the
//!   ``current_component`` integer index in ``meta``) tracks link
//!   transitions.
//!
//! # Time-stepping inactive components
//!
//! Inactive components are *not* stepped. Only the active component
//! receives [`Schedule::step`] calls. This matches the operant-chamber
//! convention that inactive components are "off": their clocks do not
//! advance while another S^D is in effect. The consequence is that a
//! newly-activated time-based schedule (FI, FT, VI, VT, RI, RT) will
//! anchor on its first step *after* the transition, not on the moment
//! the transition occurred — exactly as in a real operant chamber where
//! the S^D and the clock both restart on link entry.
//!
//! # References
//!
//! Ferster, C. B., & Skinner, B. F. (1957). *Schedules of reinforcement*.
//! Appleton-Century-Crofts.
//!
//! Kelleher, R. T., & Gollub, L. R. (1962). A review of positive
//! conditioned reinforcement. *Journal of the Experimental Analysis of
//! Behavior*, 5(4 Suppl), 543-597.
//! <https://doi.org/10.1901/jeab.1962.5-s543>
//!
//! Reynolds, G. S. (1961). Behavioral contrast. *Journal of the
//! Experimental Analysis of Behavior*, 4(1), 57-71.
//! <https://doi.org/10.1901/jeab.1961.4-57>

use std::collections::HashSet;
use std::fmt;

use crate::errors::ContingencyError;
use crate::helpers::checks::{check_event, check_time};
use crate::schedule::Schedule;
use crate::types::{MetaValue, Outcome, ResponseEvent};
use crate::Result;

// ---------------------------------------------------------------------
// Shared validation helpers
// ---------------------------------------------------------------------

fn validate_components(components: &[Box<dyn Schedule>]) -> Result<()> {
    if components.len() < 2 {
        return Err(ContingencyError::Config(format!(
            "compound schedule requires >= 2 components, got {}",
            components.len()
        )));
    }
    Ok(())
}

fn default_stimuli(n: usize) -> Vec<String> {
    (0..n).map(|i| format!("comp_{i}")).collect()
}

fn validate_stimuli(stimuli: Option<Vec<String>>, n_components: usize) -> Result<Vec<String>> {
    let names = match stimuli {
        None => return Ok(default_stimuli(n_components)),
        Some(v) => v,
    };
    if names.len() != n_components {
        return Err(ContingencyError::Config(format!(
            "stimuli length ({}) must equal components length ({})",
            names.len(),
            n_components
        )));
    }
    let unique: HashSet<&String> = names.iter().collect();
    if unique.len() != names.len() {
        return Err(ContingencyError::Config(format!(
            "stimulus names must be unique, got {names:?}"
        )));
    }
    Ok(names)
}

// ---------------------------------------------------------------------
// Multiple
// ---------------------------------------------------------------------

/// Multiple (``mult``) compound schedule.
///
/// Two or more independent component schedules alternate in the
/// presence of distinctive discriminative stimuli (S^D). Exactly one
/// component is active at a time; responses and ticks are routed only
/// to that component. When the active component produces a reinforcer,
/// the primary reinforcer is delivered and the active index advances
/// (wrapping to 0 after the last component). The returned
/// [`Outcome::meta`] entry ``current_component`` always carries the
/// stimulus NAME of the component that just produced the step's result
/// (i.e., the component that just fired on reinforced steps, or the
/// currently-active component on non-reinforced steps).
///
/// Inactive components are not stepped; see the module docstring.
///
/// # References
///
/// Ferster, C. B., & Skinner, B. F. (1957). *Schedules of
/// reinforcement*. Appleton-Century-Crofts.
///
/// Reynolds, G. S. (1961). Behavioral contrast. *Journal of the
/// Experimental Analysis of Behavior*, 4(1), 57-71.
/// <https://doi.org/10.1901/jeab.1961.4-57>
pub struct Multiple {
    components: Vec<Box<dyn Schedule>>,
    stimuli: Vec<String>,
    active_index: usize,
    last_now: Option<f64>,
}

impl fmt::Debug for Multiple {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Multiple")
            .field("n_components", &self.components.len())
            .field("stimuli", &self.stimuli)
            .field("active_index", &self.active_index)
            .field("last_now", &self.last_now)
            .finish()
    }
}

impl Multiple {
    /// Construct a Multiple schedule from a vector of components and an
    /// optional vector of per-component stimulus labels.
    ///
    /// # Errors
    ///
    /// Returns [`ContingencyError::Config`] if
    /// * `components.len() < 2`, or
    /// * `stimuli.is_some()` and its length does not match the number
    ///   of components, or
    /// * duplicate stimulus names are provided.
    pub fn new(components: Vec<Box<dyn Schedule>>, stimuli: Option<Vec<String>>) -> Result<Self> {
        validate_components(&components)?;
        let stimuli = validate_stimuli(stimuli, components.len())?;
        Ok(Self {
            components,
            stimuli,
            active_index: 0,
            last_now: None,
        })
    }

    /// The active component index.
    pub fn active_index(&self) -> usize {
        self.active_index
    }

    /// Total number of components.
    pub fn n_components(&self) -> usize {
        self.components.len()
    }

    /// Stimulus label of the currently active component.
    pub fn current_stimulus(&self) -> &str {
        &self.stimuli[self.active_index]
    }
}

impl Schedule for Multiple {
    fn step(&mut self, now: f64, event: Option<&ResponseEvent>) -> Result<Outcome> {
        check_time(now, self.last_now)?;
        check_event(now, event)?;
        self.last_now = Some(now);

        let stim_before = self.stimuli[self.active_index].clone();
        let inner = self.components[self.active_index].step(now, event)?;

        if inner.reinforced {
            // Primary reinforcement; then advance (wrap) for the next step.
            self.active_index = (self.active_index + 1) % self.components.len();
            let mut meta = inner.meta.clone();
            meta.insert("current_component".to_string(), MetaValue::Str(stim_before));
            return Ok(Outcome {
                reinforced: true,
                reinforcer: inner.reinforcer,
                meta,
            });
        }

        let mut meta = inner.meta.clone();
        meta.insert("current_component".to_string(), MetaValue::Str(stim_before));
        Ok(Outcome {
            reinforced: false,
            reinforcer: None,
            meta,
        })
    }

    fn reset(&mut self) {
        self.active_index = 0;
        self.last_now = None;
        for c in self.components.iter_mut() {
            c.reset();
        }
    }
}

// ---------------------------------------------------------------------
// Chained
// ---------------------------------------------------------------------

/// Chained (``chain``) compound schedule.
///
/// A sequence of N components (links). Each link carries a distinctive
/// S^D; completing a non-terminal link produces a *conditioned*
/// reinforcement event (the S^D changes, signalling access to the next
/// link) but delivers no primary [`Reinforcer`]. Only the terminal
/// (last) link produces primary reinforcement. After primary
/// reinforcement the active link resets to 0.
///
/// The returned [`Outcome::meta`] always carries
/// ``current_component: Str`` naming the component that is active
/// *after* any advancement (i.e., on a transition, the name of the
/// newly entered link; on terminal SR+, the name of link 0 since we
/// cycled back). Non-terminal transitions additionally set
/// ``chain_transition: Bool(true)``.
///
/// Inactive components are not stepped; see the module docstring.
///
/// # References
///
/// Ferster, C. B., & Skinner, B. F. (1957). *Schedules of
/// reinforcement*. Appleton-Century-Crofts.
///
/// Kelleher, R. T., & Gollub, L. R. (1962). A review of positive
/// conditioned reinforcement. *Journal of the Experimental Analysis of
/// Behavior*, 5(4 Suppl), 543-597.
/// <https://doi.org/10.1901/jeab.1962.5-s543>
pub struct Chained {
    components: Vec<Box<dyn Schedule>>,
    stimuli: Vec<String>,
    active_index: usize,
    last_now: Option<f64>,
}

impl fmt::Debug for Chained {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Chained")
            .field("n_components", &self.components.len())
            .field("stimuli", &self.stimuli)
            .field("active_index", &self.active_index)
            .field("last_now", &self.last_now)
            .finish()
    }
}

impl Chained {
    /// Construct a Chained schedule from a vector of components and an
    /// optional vector of per-link stimulus labels.
    ///
    /// # Errors
    ///
    /// Returns [`ContingencyError::Config`] if
    /// * `components.len() < 2`, or
    /// * `stimuli.is_some()` and its length does not match the number
    ///   of components, or
    /// * duplicate stimulus names are provided.
    pub fn new(components: Vec<Box<dyn Schedule>>, stimuli: Option<Vec<String>>) -> Result<Self> {
        validate_components(&components)?;
        let stimuli = validate_stimuli(stimuli, components.len())?;
        Ok(Self {
            components,
            stimuli,
            active_index: 0,
            last_now: None,
        })
    }

    /// The active link index.
    pub fn active_index(&self) -> usize {
        self.active_index
    }

    /// Total number of links.
    pub fn n_components(&self) -> usize {
        self.components.len()
    }

    /// Stimulus label of the currently active link.
    pub fn current_stimulus(&self) -> &str {
        &self.stimuli[self.active_index]
    }

    /// Whether the currently active link is the terminal link.
    pub fn is_terminal(&self) -> bool {
        self.active_index + 1 == self.components.len()
    }
}

impl Schedule for Chained {
    fn step(&mut self, now: f64, event: Option<&ResponseEvent>) -> Result<Outcome> {
        check_time(now, self.last_now)?;
        check_event(now, event)?;
        self.last_now = Some(now);

        let inner = self.components[self.active_index].step(now, event)?;

        if inner.reinforced {
            let terminal = self.is_terminal();
            if terminal {
                // Primary reinforcement; cycle back to link 0.
                self.active_index = 0;
                let mut meta = inner.meta.clone();
                meta.insert(
                    "current_component".to_string(),
                    MetaValue::Str(self.stimuli[self.active_index].clone()),
                );
                return Ok(Outcome {
                    reinforced: true,
                    reinforcer: inner.reinforcer,
                    meta,
                });
            }
            // Non-terminal completion: conditioned reinforcement =
            // transition to next link; no primary reinforcer.
            self.active_index += 1;
            let mut meta = inner.meta.clone();
            meta.insert(
                "current_component".to_string(),
                MetaValue::Str(self.stimuli[self.active_index].clone()),
            );
            meta.insert("chain_transition".to_string(), MetaValue::Bool(true));
            return Ok(Outcome {
                reinforced: false,
                reinforcer: None,
                meta,
            });
        }

        let mut meta = inner.meta.clone();
        meta.insert(
            "current_component".to_string(),
            MetaValue::Str(self.stimuli[self.active_index].clone()),
        );
        Ok(Outcome {
            reinforced: false,
            reinforcer: None,
            meta,
        })
    }

    fn reset(&mut self) {
        self.active_index = 0;
        self.last_now = None;
        for c in self.components.iter_mut() {
            c.reset();
        }
    }
}

// ---------------------------------------------------------------------
// Tandem
// ---------------------------------------------------------------------

/// Tandem (``tand``) compound schedule.
///
/// Structurally identical to [`Chained`], but without distinctive
/// stimuli. Link transitions are invisible to the subject — only the
/// experimenter's record (the integer ``current_component`` index in
/// [`Outcome::meta`]) tracks the active link. Primary reinforcement
/// occurs only on the terminal link. After primary reinforcement the
/// active link resets to 0.
///
/// The ``current_component`` meta value is [`MetaValue::Int`] (the
/// active link index *after* any advancement), distinguishing Tandem
/// output from the string-valued [`Multiple`]/[`Chained`] output.
/// Non-terminal transitions additionally set ``chain_transition:
/// Bool(true)``.
///
/// Because the constructor accepts no stimuli argument, attempting to
/// supply stimulus labels is a compile-time error — the Rust analogue
/// of the Python ``TypeError`` contract.
///
/// Inactive components are not stepped; see the module docstring.
///
/// # References
///
/// Ferster, C. B., & Skinner, B. F. (1957). *Schedules of
/// reinforcement*. Appleton-Century-Crofts.
///
/// Kelleher, R. T., & Gollub, L. R. (1962). A review of positive
/// conditioned reinforcement. *Journal of the Experimental Analysis of
/// Behavior*, 5(4 Suppl), 543-597.
/// <https://doi.org/10.1901/jeab.1962.5-s543>
pub struct Tandem {
    components: Vec<Box<dyn Schedule>>,
    active_index: usize,
    last_now: Option<f64>,
}

impl fmt::Debug for Tandem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Tandem")
            .field("n_components", &self.components.len())
            .field("active_index", &self.active_index)
            .field("last_now", &self.last_now)
            .finish()
    }
}

impl Tandem {
    /// Construct a Tandem schedule from a vector of components.
    ///
    /// # Errors
    ///
    /// Returns [`ContingencyError::Config`] if `components.len() < 2`.
    pub fn new(components: Vec<Box<dyn Schedule>>) -> Result<Self> {
        validate_components(&components)?;
        Ok(Self {
            components,
            active_index: 0,
            last_now: None,
        })
    }

    /// The active link index.
    pub fn active_index(&self) -> usize {
        self.active_index
    }

    /// Total number of links.
    pub fn n_components(&self) -> usize {
        self.components.len()
    }

    /// Whether the currently active link is the terminal link.
    pub fn is_terminal(&self) -> bool {
        self.active_index + 1 == self.components.len()
    }
}

impl Schedule for Tandem {
    fn step(&mut self, now: f64, event: Option<&ResponseEvent>) -> Result<Outcome> {
        check_time(now, self.last_now)?;
        check_event(now, event)?;
        self.last_now = Some(now);

        let inner = self.components[self.active_index].step(now, event)?;

        if inner.reinforced {
            let terminal = self.is_terminal();
            if terminal {
                self.active_index = 0;
                let mut meta = inner.meta.clone();
                meta.insert(
                    "current_component".to_string(),
                    MetaValue::Int(self.active_index as i64),
                );
                return Ok(Outcome {
                    reinforced: true,
                    reinforcer: inner.reinforcer,
                    meta,
                });
            }
            self.active_index += 1;
            let mut meta = inner.meta.clone();
            meta.insert(
                "current_component".to_string(),
                MetaValue::Int(self.active_index as i64),
            );
            meta.insert("chain_transition".to_string(), MetaValue::Bool(true));
            return Ok(Outcome {
                reinforced: false,
                reinforcer: None,
                meta,
            });
        }

        let mut meta = inner.meta.clone();
        meta.insert(
            "current_component".to_string(),
            MetaValue::Int(self.active_index as i64),
        );
        Ok(Outcome {
            reinforced: false,
            reinforcer: None,
            meta,
        })
    }

    fn reset(&mut self) {
        self.active_index = 0;
        self.last_now = None;
        for c in self.components.iter_mut() {
            c.reset();
        }
    }
}

// ---------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schedules::{FR, FT};

    fn respond(s: &mut dyn Schedule, now: f64) -> Outcome {
        let ev = ResponseEvent::new(now);
        s.step(now, Some(&ev)).expect("step should succeed")
    }

    fn fr(n: u64) -> Box<dyn Schedule> {
        Box::new(FR::new(n).unwrap())
    }

    fn ft(interval: f64) -> Box<dyn Schedule> {
        Box::new(FT::new(interval).unwrap())
    }

    fn meta_str<'a>(o: &'a Outcome, key: &str) -> Option<&'a str> {
        match o.meta.get(key)? {
            MetaValue::Str(s) => Some(s.as_str()),
            _ => None,
        }
    }

    fn meta_int(o: &Outcome, key: &str) -> Option<i64> {
        match o.meta.get(key)? {
            MetaValue::Int(i) => Some(*i),
            _ => None,
        }
    }

    fn meta_bool(o: &Outcome, key: &str) -> Option<bool> {
        match o.meta.get(key)? {
            MetaValue::Bool(b) => Some(*b),
            _ => None,
        }
    }

    // ----- Multiple -----------------------------------------------------

    #[test]
    fn multiple_default_stimuli() {
        let m = Multiple::new(vec![fr(1), fr(1), fr(1)], None).unwrap();
        assert_eq!(m.current_stimulus(), "comp_0");
        assert_eq!(m.n_components(), 3);
        assert_eq!(m.active_index(), 0);
    }

    #[test]
    fn multiple_routes_to_active_component_only() {
        // FR(3) active, FR(5) idle. Reinforcement should happen at the
        // 3rd response even though FR(5) would require 5.
        let mut m =
            Multiple::new(vec![fr(3), fr(5)], Some(vec!["red".into(), "green".into()])).unwrap();
        assert!(!respond(&mut m, 1.0).reinforced);
        assert!(!respond(&mut m, 2.0).reinforced);
        let o = respond(&mut m, 3.0);
        assert!(o.reinforced);
        let r = o.reinforcer.as_ref().expect("reinforcer");
        assert_eq!(r.time, 3.0);
    }

    #[test]
    fn multiple_advances_after_reinforcement_and_wraps() {
        let mut m =
            Multiple::new(vec![fr(2), fr(3)], Some(vec!["red".into(), "green".into()])).unwrap();
        respond(&mut m, 1.0);
        let o_rf = respond(&mut m, 2.0);
        assert!(o_rf.reinforced);
        assert_eq!(m.active_index(), 1);
        assert_eq!(m.current_stimulus(), "green");

        // FR(3) now active.
        assert!(!respond(&mut m, 3.0).reinforced);
        assert!(!respond(&mut m, 4.0).reinforced);
        let o2 = respond(&mut m, 5.0);
        assert!(o2.reinforced);
        // Wraps back to component 0.
        assert_eq!(m.active_index(), 0);
        assert_eq!(m.current_stimulus(), "red");
    }

    #[test]
    fn multiple_wraps_with_fr1_trio() {
        let mut m = Multiple::new(
            vec![fr(1), fr(1), fr(1)],
            Some(vec!["a".into(), "b".into(), "c".into()]),
        )
        .unwrap();
        let expected = ["a", "b", "c", "a", "b", "c"];
        for (i, want) in expected.iter().enumerate() {
            let now = (i + 1) as f64;
            let o = respond(&mut m, now);
            assert!(o.reinforced);
            assert_eq!(meta_str(&o, "current_component").unwrap(), *want);
        }
    }

    #[test]
    fn multiple_current_component_on_non_reinforced_step() {
        let mut m =
            Multiple::new(vec![fr(3), fr(3)], Some(vec!["red".into(), "green".into()])).unwrap();
        let o = respond(&mut m, 1.0);
        assert!(!o.reinforced);
        assert_eq!(meta_str(&o, "current_component").unwrap(), "red");
    }

    #[test]
    fn multiple_current_component_on_reinforced_step_is_firing_component() {
        // On reinforcement the reported stimulus is the one that just
        // fired; the next step runs under the next stimulus.
        let mut m =
            Multiple::new(vec![fr(1), fr(1)], Some(vec!["red".into(), "green".into()])).unwrap();
        let o = respond(&mut m, 1.0);
        assert!(o.reinforced);
        assert_eq!(meta_str(&o, "current_component").unwrap(), "red");
        let o2 = m.step(2.0, None).unwrap();
        assert!(!o2.reinforced);
        assert_eq!(meta_str(&o2, "current_component").unwrap(), "green");
    }

    #[test]
    fn multiple_non_response_tick_does_not_advance() {
        let mut m =
            Multiple::new(vec![fr(2), fr(2)], Some(vec!["red".into(), "green".into()])).unwrap();
        let o = m.step(1.0, None).unwrap();
        assert!(!o.reinforced);
        assert_eq!(m.active_index(), 0);
    }

    #[test]
    fn multiple_with_ft_fires_while_active() {
        // FR(2) first, then FT(1.0). After the FR reinforces, FT becomes
        // active, anchors on its first step, and fires one interval later.
        let mut m = Multiple::new(
            vec![fr(2), ft(1.0)],
            Some(vec!["red".into(), "green".into()]),
        )
        .unwrap();
        respond(&mut m, 0.0);
        let o_rf = respond(&mut m, 1.0);
        assert!(o_rf.reinforced);
        // FT is now active. First step anchors; the step 1s later fires.
        let o_anchor = m.step(1.0, None).unwrap();
        assert!(!o_anchor.reinforced);
        assert_eq!(m.active_index(), 1);
        let o_ft = m.step(2.0, None).unwrap();
        assert!(o_ft.reinforced);
        let r = o_ft.reinforcer.as_ref().expect("reinforcer");
        assert_eq!(r.time, 2.0);
        // After FT fires, cycle wraps back to the FR component.
        assert_eq!(m.active_index(), 0);
        assert_eq!(m.current_stimulus(), "red");
    }

    #[test]
    fn multiple_non_active_component_is_not_stepped() {
        // Build a Multiple with FR(2) and an FT(1.0) in the inactive
        // slot. If Multiple were (incorrectly) stepping the inactive
        // FT, the FT's anchor would advance and it would fire on its
        // first tick after activation. We verify the opposite: the FT
        // anchors only *after* becoming active.
        let mut m =
            Multiple::new(vec![fr(3), ft(0.5)], Some(vec!["a".into(), "b".into()])).unwrap();
        // Many FR-directed responses; inactive FT must not advance.
        respond(&mut m, 1.0);
        respond(&mut m, 2.0);
        let o_rf = respond(&mut m, 3.0);
        assert!(o_rf.reinforced);
        // FT is now active. Its first step anchors at t (not earlier);
        // therefore the next step at t+0.4 must NOT yet reinforce,
        // proving the inactive FT didn't silently advance its clock
        // during the preceding FR-directed responses.
        let o_anchor = m.step(3.0, None).unwrap();
        assert!(!o_anchor.reinforced);
        let o_partial = m.step(3.4, None).unwrap();
        assert!(!o_partial.reinforced);
        // At t + 0.5 (full FT interval from anchor), it must fire.
        let o_fire = m.step(3.5, None).unwrap();
        assert!(o_fire.reinforced);
    }

    #[test]
    fn multiple_reset_returns_to_component_zero() {
        let mut m = Multiple::new(vec![fr(1), fr(1)], Some(vec!["a".into(), "b".into()])).unwrap();
        respond(&mut m, 1.0);
        assert_eq!(m.active_index(), 1);
        m.reset();
        assert_eq!(m.active_index(), 0);
        assert_eq!(m.current_stimulus(), "a");
    }

    #[test]
    fn multiple_reset_clears_last_now() {
        let mut m = Multiple::new(vec![fr(1), fr(1)], None).unwrap();
        m.step(5.0, None).unwrap();
        m.reset();
        let o = m.step(0.0, None).unwrap();
        assert!(!o.reinforced);
    }

    #[test]
    fn multiple_reset_resets_each_component() {
        let mut m = Multiple::new(vec![fr(3), fr(3)], Some(vec!["a".into(), "b".into()])).unwrap();
        respond(&mut m, 1.0);
        respond(&mut m, 2.0);
        m.reset();
        assert!(!respond(&mut m, 1.0).reinforced);
        assert!(!respond(&mut m, 2.0).reinforced);
        assert!(respond(&mut m, 3.0).reinforced);
    }

    #[test]
    fn multiple_non_monotonic_time_rejected() {
        let mut m = Multiple::new(vec![fr(1), fr(1)], None).unwrap();
        m.step(5.0, None).unwrap();
        let err = m.step(1.0, None).unwrap_err();
        assert!(matches!(err, ContingencyError::State(_)));
    }

    #[test]
    fn multiple_event_time_mismatch_rejected() {
        let mut m = Multiple::new(vec![fr(1), fr(1)], None).unwrap();
        let ev = ResponseEvent::new(2.0);
        let err = m.step(1.0, Some(&ev)).unwrap_err();
        assert!(matches!(err, ContingencyError::State(_)));
    }

    // ----- Chained ------------------------------------------------------

    #[test]
    fn chained_default_stimuli() {
        let c = Chained::new(vec![fr(1), fr(1)], None).unwrap();
        assert_eq!(c.current_stimulus(), "comp_0");
        assert_eq!(c.n_components(), 2);
        assert!(!c.is_terminal());
    }

    #[test]
    fn chained_non_terminal_reinforcement_is_transition() {
        // FR2 -> FR3. Completing FR2 must NOT produce a Reinforcer;
        // only the FR3 terminal does.
        let mut c =
            Chained::new(vec![fr(2), fr(3)], Some(vec!["red".into(), "green".into()])).unwrap();
        respond(&mut c, 1.0);
        let o_trans = respond(&mut c, 2.0);
        assert!(!o_trans.reinforced);
        assert!(o_trans.reinforcer.is_none());
        assert_eq!(meta_bool(&o_trans, "chain_transition"), Some(true));
        assert_eq!(meta_str(&o_trans, "current_component").unwrap(), "green");
        assert_eq!(c.active_index(), 1);
        assert!(c.is_terminal());
    }

    #[test]
    fn chained_terminal_produces_primary_reinforcer() {
        let mut c =
            Chained::new(vec![fr(2), fr(3)], Some(vec!["red".into(), "green".into()])).unwrap();
        respond(&mut c, 1.0);
        respond(&mut c, 2.0); // transition
        respond(&mut c, 3.0);
        respond(&mut c, 4.0);
        let o = respond(&mut c, 5.0);
        assert!(o.reinforced);
        let r = o.reinforcer.as_ref().expect("reinforcer");
        assert_eq!(r.time, 5.0);
        assert!(!o.meta.contains_key("chain_transition"));
        assert_eq!(c.active_index(), 0);
        assert_eq!(c.current_stimulus(), "red");
        assert_eq!(meta_str(&o, "current_component").unwrap(), "red");
    }

    #[test]
    fn chained_non_terminal_non_transition_step_carries_stimulus() {
        let mut c = Chained::new(vec![fr(3), fr(2)], Some(vec!["a".into(), "b".into()])).unwrap();
        let o = respond(&mut c, 1.0);
        assert!(!o.reinforced);
        assert_eq!(meta_str(&o, "current_component").unwrap(), "a");
        assert!(!o.meta.contains_key("chain_transition"));
    }

    #[test]
    fn chained_transition_flag_only_on_non_terminal() {
        let mut c = Chained::new(
            vec![fr(1), fr(1), fr(1)],
            Some(vec!["a".into(), "b".into(), "c".into()]),
        )
        .unwrap();
        let o0 = respond(&mut c, 1.0);
        assert!(!o0.reinforced);
        assert_eq!(meta_bool(&o0, "chain_transition"), Some(true));
        assert_eq!(meta_str(&o0, "current_component").unwrap(), "b");

        let o1 = respond(&mut c, 2.0);
        assert!(!o1.reinforced);
        assert_eq!(meta_bool(&o1, "chain_transition"), Some(true));
        assert_eq!(meta_str(&o1, "current_component").unwrap(), "c");

        let o2 = respond(&mut c, 3.0);
        assert!(o2.reinforced);
        assert!(!o2.meta.contains_key("chain_transition"));
        assert_eq!(c.active_index(), 0);
        assert_eq!(meta_str(&o2, "current_component").unwrap(), "a");
    }

    #[test]
    fn chained_cycles_back_after_terminal() {
        let mut c = Chained::new(vec![fr(1), fr(1)], Some(vec!["x".into(), "y".into()])).unwrap();
        respond(&mut c, 1.0); // x -> y
        let o = respond(&mut c, 2.0);
        assert!(o.reinforced);
        assert_eq!(c.active_index(), 0);
        let o2 = respond(&mut c, 3.0);
        assert!(!o2.reinforced);
        assert_eq!(meta_bool(&o2, "chain_transition"), Some(true));
        let o3 = respond(&mut c, 4.0);
        assert!(o3.reinforced);
    }

    #[test]
    fn chained_non_response_tick_does_not_transition() {
        let mut c = Chained::new(vec![fr(2), fr(2)], Some(vec!["a".into(), "b".into()])).unwrap();
        let o = c.step(1.0, None).unwrap();
        assert!(!o.reinforced);
        assert_eq!(meta_str(&o, "current_component").unwrap(), "a");
        assert!(!o.meta.contains_key("chain_transition"));
        assert_eq!(c.active_index(), 0);
    }

    #[test]
    fn chained_reset_returns_to_link_zero() {
        let mut c = Chained::new(vec![fr(1), fr(1)], Some(vec!["a".into(), "b".into()])).unwrap();
        respond(&mut c, 1.0);
        assert_eq!(c.active_index(), 1);
        c.reset();
        assert_eq!(c.active_index(), 0);
        assert_eq!(c.current_stimulus(), "a");
    }

    #[test]
    fn chained_reset_resets_components() {
        let mut c = Chained::new(vec![fr(3), fr(1)], Some(vec!["a".into(), "b".into()])).unwrap();
        respond(&mut c, 1.0);
        respond(&mut c, 2.0);
        c.reset();
        let o1 = respond(&mut c, 1.0);
        let o2 = respond(&mut c, 2.0);
        let o3 = respond(&mut c, 3.0);
        assert!(!o1.reinforced && !o1.meta.contains_key("chain_transition"));
        assert!(!o2.reinforced && !o2.meta.contains_key("chain_transition"));
        assert_eq!(meta_bool(&o3, "chain_transition"), Some(true));
    }

    #[test]
    fn chained_non_monotonic_time_rejected() {
        let mut c = Chained::new(vec![fr(1), fr(1)], None).unwrap();
        c.step(5.0, None).unwrap();
        let err = c.step(1.0, None).unwrap_err();
        assert!(matches!(err, ContingencyError::State(_)));
    }

    #[test]
    fn chained_event_time_mismatch_rejected() {
        let mut c = Chained::new(vec![fr(1), fr(1)], None).unwrap();
        let ev = ResponseEvent::new(2.0);
        let err = c.step(1.0, Some(&ev)).unwrap_err();
        assert!(matches!(err, ContingencyError::State(_)));
    }

    // ----- Tandem -------------------------------------------------------

    #[test]
    fn tandem_behaves_like_chained_without_stimuli() {
        let mut t = Tandem::new(vec![fr(2), fr(3)]).unwrap();
        assert_eq!(t.n_components(), 2);
        assert_eq!(t.active_index(), 0);
        respond(&mut t, 1.0);
        let o_trans = respond(&mut t, 2.0);
        assert!(!o_trans.reinforced);
        assert_eq!(meta_bool(&o_trans, "chain_transition"), Some(true));
        assert_eq!(meta_int(&o_trans, "current_component"), Some(1));
        assert_eq!(t.active_index(), 1);
        assert!(t.is_terminal());
    }

    #[test]
    fn tandem_terminal_primary_reinforcer() {
        let mut t = Tandem::new(vec![fr(1), fr(2)]).unwrap();
        respond(&mut t, 1.0); // transition
        respond(&mut t, 2.0);
        let o = respond(&mut t, 3.0);
        assert!(o.reinforced);
        let r = o.reinforcer.as_ref().expect("reinforcer");
        assert_eq!(r.time, 3.0);
        assert_eq!(t.active_index(), 0);
        assert_eq!(meta_int(&o, "current_component"), Some(0));
        assert!(!o.meta.contains_key("chain_transition"));
    }

    #[test]
    fn tandem_meta_component_is_int_on_non_reinforced_step() {
        let mut t = Tandem::new(vec![fr(3), fr(1)]).unwrap();
        let o = respond(&mut t, 1.0);
        assert!(!o.reinforced);
        assert_eq!(meta_int(&o, "current_component"), Some(0));
    }

    #[test]
    fn tandem_non_response_tick() {
        let mut t = Tandem::new(vec![fr(2), fr(2)]).unwrap();
        let o = t.step(1.0, None).unwrap();
        assert!(!o.reinforced);
        assert_eq!(meta_int(&o, "current_component"), Some(0));
        assert_eq!(t.active_index(), 0);
    }

    #[test]
    fn tandem_reset_returns_to_link_zero() {
        let mut t = Tandem::new(vec![fr(1), fr(1)]).unwrap();
        respond(&mut t, 1.0);
        assert_eq!(t.active_index(), 1);
        t.reset();
        assert_eq!(t.active_index(), 0);
    }

    #[test]
    fn tandem_reset_resets_components() {
        let mut t = Tandem::new(vec![fr(3), fr(1)]).unwrap();
        respond(&mut t, 1.0);
        respond(&mut t, 2.0);
        t.reset();
        let o1 = respond(&mut t, 1.0);
        let o2 = respond(&mut t, 2.0);
        let o3 = respond(&mut t, 3.0);
        assert!(!o1.reinforced && !o1.meta.contains_key("chain_transition"));
        assert!(!o2.reinforced && !o2.meta.contains_key("chain_transition"));
        assert_eq!(meta_bool(&o3, "chain_transition"), Some(true));
    }

    #[test]
    fn tandem_non_monotonic_time_rejected() {
        let mut t = Tandem::new(vec![fr(1), fr(1)]).unwrap();
        t.step(5.0, None).unwrap();
        let err = t.step(1.0, None).unwrap_err();
        assert!(matches!(err, ContingencyError::State(_)));
    }

    #[test]
    fn tandem_event_time_mismatch_rejected() {
        let mut t = Tandem::new(vec![fr(1), fr(1)]).unwrap();
        let ev = ResponseEvent::new(2.0);
        let err = t.step(1.0, Some(&ev)).unwrap_err();
        assert!(matches!(err, ContingencyError::State(_)));
    }

    // ----- Config errors ------------------------------------------------

    #[test]
    fn multiple_requires_two_components() {
        let err = Multiple::new(vec![fr(1)], None).unwrap_err();
        assert!(matches!(err, ContingencyError::Config(_)));
    }

    #[test]
    fn chained_requires_two_components() {
        let err = Chained::new(vec![fr(1)], None).unwrap_err();
        assert!(matches!(err, ContingencyError::Config(_)));
    }

    #[test]
    fn tandem_requires_two_components() {
        let err = Tandem::new(vec![fr(1)]).unwrap_err();
        assert!(matches!(err, ContingencyError::Config(_)));
    }

    #[test]
    fn multiple_empty_rejected() {
        let err = Multiple::new(vec![], None).unwrap_err();
        assert!(matches!(err, ContingencyError::Config(_)));
    }

    #[test]
    fn multiple_stimuli_length_mismatch_rejected() {
        let err = Multiple::new(vec![fr(1), fr(1)], Some(vec!["a".into()])).unwrap_err();
        assert!(matches!(err, ContingencyError::Config(_)));
    }

    #[test]
    fn chained_stimuli_length_mismatch_rejected() {
        let err = Chained::new(
            vec![fr(1), fr(1)],
            Some(vec!["a".into(), "b".into(), "c".into()]),
        )
        .unwrap_err();
        assert!(matches!(err, ContingencyError::Config(_)));
    }

    #[test]
    fn multiple_duplicate_stimuli_rejected() {
        let err =
            Multiple::new(vec![fr(1), fr(1)], Some(vec!["red".into(), "red".into()])).unwrap_err();
        assert!(matches!(err, ContingencyError::Config(_)));
    }

    #[test]
    fn chained_duplicate_stimuli_rejected() {
        let err = Chained::new(vec![fr(1), fr(1)], Some(vec!["g".into(), "g".into()])).unwrap_err();
        assert!(matches!(err, ContingencyError::Config(_)));
    }
}

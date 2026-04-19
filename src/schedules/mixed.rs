//! Mixed (MIX) compound schedule.
//!
//! A mixed schedule alternates between two or more component
//! schedules on each reinforcement — exactly like `Multiple` but
//! without distinctive stimuli. The subject receives no external cue
//! indicating which component is currently in effect; only the
//! experimenter's record (the integer `current_component` index in
//! meta) tracks the active component.
//!
//! Inactive components are not stepped — only the active component
//! receives `(now, event)`. On reinforcement the active index
//! advances (wrapping to 0 after the last component).
//!
//! # References
//!
//! Ferster, C. B., & Skinner, B. F. (1957). *Schedules of
//! reinforcement* (pp. 623-626). Appleton-Century-Crofts.

use crate::errors::ContingencyError;
use crate::helpers::checks::{check_event, check_time};
use crate::schedule::Schedule;
use crate::types::{MetaValue, Outcome, ResponseEvent};
use crate::Result;

/// Mixed (`mix`) compound schedule.
///
/// Two or more independent component schedules alternate on
/// reinforcement, with no distinctive stimuli. Exactly one component
/// is active at a time. The active index is exposed as
/// `Outcome.meta["current_component"] = Int(index)`.
pub struct Mixed {
    components: Vec<Box<dyn Schedule>>,
    active: usize,
    last_now: Option<f64>,
}

impl std::fmt::Debug for Mixed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mixed")
            .field("n_components", &self.components.len())
            .field("active", &self.active)
            .field("last_now", &self.last_now)
            .finish()
    }
}

impl Mixed {
    /// Construct a mixed schedule over `>= 2` components.
    pub fn new(components: Vec<Box<dyn Schedule>>) -> Result<Self> {
        if components.len() < 2 {
            return Err(ContingencyError::Config(format!(
                "Mixed requires >= 2 components, got {}",
                components.len()
            )));
        }
        Ok(Self {
            components,
            active: 0,
            last_now: None,
        })
    }

    /// Number of component schedules.
    pub fn n_components(&self) -> usize {
        self.components.len()
    }

    /// Index of the currently active component.
    pub fn active_index(&self) -> usize {
        self.active
    }
}

impl Schedule for Mixed {
    fn step(&mut self, now: f64, event: Option<&ResponseEvent>) -> Result<Outcome> {
        check_time(now, self.last_now)?;
        check_event(now, event)?;
        self.last_now = Some(now);

        let current = self.active;
        let inner = self.components[current].step(now, event)?;

        let mut meta = inner.meta.clone();
        meta.insert(
            "current_component".to_string(),
            MetaValue::Int(current as i64),
        );

        if inner.reinforced {
            self.active = (self.active + 1) % self.components.len();
            return Ok(Outcome {
                reinforced: true,
                reinforcer: inner.reinforcer,
                meta,
            });
        }
        Ok(Outcome {
            reinforced: false,
            reinforcer: None,
            meta,
        })
    }

    fn reset(&mut self) {
        self.active = 0;
        self.last_now = None;
        for c in self.components.iter_mut() {
            c.reset();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schedules::FR;

    fn respond(s: &mut Mixed, now: f64) -> Outcome {
        let ev = ResponseEvent::new(now);
        s.step(now, Some(&ev)).expect("step should succeed")
    }

    fn idx(o: &Outcome) -> i64 {
        match o.meta.get("current_component") {
            Some(MetaValue::Int(i)) => *i,
            other => panic!("current_component missing or wrong type: {:?}", other),
        }
    }

    // --- Construction --------------------------------------------------

    #[test]
    fn accepts_two_schedules() {
        let m = Mixed::new(vec![
            Box::new(FR::new(2).unwrap()),
            Box::new(FR::new(3).unwrap()),
        ])
        .unwrap();
        assert_eq!(m.n_components(), 2);
    }

    #[test]
    fn accepts_three_components() {
        let m = Mixed::new(vec![
            Box::new(FR::new(1).unwrap()),
            Box::new(FR::new(2).unwrap()),
            Box::new(FR::new(3).unwrap()),
        ])
        .unwrap();
        assert_eq!(m.n_components(), 3);
    }

    #[test]
    fn requires_at_least_two_components() {
        let err = Mixed::new(vec![Box::new(FR::new(1).unwrap())]).unwrap_err();
        assert!(matches!(err, ContingencyError::Config(_)));
    }

    #[test]
    fn rejects_empty_components() {
        let err = Mixed::new(vec![]).unwrap_err();
        assert!(matches!(err, ContingencyError::Config(_)));
    }

    #[test]
    fn active_index_starts_at_zero() {
        let m = Mixed::new(vec![
            Box::new(FR::new(1).unwrap()),
            Box::new(FR::new(1).unwrap()),
        ])
        .unwrap();
        assert_eq!(m.active_index(), 0);
    }

    // --- Alternation ---------------------------------------------------

    #[test]
    fn advances_on_each_reinforcement() {
        let mut m = Mixed::new(vec![
            Box::new(FR::new(1).unwrap()),
            Box::new(FR::new(1).unwrap()),
        ])
        .unwrap();
        assert_eq!(m.active_index(), 0);

        let out = respond(&mut m, 1.0);
        assert!(out.reinforced);
        assert_eq!(m.active_index(), 1);

        let out = respond(&mut m, 2.0);
        assert!(out.reinforced);
        assert_eq!(m.active_index(), 0);
    }

    #[test]
    fn three_components_cycle() {
        let mut m = Mixed::new(vec![
            Box::new(FR::new(1).unwrap()),
            Box::new(FR::new(1).unwrap()),
            Box::new(FR::new(1).unwrap()),
        ])
        .unwrap();
        let mut seen = Vec::new();
        for t in 1..=6 {
            let out = respond(&mut m, t as f64);
            seen.push(idx(&out));
        }
        assert_eq!(seen, vec![0, 1, 2, 0, 1, 2]);
    }

    #[test]
    fn non_reinforced_step_does_not_advance() {
        let mut m = Mixed::new(vec![
            Box::new(FR::new(3).unwrap()),
            Box::new(FR::new(3).unwrap()),
        ])
        .unwrap();
        respond(&mut m, 1.0);
        respond(&mut m, 2.0);
        assert_eq!(m.active_index(), 0);
    }

    #[test]
    fn only_active_component_is_stepped() {
        let mut m = Mixed::new(vec![
            Box::new(FR::new(3).unwrap()),
            Box::new(FR::new(2).unwrap()),
        ])
        .unwrap();
        respond(&mut m, 1.0);
        respond(&mut m, 2.0);
        respond(&mut m, 3.0); // component 0 reinforces → switch to 1
        assert_eq!(m.active_index(), 1);
        assert!(!respond(&mut m, 4.0).reinforced);
        assert!(respond(&mut m, 5.0).reinforced);
    }

    // --- Meta ----------------------------------------------------------

    #[test]
    fn current_component_is_int() {
        let mut m = Mixed::new(vec![
            Box::new(FR::new(1).unwrap()),
            Box::new(FR::new(1).unwrap()),
        ])
        .unwrap();
        let out = respond(&mut m, 1.0);
        match out.meta.get("current_component") {
            Some(MetaValue::Int(0)) => {}
            other => panic!("expected Int(0), got {:?}", other),
        }
    }

    #[test]
    fn non_reinforced_step_still_reports_index() {
        let mut m = Mixed::new(vec![
            Box::new(FR::new(3).unwrap()),
            Box::new(FR::new(3).unwrap()),
        ])
        .unwrap();
        let out = respond(&mut m, 1.0);
        assert!(!out.reinforced);
        assert_eq!(idx(&out), 0);
    }

    #[test]
    fn reinforced_step_reports_firing_component() {
        let mut m = Mixed::new(vec![
            Box::new(FR::new(1).unwrap()),
            Box::new(FR::new(1).unwrap()),
        ])
        .unwrap();
        let o1 = respond(&mut m, 1.0);
        assert!(o1.reinforced);
        assert_eq!(idx(&o1), 0);
        let o2 = respond(&mut m, 2.0);
        assert_eq!(idx(&o2), 1);
    }

    // --- Reset ---------------------------------------------------------

    #[test]
    fn reset_returns_to_initial_index() {
        let mut m = Mixed::new(vec![
            Box::new(FR::new(1).unwrap()),
            Box::new(FR::new(1).unwrap()),
            Box::new(FR::new(1).unwrap()),
        ])
        .unwrap();
        respond(&mut m, 1.0);
        respond(&mut m, 2.0);
        assert_eq!(m.active_index(), 2);
        m.reset();
        assert_eq!(m.active_index(), 0);
    }

    #[test]
    fn reset_clears_component_state() {
        let mut m = Mixed::new(vec![
            Box::new(FR::new(3).unwrap()),
            Box::new(FR::new(3).unwrap()),
        ])
        .unwrap();
        respond(&mut m, 1.0);
        respond(&mut m, 2.0);
        m.reset();
        assert!(!respond(&mut m, 10.0).reinforced);
        assert!(!respond(&mut m, 11.0).reinforced);
        assert!(respond(&mut m, 12.0).reinforced);
    }

    #[test]
    fn reset_allows_time_to_restart() {
        let mut m = Mixed::new(vec![
            Box::new(FR::new(1).unwrap()),
            Box::new(FR::new(1).unwrap()),
        ])
        .unwrap();
        m.step(10.0, None).unwrap();
        m.reset();
        m.step(1.0, None).unwrap();
    }

    // --- State errors --------------------------------------------------

    #[test]
    fn non_monotonic_now_raises() {
        let mut m = Mixed::new(vec![
            Box::new(FR::new(1).unwrap()),
            Box::new(FR::new(1).unwrap()),
        ])
        .unwrap();
        m.step(1.0, None).unwrap();
        let err = m.step(0.5, None).unwrap_err();
        assert!(matches!(err, ContingencyError::State(_)));
    }

    #[test]
    fn event_time_mismatch_raises() {
        let mut m = Mixed::new(vec![
            Box::new(FR::new(1).unwrap()),
            Box::new(FR::new(1).unwrap()),
        ])
        .unwrap();
        let ev = ResponseEvent::new(2.0);
        let err = m.step(1.0, Some(&ev)).unwrap_err();
        assert!(matches!(err, ContingencyError::State(_)));
    }

    // --- Reinforcer timing --------------------------------------------

    #[test]
    fn reinforcer_timestamped_at_now() {
        let mut m = Mixed::new(vec![
            Box::new(FR::new(1).unwrap()),
            Box::new(FR::new(1).unwrap()),
        ])
        .unwrap();
        let out = respond(&mut m, 3.25);
        assert!(out.reinforced);
        assert_eq!(out.reinforcer.as_ref().unwrap().time, 3.25);
    }
}

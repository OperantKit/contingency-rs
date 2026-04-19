//! Conjunctive (CONJ, AND) compound schedule.
//!
//! A conjunctive schedule reinforces only when **both** component
//! schedules simultaneously satisfy their individual criteria. The
//! canonical example is `Conj(FR5, FI10)`: reinforcement occurs on a
//! response that is both the 5th (or later) since the last
//! reinforcement *and* emitted after the 10-second interval has
//! elapsed.
//!
//! # Binary shape
//!
//! `Conjunctive` is strictly binary. Compose three or more conjuncts
//! by nesting: `Conjunctive::new(Box::new(Conjunctive::new(a, b)?), c)?`.
//!
//! # Event forwarding & simultaneity semantics
//!
//! Each `step` forwards `(now, event)` to both components so each
//! maintains its own internal state. A conjunctive reinforcement is
//! emitted only when both components would reinforce on the same
//! step. In that case both are reset.
//!
//! # References
//!
//! Ferster, C. B., & Skinner, B. F. (1957). *Schedules of
//! reinforcement* (pp. 508-509). Appleton-Century-Crofts.

use crate::helpers::checks::{check_event, check_time};
use crate::schedule::Schedule;
use crate::types::{MetaValue, Outcome, Reinforcer, ResponseEvent};
use crate::Result;

/// Conjunctive (AND) compound schedule of two components.
///
/// Both components are stepped on every call with the same
/// `(now, event)`. Reinforcement is delivered only when both
/// components would reinforce on the same step; both are then reset.
///
/// # References
///
/// Ferster, C. B., & Skinner, B. F. (1957). *Schedules of
/// reinforcement* (pp. 508-509). Appleton-Century-Crofts.
pub struct Conjunctive {
    first: Box<dyn Schedule>,
    second: Box<dyn Schedule>,
    last_now: Option<f64>,
}

impl Conjunctive {
    /// Construct a binary conjunctive schedule.
    pub fn new(first: Box<dyn Schedule>, second: Box<dyn Schedule>) -> Result<Self> {
        // Both arguments implement Schedule at the type level; no
        // runtime isinstance check needed. The `Result` return type
        // mirrors other schedule constructors for API uniformity.
        Ok(Self {
            first,
            second,
            last_now: None,
        })
    }
}

impl Schedule for Conjunctive {
    fn step(&mut self, now: f64, event: Option<&ResponseEvent>) -> Result<Outcome> {
        check_time(now, self.last_now)?;
        check_event(now, event)?;
        self.last_now = Some(now);

        let first_outcome = self.first.step(now, event)?;
        let second_outcome = self.second.step(now, event)?;

        if first_outcome.reinforced && second_outcome.reinforced {
            self.first.reset();
            self.second.reset();
            let mut out = Outcome {
                reinforced: true,
                reinforcer: Some(Reinforcer::at(now)),
                ..Outcome::default()
            };
            out.meta
                .insert("conjunctive".to_string(), MetaValue::Bool(true));
            return Ok(out);
        }
        Ok(Outcome::empty())
    }

    fn reset(&mut self) {
        self.first.reset();
        self.second.reset();
        self.last_now = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errors::ContingencyError;
    use crate::schedules::{FI, FR, FT};

    fn respond(s: &mut Conjunctive, now: f64) -> Outcome {
        let ev = ResponseEvent::new(now);
        s.step(now, Some(&ev)).expect("step should succeed")
    }

    // --- Basic FR + FI semantics ---------------------------------------

    #[test]
    fn no_reinforcement_until_both_criteria_met() {
        let mut conj = Conjunctive::new(
            Box::new(FR::new(3).unwrap()),
            Box::new(FI::new(5.0).unwrap()),
        )
        .unwrap();
        conj.step(0.0, None).unwrap();
        for t in [1.0, 2.0, 3.0] {
            let out = respond(&mut conj, t);
            assert!(!out.reinforced);
        }
    }

    #[test]
    fn reinforces_when_both_fire_same_step() {
        let mut conj = Conjunctive::new(
            Box::new(FR::new(3).unwrap()),
            Box::new(FI::new(5.0).unwrap()),
        )
        .unwrap();
        conj.step(0.0, None).unwrap();
        respond(&mut conj, 1.0);
        respond(&mut conj, 2.0);
        let out = respond(&mut conj, 5.0);
        assert!(out.reinforced);
        let r = out.reinforcer.as_ref().unwrap();
        assert_eq!(r.time, 5.0);
        assert_eq!(out.meta.get("conjunctive"), Some(&MetaValue::Bool(true)));
    }

    #[test]
    fn does_not_reinforce_when_only_fi_fires() {
        let mut conj = Conjunctive::new(
            Box::new(FR::new(3).unwrap()),
            Box::new(FI::new(5.0).unwrap()),
        )
        .unwrap();
        conj.step(0.0, None).unwrap();
        let out = conj.step(6.0, None).unwrap();
        assert!(!out.reinforced);
    }

    // --- FR + FT ------------------------------------------------------

    #[test]
    fn fr_alone_firing_is_suppressed() {
        let mut conj = Conjunctive::new(
            Box::new(FR::new(3).unwrap()),
            Box::new(FT::new(5.0).unwrap()),
        )
        .unwrap();
        conj.step(0.0, None).unwrap();
        respond(&mut conj, 1.0);
        respond(&mut conj, 2.0);
        let out = respond(&mut conj, 3.0);
        assert!(!out.reinforced);
    }

    #[test]
    fn ft_alone_firing_is_suppressed() {
        let mut conj = Conjunctive::new(
            Box::new(FR::new(10).unwrap()),
            Box::new(FT::new(1.0).unwrap()),
        )
        .unwrap();
        conj.step(0.0, None).unwrap();
        let out = respond(&mut conj, 1.0);
        assert!(!out.reinforced);
    }

    // --- Reset semantics ----------------------------------------------

    #[test]
    fn both_components_reset_after_joint_win() {
        let mut conj = Conjunctive::new(
            Box::new(FR::new(3).unwrap()),
            Box::new(FI::new(5.0).unwrap()),
        )
        .unwrap();
        conj.step(0.0, None).unwrap();
        respond(&mut conj, 1.0);
        respond(&mut conj, 2.0);
        let win = respond(&mut conj, 5.0);
        assert!(win.reinforced);

        let out = respond(&mut conj, 6.0);
        assert!(!out.reinforced);
        let out = respond(&mut conj, 7.0);
        assert!(!out.reinforced);
        let out = respond(&mut conj, 11.0);
        assert!(out.reinforced);
    }

    #[test]
    fn explicit_reset_clears_internal_state() {
        let mut conj = Conjunctive::new(
            Box::new(FR::new(3).unwrap()),
            Box::new(FI::new(5.0).unwrap()),
        )
        .unwrap();
        conj.step(0.0, None).unwrap();
        respond(&mut conj, 1.0);
        respond(&mut conj, 2.0);

        conj.reset();

        // After reset monotonic-time bookkeeping cleared — earlier time OK.
        conj.step(0.0, None).unwrap();
        respond(&mut conj, 1.0);
        respond(&mut conj, 2.0);
        let out = respond(&mut conj, 5.0);
        assert!(out.reinforced);
    }

    // --- Event forwarding ---------------------------------------------

    #[test]
    fn event_forwarded_to_both_components() {
        let mut conj = Conjunctive::new(
            Box::new(FR::new(3).unwrap()),
            Box::new(FR::new(3).unwrap()),
        )
        .unwrap();
        respond(&mut conj, 1.0);
        respond(&mut conj, 2.0);
        let out = respond(&mut conj, 3.0);
        assert!(out.reinforced);
    }

    // --- State errors -------------------------------------------------

    #[test]
    fn non_monotonic_now_raises() {
        let mut conj = Conjunctive::new(
            Box::new(FR::new(3).unwrap()),
            Box::new(FI::new(5.0).unwrap()),
        )
        .unwrap();
        conj.step(1.0, None).unwrap();
        let err = conj.step(0.5, None).unwrap_err();
        assert!(matches!(err, ContingencyError::State(_)));
    }

    #[test]
    fn event_time_mismatch_raises() {
        let mut conj = Conjunctive::new(
            Box::new(FR::new(3).unwrap()),
            Box::new(FI::new(5.0).unwrap()),
        )
        .unwrap();
        let ev = ResponseEvent::new(2.0);
        let err = conj.step(1.0, Some(&ev)).unwrap_err();
        assert!(matches!(err, ContingencyError::State(_)));
    }

    // --- Meta ---------------------------------------------------------

    #[test]
    fn reinforced_outcome_has_conjunctive_flag() {
        let mut conj = Conjunctive::new(
            Box::new(FR::new(1).unwrap()),
            Box::new(FR::new(1).unwrap()),
        )
        .unwrap();
        let out = respond(&mut conj, 1.0);
        assert!(out.reinforced);
        assert_eq!(out.meta.get("conjunctive"), Some(&MetaValue::Bool(true)));
    }

    #[test]
    fn non_reinforced_outcome_has_empty_meta() {
        let mut conj = Conjunctive::new(
            Box::new(FR::new(3).unwrap()),
            Box::new(FI::new(5.0).unwrap()),
        )
        .unwrap();
        let out = conj.step(0.0, None).unwrap();
        assert!(!out.reinforced);
        assert!(!out.meta.contains_key("conjunctive"));
    }

    // --- Nested conjunctives ------------------------------------------

    #[test]
    fn three_component_conjunctive_via_nesting() {
        let inner = Conjunctive::new(
            Box::new(FR::new(1).unwrap()),
            Box::new(FR::new(1).unwrap()),
        )
        .unwrap();
        let mut conj =
            Conjunctive::new(Box::new(inner), Box::new(FR::new(1).unwrap())).unwrap();
        let out = respond(&mut conj, 1.0);
        assert!(out.reinforced);
    }
}

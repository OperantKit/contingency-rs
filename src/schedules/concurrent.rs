//! Concurrent compound schedule with Changeover Delay (COD) and
//! Changeover Ratio (COR).
//!
//! A concurrent schedule presents two or more component schedules on
//! distinct *operanda* (keys, levers, buttons). Each component is a
//! standalone [`Schedule`]; responses on operandum `k` are routed to
//! `components[k]`. The composite returns the [`Outcome`] produced by
//! the component that received the event, subject to changeover gating.
//!
//! # Changeover Delay (COD)
//!
//! When the subject switches from operandum A to operandum B, the
//! reinforcer that would otherwise be delivered on B is *suppressed*
//! for `cod` time units after the switch. This discourages rapid
//! alternation and superstitious changeover responding. The timer
//! resets on every subsequent changeover: "since the last changeover".
//!
//! If no changeover has yet occurred (e.g. the very first response of
//! a session), COD is not active. This implements the standard
//! "since-last-changeover" semantics of Catania (1966): each fresh
//! switch re-anchors the COD clock.
//!
//! # Changeover Ratio (COR)
//!
//! With `cor > 0`, a switch is not counted as a changeover until the
//! subject has emitted `cor` *consecutive* responses on the new
//! operandum. The COD timer arms only on the `cor`-th such response
//! (and at that response's timestamp). Responses `1..cor-1` on the
//! new operandum are not yet "a changeover" and are therefore not
//! gated; reinforcement on those early responses is allowed.
//!
//! # Choice analytics NOT in scope
//!
//! This schedule handles only reinforcement *gating*. Choice
//! allocation, exclusive-choice detection, and other analytics live
//! in `session-analyzer` and consume the event log, not the schedule
//! itself.
//!
//! # References
//!
//! Catania, A. C. (1966). Concurrent performances: Reinforcement
//! interaction and response independence. *Journal of the Experimental
//! Analysis of Behavior*, 9(3), 253-263.
//! <https://doi.org/10.1901/jeab.1966.9-253>
//!
//! Herrnstein, R. J. (1961). Relative and absolute strength of
//! response as a function of frequency of reinforcement. *Journal of
//! the Experimental Analysis of Behavior*, 4(3), 267-272.
//! <https://doi.org/10.1901/jeab.1961.4-267>

use indexmap::IndexMap;

use crate::{
    constants::TIME_TOL,
    errors::ContingencyError,
    helpers::checks::{check_event, check_time},
    schedule::Schedule,
    types::{MetaValue, Outcome, ResponseEvent},
    Result,
};

/// Concurrent schedule with changeover gating (COD + COR).
///
/// Construct with an ordered map of component schedules keyed by
/// operandum identifier. Responses bearing `operandum == k` are
/// routed to `components[k]`; all other components are advanced with
/// `None` so that time-based components (FT, VT, RT) can still tick.
///
/// # Priority of the returned outcome
///
/// 1. Event-side reinforcement (not COD-suppressed).
/// 2. First tick-side reinforcement in insertion order (never COD-gated).
/// 3. Event-side un-reinforced outcome, preserving its `meta`
///    (e.g. `cod_suppressed`).
/// 4. Empty outcome.
///
/// Rationale: a time-based component that fires on a step must not
/// be silently dropped just because another operandum also saw a
/// response on the same step. Preferring the event-side outcome when
/// both reinforce matches the single-outcome API; when the event side
/// does not reinforce, tick-side reinforcement is surfaced rather
/// than consumed.
pub struct Concurrent {
    components: IndexMap<String, Box<dyn Schedule>>,
    cod: f64,
    cor: u32,
    last_operandum: Option<String>,
    switch_time: Option<f64>,
    consecutive_new_count: u32,
    last_now: Option<f64>,
}

impl std::fmt::Debug for Concurrent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Concurrent")
            .field("components", &self.components.keys().collect::<Vec<_>>())
            .field("cod", &self.cod)
            .field("cor", &self.cor)
            .field("last_operandum", &self.last_operandum)
            .field("switch_time", &self.switch_time)
            .field("consecutive_new_count", &self.consecutive_new_count)
            .field("last_now", &self.last_now)
            .finish()
    }
}

impl Concurrent {
    /// Construct a concurrent schedule.
    ///
    /// # Parameters
    ///
    /// * `components` — ordered map of operandum key → component
    ///   schedule. Must contain at least two entries. Insertion order
    ///   is preserved for tick-side priority.
    /// * `cod` — Changeover Delay in the caller's time units. After a
    ///   changeover, reinforcement on the new operandum is suppressed
    ///   for `cod` time units. `0.0` disables COD. Must be `>= 0` and
    ///   finite.
    /// * `cor` — Changeover Ratio: number of consecutive responses on
    ///   a new operandum required for the switch to count as a
    ///   changeover. `0` treats every switch as immediate.
    ///
    /// # Errors
    ///
    /// Returns [`ContingencyError::Config`] if `components.len() < 2`
    /// or `cod` is negative / non-finite.
    pub fn new(
        components: IndexMap<String, Box<dyn Schedule>>,
        cod: f64,
        cor: u32,
    ) -> Result<Self> {
        if components.len() < 2 {
            return Err(ContingencyError::Config(format!(
                "Concurrent requires >= 2 components, got {}",
                components.len()
            )));
        }
        if !cod.is_finite() || cod < 0.0 {
            return Err(ContingencyError::Config(format!(
                "Concurrent requires cod >= 0 and finite, got {cod}"
            )));
        }
        Ok(Self {
            components,
            cod,
            cor,
            last_operandum: None,
            switch_time: None,
            consecutive_new_count: 0,
            last_now: None,
        })
    }

    /// The configured changeover delay.
    pub fn cod(&self) -> f64 {
        self.cod
    }

    /// The configured changeover ratio.
    pub fn cor(&self) -> u32 {
        self.cor
    }

    /// The set of component keys in insertion order.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.components.keys().map(|s| s.as_str())
    }

    /// Advance all components. Only `event_operandum` receives
    /// `event`; the rest receive `None`. Returns the selected
    /// outcome and a flag indicating whether it originated from the
    /// event-matched component.
    fn advance_components(
        &mut self,
        now: f64,
        event_operandum: Option<&str>,
        event: Option<&ResponseEvent>,
    ) -> Result<(Outcome, bool)> {
        let mut outcome_for_event: Option<Outcome> = None;
        let mut tick_outcome: Option<Outcome> = None;

        for (key, component) in self.components.iter_mut() {
            let is_event_component = match event_operandum {
                Some(op) => key == op,
                None => false,
            };
            if is_event_component {
                outcome_for_event = Some(component.step(now, event)?);
            } else {
                let out = component.step(now, None)?;
                if tick_outcome.is_none() && out.reinforced {
                    tick_outcome = Some(out);
                }
            }
        }

        if let Some(out) = outcome_for_event.as_ref() {
            if out.reinforced {
                return Ok((outcome_for_event.unwrap(), true));
            }
        }
        if let Some(tick) = tick_outcome {
            return Ok((tick, false));
        }
        if let Some(out) = outcome_for_event {
            return Ok((out, true));
        }
        Ok((Outcome::empty(), false))
    }

    /// Update changeover bookkeeping for a response at `operandum`.
    fn register_event(&mut self, operandum: &str, now: f64) {
        match self.last_operandum.as_deref() {
            None => {
                // First response ever: not a changeover.
                self.last_operandum = Some(operandum.to_string());
                self.consecutive_new_count = 0;
            }
            Some(last) if last == operandum => {
                // Stayed on the same operandum: reset streak.
                self.consecutive_new_count = 0;
            }
            Some(_) => {
                // Differs from previously confirmed operandum.
                if self.cor == 0 {
                    // Immediate changeover.
                    self.switch_time = Some(now);
                    self.last_operandum = Some(operandum.to_string());
                    self.consecutive_new_count = 0;
                } else {
                    // Build the streak; only confirm on the cor-th response.
                    self.consecutive_new_count += 1;
                    if self.consecutive_new_count >= self.cor {
                        self.switch_time = Some(now);
                        self.last_operandum = Some(operandum.to_string());
                        self.consecutive_new_count = 0;
                    }
                }
            }
        }
    }

    /// Is reinforcement on `operandum` currently COD-gated?
    fn cod_active(&self, operandum: &str, now: f64) -> bool {
        if self.cod <= 0.0 {
            return false;
        }
        let Some(switch_time) = self.switch_time else {
            return false;
        };
        let Some(ref last) = self.last_operandum else {
            return false;
        };
        if last != operandum {
            return false;
        }
        (now - switch_time) < self.cod - TIME_TOL
    }
}

impl Schedule for Concurrent {
    fn step(&mut self, now: f64, event: Option<&ResponseEvent>) -> Result<Outcome> {
        check_time(now, self.last_now)?;
        check_event(now, event)?;
        self.last_now = Some(now);

        let Some(event) = event else {
            // Pure tick: advance every component. A time-based
            // component may fire; its outcome is returned as-is
            // because no changeover is involved.
            let (outcome, _) = self.advance_components(now, None, None)?;
            return Ok(outcome);
        };

        let operandum = event.operandum.clone();
        if !self.components.contains_key(&operandum) {
            let mut known: Vec<&str> = self.components.keys().map(|s| s.as_str()).collect();
            known.sort_unstable();
            return Err(ContingencyError::Config(format!(
                "Response event references unknown operandum {operandum:?}; \
                 known operanda: {known:?}"
            )));
        }

        // Step components; only the matched one sees the event.
        // Advance before gating so that a suppressed reinforcer still
        // advances the component's state.
        let (outcome, from_event) =
            self.advance_components(now, Some(&operandum), Some(event))?;

        // Update changeover bookkeeping. This happens before gating so
        // that the response which completes a changeover is itself
        // covered by the COD window it opens (Catania, 1966).
        self.register_event(&operandum, now);

        if outcome.reinforced && from_event && self.cod_active(&operandum, now) {
            return Ok(Outcome::empty()
                .with_meta("cod_suppressed", MetaValue::Bool(true))
                .with_meta("operandum", MetaValue::Str(operandum)));
        }
        Ok(outcome)
    }

    fn reset(&mut self) {
        for (_, component) in self.components.iter_mut() {
            component.reset();
        }
        self.last_operandum = None;
        self.switch_time = None;
        self.consecutive_new_count = 0;
        self.last_now = None;
    }
}

// ---------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schedules::{ratio::FR, time_based::FT};

    fn make_two(left: Box<dyn Schedule>, right: Box<dyn Schedule>) -> IndexMap<String, Box<dyn Schedule>> {
        let mut m: IndexMap<String, Box<dyn Schedule>> = IndexMap::new();
        m.insert("left".into(), left);
        m.insert("right".into(), right);
        m
    }

    fn ev(op: &str, t: f64) -> ResponseEvent {
        ResponseEvent::on(op, t)
    }

    // --- Config validation ------------------------------------------------

    #[test]
    fn config_requires_two_components() {
        let mut m: IndexMap<String, Box<dyn Schedule>> = IndexMap::new();
        m.insert("only".into(), Box::new(FR::new(1).unwrap()));
        let err = Concurrent::new(m, 0.0, 0).unwrap_err();
        assert!(matches!(err, ContingencyError::Config(_)));
    }

    #[test]
    fn config_rejects_empty_components() {
        let m: IndexMap<String, Box<dyn Schedule>> = IndexMap::new();
        let err = Concurrent::new(m, 0.0, 0).unwrap_err();
        assert!(matches!(err, ContingencyError::Config(_)));
    }

    #[test]
    fn config_rejects_negative_cod() {
        let comps = make_two(
            Box::new(FR::new(1).unwrap()),
            Box::new(FR::new(1).unwrap()),
        );
        let err = Concurrent::new(comps, -0.1, 0).unwrap_err();
        assert!(matches!(err, ContingencyError::Config(_)));
    }

    #[test]
    fn config_rejects_nan_cod() {
        let comps = make_two(
            Box::new(FR::new(1).unwrap()),
            Box::new(FR::new(1).unwrap()),
        );
        let err = Concurrent::new(comps, f64::NAN, 0).unwrap_err();
        assert!(matches!(err, ContingencyError::Config(_)));
    }

    #[test]
    fn config_defaults_accepted() {
        let comps = make_two(
            Box::new(FR::new(1).unwrap()),
            Box::new(FR::new(1).unwrap()),
        );
        let sch = Concurrent::new(comps, 0.0, 0).unwrap();
        assert_eq!(sch.cod(), 0.0);
        assert_eq!(sch.cor(), 0);
    }

    // --- Basic routing ----------------------------------------------------

    #[test]
    fn two_fr_components_independent_reinforcement() {
        let comps = make_two(
            Box::new(FR::new(3).unwrap()),
            Box::new(FR::new(5).unwrap()),
        );
        let mut sch = Concurrent::new(comps, 0.0, 0).unwrap();

        assert!(!sch.step(0.1, Some(&ev("left", 0.1))).unwrap().reinforced);
        assert!(!sch.step(0.2, Some(&ev("left", 0.2))).unwrap().reinforced);
        let out = sch.step(0.3, Some(&ev("left", 0.3))).unwrap();
        assert!(out.reinforced);
        assert_eq!(out.reinforcer.as_ref().unwrap().time, 0.3);

        for t in [0.4, 0.5, 0.6, 0.7] {
            assert!(!sch.step(t, Some(&ev("right", t))).unwrap().reinforced);
        }
        let out = sch.step(0.8, Some(&ev("right", 0.8))).unwrap();
        assert!(out.reinforced);
        assert_eq!(out.reinforcer.as_ref().unwrap().time, 0.8);
    }

    #[test]
    fn three_component_routing() {
        let mut m: IndexMap<String, Box<dyn Schedule>> = IndexMap::new();
        m.insert("A".into(), Box::new(FR::new(1).unwrap()));
        m.insert("B".into(), Box::new(FR::new(2).unwrap()));
        m.insert("C".into(), Box::new(FR::new(3).unwrap()));
        let mut sch = Concurrent::new(m, 0.0, 0).unwrap();

        assert!(sch.step(0.1, Some(&ev("A", 0.1))).unwrap().reinforced);
        assert!(!sch.step(0.2, Some(&ev("B", 0.2))).unwrap().reinforced);
        assert!(sch.step(0.3, Some(&ev("B", 0.3))).unwrap().reinforced);
        assert!(!sch.step(0.4, Some(&ev("C", 0.4))).unwrap().reinforced);
        assert!(!sch.step(0.5, Some(&ev("C", 0.5))).unwrap().reinforced);
        assert!(sch.step(0.6, Some(&ev("C", 0.6))).unwrap().reinforced);
    }

    #[test]
    fn unknown_operandum_is_config_error() {
        let comps = make_two(
            Box::new(FR::new(1).unwrap()),
            Box::new(FR::new(1).unwrap()),
        );
        let mut sch = Concurrent::new(comps, 0.0, 0).unwrap();
        let err = sch.step(0.1, Some(&ev("center", 0.1))).unwrap_err();
        assert!(matches!(err, ContingencyError::Config(_)));
    }

    #[test]
    fn tick_without_event_does_not_reinforce_response_based() {
        let comps = make_two(
            Box::new(FR::new(1).unwrap()),
            Box::new(FR::new(1).unwrap()),
        );
        let mut sch = Concurrent::new(comps, 0.0, 0).unwrap();
        for t in [0.0, 1.0, 10.0, 100.0] {
            let out = sch.step(t, None).unwrap();
            assert!(!out.reinforced);
            assert!(out.reinforcer.is_none());
        }
    }

    // --- COD semantics ----------------------------------------------------

    #[test]
    fn first_response_not_gated_by_cod() {
        let comps = make_two(
            Box::new(FR::new(1).unwrap()),
            Box::new(FR::new(1).unwrap()),
        );
        let mut sch = Concurrent::new(comps, 1.0, 0).unwrap();
        let out = sch.step(0.0, Some(&ev("left", 0.0))).unwrap();
        assert!(out.reinforced);
    }

    #[test]
    fn switch_within_cod_suppresses_with_meta() {
        let comps = make_two(
            Box::new(FR::new(1).unwrap()),
            Box::new(FR::new(1).unwrap()),
        );
        let mut sch = Concurrent::new(comps, 1.0, 0).unwrap();
        sch.step(0.0, Some(&ev("left", 0.0))).unwrap();
        let out = sch.step(0.5, Some(&ev("right", 0.5))).unwrap();
        assert!(!out.reinforced);
        assert!(out.reinforcer.is_none());
        assert_eq!(
            out.meta.get("cod_suppressed"),
            Some(&MetaValue::Bool(true))
        );
        assert_eq!(
            out.meta.get("operandum"),
            Some(&MetaValue::Str("right".into()))
        );
    }

    #[test]
    fn response_after_cod_reinforces() {
        let comps = make_two(
            Box::new(FR::new(1).unwrap()),
            Box::new(FR::new(1).unwrap()),
        );
        let mut sch = Concurrent::new(comps, 1.0, 0).unwrap();
        sch.step(0.0, Some(&ev("left", 0.0))).unwrap();
        sch.step(0.5, Some(&ev("right", 0.5))).unwrap();
        let out = sch.step(1.6, Some(&ev("right", 1.6))).unwrap();
        assert!(out.reinforced);
        assert_eq!(out.reinforcer.as_ref().unwrap().time, 1.6);
    }

    #[test]
    fn repeated_switches_reset_cod_timer() {
        let comps = make_two(
            Box::new(FR::new(1).unwrap()),
            Box::new(FR::new(1).unwrap()),
        );
        let mut sch = Concurrent::new(comps, 1.0, 0).unwrap();
        sch.step(0.0, Some(&ev("left", 0.0))).unwrap();
        assert!(!sch.step(0.2, Some(&ev("right", 0.2))).unwrap().reinforced);
        assert!(!sch.step(0.5, Some(&ev("left", 0.5))).unwrap().reinforced);
        assert!(!sch.step(0.9, Some(&ev("right", 0.9))).unwrap().reinforced);
        // 1.05 elapsed since last switch at t=0.9.
        assert!(sch.step(1.95, Some(&ev("right", 1.95))).unwrap().reinforced);
    }

    #[test]
    fn non_switching_responses_not_gated() {
        let comps = make_two(
            Box::new(FR::new(1).unwrap()),
            Box::new(FR::new(1).unwrap()),
        );
        let mut sch = Concurrent::new(comps, 5.0, 0).unwrap();
        let out = sch.step(0.0, Some(&ev("left", 0.0))).unwrap();
        assert!(out.reinforced);
        for t in [0.1, 0.2, 0.3] {
            let out = sch.step(t, Some(&ev("left", t))).unwrap();
            assert!(out.reinforced);
        }
    }

    #[test]
    fn cod_zero_never_suppresses() {
        let comps = make_two(
            Box::new(FR::new(1).unwrap()),
            Box::new(FR::new(1).unwrap()),
        );
        let mut sch = Concurrent::new(comps, 0.0, 0).unwrap();
        sch.step(0.0, Some(&ev("left", 0.0))).unwrap();
        let out = sch.step(0.0001, Some(&ev("right", 0.0001))).unwrap();
        assert!(out.reinforced);
        assert!(!out.meta.contains_key("cod_suppressed"));
    }

    #[test]
    fn cod_exactly_at_boundary_is_not_suppressed() {
        let comps = make_two(
            Box::new(FR::new(1).unwrap()),
            Box::new(FR::new(1).unwrap()),
        );
        let mut sch = Concurrent::new(comps, 1.0, 0).unwrap();
        sch.step(0.0, Some(&ev("left", 0.0))).unwrap();
        sch.step(0.5, Some(&ev("right", 0.5))).unwrap();
        // delta == 1.0 exactly → past the edge.
        let out = sch.step(1.5, Some(&ev("right", 1.5))).unwrap();
        assert!(out.reinforced);
    }

    // --- COR semantics ----------------------------------------------------

    #[test]
    fn cor_two_first_response_does_not_arm_cod() {
        let comps = make_two(
            Box::new(FR::new(1).unwrap()),
            Box::new(FR::new(1).unwrap()),
        );
        let mut sch = Concurrent::new(comps, 1.0, 2).unwrap();
        assert!(sch.step(0.0, Some(&ev("left", 0.0))).unwrap().reinforced);
        let out = sch.step(0.5, Some(&ev("right", 0.5))).unwrap();
        assert!(out.reinforced);
        assert!(!out.meta.contains_key("cod_suppressed"));
    }

    #[test]
    fn cor_two_second_response_arms_cod_and_suppresses_itself() {
        let comps = make_two(
            Box::new(FR::new(1).unwrap()),
            Box::new(FR::new(1).unwrap()),
        );
        let mut sch = Concurrent::new(comps, 1.0, 2).unwrap();
        sch.step(0.0, Some(&ev("left", 0.0))).unwrap();
        sch.step(0.5, Some(&ev("right", 0.5))).unwrap();
        let out = sch.step(0.6, Some(&ev("right", 0.6))).unwrap();
        assert!(!out.reinforced);
        assert_eq!(
            out.meta.get("cod_suppressed"),
            Some(&MetaValue::Bool(true))
        );
    }

    #[test]
    fn cor_three_requires_three_consecutive() {
        let comps = make_two(
            Box::new(FR::new(1).unwrap()),
            Box::new(FR::new(1).unwrap()),
        );
        let mut sch = Concurrent::new(comps, 1.0, 3).unwrap();
        sch.step(0.0, Some(&ev("left", 0.0))).unwrap();
        assert!(sch.step(0.1, Some(&ev("right", 0.1))).unwrap().reinforced);
        assert!(sch.step(0.2, Some(&ev("right", 0.2))).unwrap().reinforced);
        let out = sch.step(0.3, Some(&ev("right", 0.3))).unwrap();
        assert!(!out.reinforced);
        assert_eq!(
            out.meta.get("cod_suppressed"),
            Some(&MetaValue::Bool(true))
        );
    }

    #[test]
    fn cor_streak_broken_by_returning_to_old_operandum() {
        let comps = make_two(
            Box::new(FR::new(1).unwrap()),
            Box::new(FR::new(1).unwrap()),
        );
        let mut sch = Concurrent::new(comps, 1.0, 3).unwrap();
        sch.step(0.0, Some(&ev("left", 0.0))).unwrap();
        sch.step(0.1, Some(&ev("right", 0.1))).unwrap();
        sch.step(0.2, Some(&ev("right", 0.2))).unwrap();
        sch.step(0.3, Some(&ev("left", 0.3))).unwrap();
        sch.step(0.4, Some(&ev("right", 0.4))).unwrap();
        sch.step(0.5, Some(&ev("right", 0.5))).unwrap();
        let out = sch.step(0.6, Some(&ev("right", 0.6))).unwrap();
        assert!(!out.reinforced);
        assert_eq!(
            out.meta.get("cod_suppressed"),
            Some(&MetaValue::Bool(true))
        );
    }

    #[test]
    fn cor_streak_on_third_operandum_not_gated_by_prior_cod() {
        let mut m: IndexMap<String, Box<dyn Schedule>> = IndexMap::new();
        m.insert("A".into(), Box::new(FR::new(1).unwrap()));
        m.insert("B".into(), Box::new(FR::new(1).unwrap()));
        m.insert("C".into(), Box::new(FR::new(1).unwrap()));
        let mut sch = Concurrent::new(m, 10.0, 3).unwrap();
        sch.step(0.0, Some(&ev("A", 0.0))).unwrap();
        sch.step(0.1, Some(&ev("B", 0.1))).unwrap();
        sch.step(0.2, Some(&ev("B", 0.2))).unwrap();
        let out = sch.step(0.3, Some(&ev("B", 0.3))).unwrap();
        assert!(!out.reinforced);
        let out = sch.step(0.4, Some(&ev("C", 0.4))).unwrap();
        assert!(out.reinforced);
        assert!(!out.meta.contains_key("cod_suppressed"));
    }

    #[test]
    fn cor_zero_immediate_switch() {
        let comps = make_two(
            Box::new(FR::new(1).unwrap()),
            Box::new(FR::new(1).unwrap()),
        );
        let mut sch = Concurrent::new(comps, 1.0, 0).unwrap();
        sch.step(0.0, Some(&ev("left", 0.0))).unwrap();
        let out = sch.step(0.5, Some(&ev("right", 0.5))).unwrap();
        assert!(!out.reinforced);
        assert_eq!(
            out.meta.get("cod_suppressed"),
            Some(&MetaValue::Bool(true))
        );
    }

    // --- Time advancement -------------------------------------------------

    #[test]
    fn tick_advances_ft_component() {
        let comps = make_two(
            Box::new(FT::new(1.0).unwrap()),
            Box::new(FR::new(1).unwrap()),
        );
        let mut sch = Concurrent::new(comps, 0.0, 0).unwrap();
        sch.step(0.0, None).unwrap();
        let out = sch.step(1.0, None).unwrap();
        assert!(out.reinforced);
        assert_eq!(out.reinforcer.as_ref().unwrap().time, 1.0);
    }

    #[test]
    fn event_on_right_still_ticks_left_ft() {
        let comps = make_two(
            Box::new(FT::new(1.0).unwrap()),
            Box::new(FR::new(2).unwrap()),
        );
        let mut sch = Concurrent::new(comps, 0.0, 0).unwrap();
        sch.step(0.0, None).unwrap();
        sch.step(0.5, Some(&ev("right", 0.5))).unwrap();
        let out = sch.step(1.0, None).unwrap();
        assert!(out.reinforced);
    }

    #[test]
    fn tick_reinforcement_surfaces_even_when_event_present() {
        let comps = make_two(
            Box::new(FT::new(1.0).unwrap()),
            Box::new(FR::new(3).unwrap()),
        );
        let mut sch = Concurrent::new(comps, 0.0, 0).unwrap();
        sch.step(0.0, None).unwrap();
        let out = sch.step(1.0, Some(&ev("right", 1.0))).unwrap();
        assert!(out.reinforced);
        assert_eq!(out.reinforcer.as_ref().unwrap().time, 1.0);
    }

    #[test]
    fn tick_reinforcement_not_gated_by_event_cod() {
        let comps = make_two(
            Box::new(FT::new(1.0).unwrap()),
            Box::new(FR::new(3).unwrap()),
        );
        let mut sch = Concurrent::new(comps, 5.0, 0).unwrap();
        sch.step(0.0, Some(&ev("right", 0.0))).unwrap();
        let out = sch.step(1.0, Some(&ev("right", 1.0))).unwrap();
        assert!(out.reinforced);
        assert!(!out.meta.contains_key("cod_suppressed"));
    }

    // --- State errors -----------------------------------------------------

    #[test]
    fn non_monotonic_time_is_state_error() {
        let comps = make_two(
            Box::new(FR::new(1).unwrap()),
            Box::new(FR::new(1).unwrap()),
        );
        let mut sch = Concurrent::new(comps, 0.0, 0).unwrap();
        sch.step(1.0, None).unwrap();
        let err = sch.step(0.5, None).unwrap_err();
        assert!(matches!(err, ContingencyError::State(_)));
    }

    #[test]
    fn event_time_mismatch_is_state_error() {
        let comps = make_two(
            Box::new(FR::new(1).unwrap()),
            Box::new(FR::new(1).unwrap()),
        );
        let mut sch = Concurrent::new(comps, 0.0, 0).unwrap();
        let err = sch.step(1.0, Some(&ev("left", 1.1))).unwrap_err();
        assert!(matches!(err, ContingencyError::State(_)));
    }

    #[test]
    fn time_within_tolerance_accepted() {
        let comps = make_two(
            Box::new(FR::new(1).unwrap()),
            Box::new(FR::new(1).unwrap()),
        );
        let mut sch = Concurrent::new(comps, 0.0, 0).unwrap();
        sch.step(1.0, None).unwrap();
        sch.step(1.0 - 1e-12, None).unwrap();
    }

    // --- Reset ------------------------------------------------------------

    #[test]
    fn reset_clears_changeover_state() {
        let comps = make_two(
            Box::new(FR::new(1).unwrap()),
            Box::new(FR::new(1).unwrap()),
        );
        let mut sch = Concurrent::new(comps, 1.0, 0).unwrap();
        sch.step(0.0, Some(&ev("left", 0.0))).unwrap();
        sch.step(0.5, Some(&ev("right", 0.5))).unwrap();
        sch.reset();
        let out = sch.step(0.0, Some(&ev("right", 0.0))).unwrap();
        assert!(out.reinforced);
    }

    #[test]
    fn reset_resets_component_state() {
        let comps = make_two(
            Box::new(FR::new(3).unwrap()),
            Box::new(FR::new(3).unwrap()),
        );
        let mut sch = Concurrent::new(comps, 0.0, 0).unwrap();
        sch.step(0.0, Some(&ev("left", 0.0))).unwrap();
        sch.step(0.1, Some(&ev("left", 0.1))).unwrap();
        sch.reset();
        assert!(!sch.step(0.2, Some(&ev("left", 0.2))).unwrap().reinforced);
        assert!(!sch.step(0.3, Some(&ev("left", 0.3))).unwrap().reinforced);
        assert!(sch.step(0.4, Some(&ev("left", 0.4))).unwrap().reinforced);
    }

    #[test]
    fn reset_clears_last_now() {
        let comps = make_two(
            Box::new(FR::new(1).unwrap()),
            Box::new(FR::new(1).unwrap()),
        );
        let mut sch = Concurrent::new(comps, 0.0, 0).unwrap();
        sch.step(10.0, None).unwrap();
        sch.reset();
        sch.step(0.0, None).unwrap();
    }

    #[test]
    fn reset_clears_cor_streak() {
        let comps = make_two(
            Box::new(FR::new(1).unwrap()),
            Box::new(FR::new(1).unwrap()),
        );
        let mut sch = Concurrent::new(comps, 1.0, 2).unwrap();
        sch.step(0.0, Some(&ev("left", 0.0))).unwrap();
        sch.step(0.1, Some(&ev("right", 0.1))).unwrap();
        sch.reset();
        assert!(sch.step(0.0, Some(&ev("right", 0.0))).unwrap().reinforced);
    }

    // --- Suppression meta -------------------------------------------------

    #[test]
    fn suppressed_outcome_carries_full_meta() {
        let comps = make_two(
            Box::new(FR::new(1).unwrap()),
            Box::new(FR::new(1).unwrap()),
        );
        let mut sch = Concurrent::new(comps, 1.0, 0).unwrap();
        sch.step(0.0, Some(&ev("left", 0.0))).unwrap();
        let out = sch.step(0.3, Some(&ev("right", 0.3))).unwrap();
        assert!(!out.reinforced);
        assert!(out.reinforcer.is_none());
        assert_eq!(out.meta.len(), 2);
        assert_eq!(
            out.meta.get("cod_suppressed"),
            Some(&MetaValue::Bool(true))
        );
        assert_eq!(
            out.meta.get("operandum"),
            Some(&MetaValue::Str("right".into()))
        );
    }

    #[test]
    fn non_suppressed_outcome_has_no_cod_meta() {
        let comps = make_two(
            Box::new(FR::new(1).unwrap()),
            Box::new(FR::new(1).unwrap()),
        );
        let mut sch = Concurrent::new(comps, 1.0, 0).unwrap();
        let out = sch.step(0.0, Some(&ev("left", 0.0))).unwrap();
        assert!(!out.meta.contains_key("cod_suppressed"));
    }

    #[test]
    fn suppression_consumes_component_reinforcement() {
        // Left FR(1), right FR(2) — the suppressed reinforcement must
        // still advance the right component's count (so a fresh FR(2)
        // must be completed to reinforce again).
        let comps = make_two(
            Box::new(FR::new(1).unwrap()),
            Box::new(FR::new(2).unwrap()),
        );
        let mut sch = Concurrent::new(comps, 10.0, 0).unwrap();
        sch.step(0.0, Some(&ev("left", 0.0))).unwrap();
        sch.step(1.0, Some(&ev("right", 1.0))).unwrap();
        let out = sch.step(1.5, Some(&ev("right", 1.5))).unwrap();
        assert!(!out.reinforced);
        assert_eq!(
            out.meta.get("cod_suppressed"),
            Some(&MetaValue::Bool(true))
        );
        // First response after COD — count goes to 1.
        let out = sch.step(12.0, Some(&ev("right", 12.0))).unwrap();
        assert!(!out.reinforced);
        // Second response after COD — FR(2) fires.
        let out = sch.step(12.1, Some(&ev("right", 12.1))).unwrap();
        assert!(out.reinforced);
    }
}

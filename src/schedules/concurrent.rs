//! Concurrent compound schedule with Changeover Delay (COD) and
//! Fixed-Ratio Changeover (FRCO).
//!
//! A concurrent schedule presents two or more component schedules on
//! distinct *operanda* (keys, levers, buttons). Each component is a
//! standalone [`Schedule`]; responses on operandum `k` are routed to
//! `components[k]`. The composite returns the [`Outcome`] produced by
//! the component that received the event, subject to changeover gating.
//!
//! # Changeover Delay (COD)
//!
//! When the subject switches from operandum A to operandum B, no
//! response on B is reinforced for `cod` time units after the switch.
//! This discourages rapid alternation and superstitious changeover
//! responding. The timer resets on every subsequent changeover: "since
//! the last changeover".
//!
//! A reinforcer that B's component sets up during the COD is *held*,
//! not lost: it is delivered by the first response on B after the COD
//! ends, and B's component does not start its next interval (or ratio)
//! until that delivery. This is the procedure of Herrnstein (1961):
//! "no response was reinforced within 1.5 seconds of a changeover",
//! with the programmed reinforcer kept until collected.
//!
//! If no changeover has yet occurred (e.g. the very first response of
//! a session), COD is not active. This implements the standard
//! "since-last-changeover" semantics of Catania (1966): each fresh
//! switch re-anchors the COD clock.
//!
//! # Fixed-Ratio Changeover (FRCO)
//!
//! With `frco > 0`, a switch is not counted as a changeover until the
//! subject has emitted `frco` *consecutive* responses on the new
//! operandum. The COD timer arms only on the `frco`-th such response
//! (and at that response's timestamp). Responses `1..frco-1` on the
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

use std::collections::VecDeque;

use indexmap::IndexMap;

use crate::{
    constants::TIME_TOL,
    errors::ContingencyError,
    helpers::checks::{check_event, check_time},
    schedule::Schedule,
    types::{MetaValue, Outcome, Reinforcer, ResponseEvent},
    Result,
};

/// Concurrent schedule with changeover gating (COD + FRCO).
///
/// Construct with an ordered map of component schedules keyed by
/// operandum identifier. Responses bearing `operandum == k` are
/// routed to `components[k]`; all other components are advanced with
/// `None` so that time-based components (FT, VT, RT) can still tick.
///
/// # Priority of the returned outcome
///
/// 1. Event-side reinforcement (not COD-gated).
/// 2. First tick-side reinforcement in insertion order (never COD-gated).
/// 3. Event-side un-reinforced outcome, preserving its `meta`.
/// 4. Empty outcome.
///
/// # Holding reinforcers under COD
///
/// While COD is active on the responded operandum, a reinforcer that
/// the response would produce is held and the step returns an
/// un-reinforced outcome carrying `meta["cod_suppressed"] = true` and
/// `meta["operandum"]`. The first response on that operandum after the
/// COD collects the held reinforcer. If another component delivers a
/// reinforcer on its tick at that step and no punisher fires, that
/// reinforcer is returned instead, with `meta["cod_suppressed"] = true`
/// and no `"operandum"` key. How it is held depends on the component:
///
/// - FR, VR, RR, FI, VI, RI ([`Schedule::probe_response`] returns
///   `Some`): the response is first tried on a clone. If the clone
///   would reinforce, the real component receives only a time tick, so
///   the reinforcer stays set up and its next interval or ratio does
///   not start until delivery. A response that would not reinforce is
///   applied as usual, so it counts toward a ratio.
/// - Any other component (time-based, differential, nested compound):
///   the response is applied as usual, so the component sees every
///   response, and the reinforcer it produces is kept by `Concurrent`
///   until delivery. The component's own next cycle starts at once.
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
    frco: u32,
    cod_directional: Option<IndexMap<(String, String), f64>>,
    punish: Option<IndexMap<String, Box<dyn Schedule>>>,
    last_operandum: Option<String>,
    switch_time: Option<f64>,
    active_cod: f64,
    consecutive_new_count: u32,
    last_now: Option<f64>,
    /// Reinforcers held by the compound (components without a
    /// [`Schedule::probe_response`]), per operandum, oldest first.
    pending: IndexMap<String, VecDeque<Reinforcer>>,
}

impl std::fmt::Debug for Concurrent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Concurrent")
            .field("components", &self.components.keys().collect::<Vec<_>>())
            .field("cod", &self.cod)
            .field("frco", &self.frco)
            .field(
                "cod_directional",
                &self
                    .cod_directional
                    .as_ref()
                    .map(|m| m.keys().collect::<Vec<_>>()),
            )
            .field(
                "punish",
                &self.punish.as_ref().map(|m| m.keys().collect::<Vec<_>>()),
            )
            .field("last_operandum", &self.last_operandum)
            .field("switch_time", &self.switch_time)
            .field("active_cod", &self.active_cod)
            .field("consecutive_new_count", &self.consecutive_new_count)
            .field("last_now", &self.last_now)
            .field("pending", &self.pending)
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
    ///   changeover, no response on the new operandum is reinforced for
    ///   `cod` time units; a reinforcer set up meanwhile is held until
    ///   the first response after the COD. `0.0` disables COD. Must be
    ///   `>= 0` and finite.
    /// * `frco` — fixed-ratio changeover: number of consecutive responses on
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
        frco: u32,
    ) -> Result<Self> {
        Self::with_extensions(components, cod, frco, None, None)
    }

    /// Extended constructor accepting optional directional COD and
    /// per-operandum punishment schedules.
    ///
    /// * `cod_directional` — per-`(from, to)` override for the base
    ///   `cod`. Keys must be `(from, to)` string pairs; self-
    ///   transitions (`(a, a)`) are rejected. Empty maps are treated
    ///   as `None`.
    /// * `punish` — per-operandum punishment schedule. Keys must be
    ///   operanda present in `components`. Each punisher is stepped
    ///   on every `step` call so its internal clock stays aligned;
    ///   only the event-matched operandum's punisher receives the
    ///   event. When a punisher fires, its reinforcement surfaces as
    ///   a negative-magnitude `SR-` outcome (or a net-magnitude
    ///   compound when the base component also fires).
    pub fn with_extensions(
        components: IndexMap<String, Box<dyn Schedule>>,
        cod: f64,
        frco: u32,
        cod_directional: Option<IndexMap<(String, String), f64>>,
        punish: Option<IndexMap<String, Box<dyn Schedule>>>,
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

        // Validate cod_directional.
        let cod_directional = match cod_directional {
            Some(map) if map.is_empty() => None,
            Some(map) => {
                for (key, value) in map.iter() {
                    let (from_op, to_op) = key;
                    if from_op == to_op {
                        return Err(ContingencyError::Config(format!(
                            "Concurrent cod_directional rejects self-transition ({from_op:?}, {to_op:?})"
                        )));
                    }
                    if !value.is_finite() || *value < 0.0 {
                        return Err(ContingencyError::Config(format!(
                            "Concurrent cod_directional values must be >= 0 and finite, got {value} for ({from_op:?}, {to_op:?})"
                        )));
                    }
                }
                Some(map)
            }
            None => None,
        };

        // Validate punish.
        let punish = match punish {
            Some(map) if map.is_empty() => None,
            Some(map) => {
                for key in map.keys() {
                    if !components.contains_key(key) {
                        let mut known: Vec<&str> = components.keys().map(|s| s.as_str()).collect();
                        known.sort_unstable();
                        return Err(ContingencyError::Config(format!(
                            "Concurrent punish references unknown operandum {key:?}; known operanda: {known:?}"
                        )));
                    }
                }
                Some(map)
            }
            None => None,
        };

        Ok(Self {
            components,
            cod,
            frco,
            cod_directional,
            punish,
            last_operandum: None,
            switch_time: None,
            active_cod: cod,
            consecutive_new_count: 0,
            last_now: None,
            pending: IndexMap::new(),
        })
    }

    /// The configured changeover delay.
    pub fn cod(&self) -> f64 {
        self.cod
    }

    /// The configured fixed-ratio changeover.
    pub fn frco(&self) -> u32 {
        self.frco
    }

    /// The set of component keys in insertion order.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.components.keys().map(|s| s.as_str())
    }

    /// Step `component` with `event` while COD gates its operandum.
    ///
    /// For components with a [`Schedule::probe_response`] the response
    /// is first tried on a clone. If the clone would reinforce, the real
    /// component only receives a time tick, so the reinforcer stays set
    /// up (and its next interval or ratio does not start) until the
    /// first response after the COD. Any other component receives the
    /// response as usual and a reinforcer it produces is queued in
    /// `pending[event.operandum]` for later delivery.
    ///
    /// Returns `(outcome, held)`.
    fn step_gated(
        component: &mut Box<dyn Schedule>,
        pending: &mut IndexMap<String, VecDeque<Reinforcer>>,
        now: f64,
        event: &ResponseEvent,
    ) -> Result<(Outcome, bool)> {
        if let Some(would_reinforce) = component.probe_response(now, event) {
            if would_reinforce? {
                component.step(now, None)?;
                return Ok((Outcome::empty(), true));
            }
            return Ok((component.step(now, Some(event))?, false));
        }
        let out = component.step(now, Some(event))?;
        if out.reinforced {
            if let Some(reinforcer) = out.reinforcer {
                pending
                    .entry(event.operandum.clone())
                    .or_default()
                    .push_back(reinforcer);
                return Ok((Outcome::empty(), true));
            }
        }
        Ok((out, false))
    }

    /// Deliver the oldest reinforcer held by `Concurrent` on
    /// `operandum`. Called for an ungated response. If the component
    /// reinforced on this response too, that reinforcer joins the back
    /// of the queue, so each response delivers at most one reinforcer.
    fn deliver_pending(
        &mut self,
        operandum: &str,
        now: f64,
        outcome: Outcome,
        from_event: bool,
    ) -> (Outcome, bool) {
        let Some(queue) = self.pending.get_mut(operandum) else {
            return (outcome, from_event);
        };
        if from_event && outcome.reinforced {
            if let Some(r) = outcome.reinforcer {
                queue.push_back(r);
            }
        }
        let held = queue.pop_front().expect("pending queues are never empty");
        if queue.is_empty() {
            self.pending.shift_remove(operandum);
        }
        let delivered = Reinforcer {
            time: now,
            magnitude: held.magnitude,
            label: held.label,
        };
        (Outcome::reinforced(delivered), true)
    }

    /// Advance all components. Only `event_operandum` receives
    /// `event`; the rest receive `None`. Returns the selected outcome,
    /// a flag indicating whether it originated from the event-matched
    /// component, and a flag indicating whether that component set up
    /// a reinforcer which COD is holding back (only when `gated`).
    fn advance_components(
        &mut self,
        now: f64,
        event_operandum: Option<&str>,
        event: Option<&ResponseEvent>,
        gated: bool,
    ) -> Result<(Outcome, bool, bool)> {
        let mut outcome_for_event: Option<Outcome> = None;
        let mut tick_outcome: Option<Outcome> = None;
        let mut held = false;

        for (key, component) in self.components.iter_mut() {
            let is_event_component = match event_operandum {
                Some(op) => key == op,
                None => false,
            };
            if is_event_component {
                match event {
                    Some(ev) if gated => {
                        let (out, h) = Self::step_gated(component, &mut self.pending, now, ev)?;
                        outcome_for_event = Some(out);
                        held = h;
                    }
                    _ => outcome_for_event = Some(component.step(now, event)?),
                }
            } else {
                let out = component.step(now, None)?;
                if tick_outcome.is_none() && out.reinforced {
                    tick_outcome = Some(out);
                }
            }
        }

        if let Some(out) = outcome_for_event.as_ref() {
            if out.reinforced {
                return Ok((outcome_for_event.unwrap(), true, held));
            }
        }
        if let Some(tick) = tick_outcome {
            return Ok((tick, false, held));
        }
        if let Some(out) = outcome_for_event {
            return Ok((out, true, held));
        }
        Ok((Outcome::empty(), false, held))
    }

    /// Resolve the COD to arm for a switch `from_op -> to_op`. Uses
    /// the directional override when present; otherwise the base.
    fn resolve_cod(&self, from_op: &str, to_op: &str) -> f64 {
        if let Some(map) = self.cod_directional.as_ref() {
            if let Some(v) = map.get(&(from_op.to_string(), to_op.to_string())) {
                return *v;
            }
        }
        self.cod
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
                if self.frco == 0 {
                    // Immediate changeover.
                    let from_op = self.last_operandum.clone().unwrap();
                    self.switch_time = Some(now);
                    self.active_cod = self.resolve_cod(&from_op, operandum);
                    self.last_operandum = Some(operandum.to_string());
                    self.consecutive_new_count = 0;
                } else {
                    // Build the streak; only confirm on the frco-th response.
                    self.consecutive_new_count += 1;
                    if self.consecutive_new_count >= self.frco {
                        let from_op = self.last_operandum.clone().unwrap();
                        self.switch_time = Some(now);
                        self.active_cod = self.resolve_cod(&from_op, operandum);
                        self.last_operandum = Some(operandum.to_string());
                        self.consecutive_new_count = 0;
                    }
                }
            }
        }
    }

    /// Is reinforcement on `operandum` currently COD-gated?
    fn cod_active(&self, operandum: &str, now: f64) -> bool {
        if self.active_cod <= 0.0 {
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
        (now - switch_time) < self.active_cod - TIME_TOL
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
            let (outcome, _, _) = self.advance_components(now, None, None, false)?;
            // Also advance punishment components' clocks on pure ticks.
            if let Some(punish) = self.punish.as_mut() {
                for p in punish.values_mut() {
                    let _ = p.step(now, None)?;
                }
            }
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

        // Update changeover bookkeeping. This happens before gating so
        // that the response which completes a changeover is itself
        // covered by the COD window it opens (Catania, 1966).
        self.register_event(&operandum, now);
        let gated = self.cod_active(&operandum, now);

        // Step components; only the matched one sees the event. Under
        // COD, a reinforcer the matched component would deliver is held
        // until the first response after the COD (Herrnstein, 1961).
        let (outcome, from_event, held) =
            self.advance_components(now, Some(&operandum), Some(event), gated)?;
        let (outcome, from_event) = if gated {
            (outcome, from_event)
        } else {
            self.deliver_pending(&operandum, now, outcome, from_event)
        };

        // Advance punishment schedules; only the matched operandum's
        // punisher receives the event, the rest receive a tick.
        let mut punish_outcome: Option<Outcome> = None;
        if let Some(punish) = self.punish.as_mut() {
            for (key, p) in punish.iter_mut() {
                if key == &operandum {
                    punish_outcome = Some(p.step(now, Some(event))?);
                } else {
                    let _ = p.step(now, None)?;
                }
            }
        }

        let cod_suppressed = held;
        let base_fired = outcome.reinforced && from_event;
        let punisher_fired = punish_outcome.as_ref().is_some_and(|p| p.reinforced);

        if cod_suppressed && !punisher_fired {
            // The reinforcer stays held and is not delivered on this
            // response. A reinforcer another component delivered on its
            // tick at this step is still returned, with `cod_suppressed`
            // recording the hold.
            if outcome.reinforced && !from_event {
                return Ok(outcome.with_meta("cod_suppressed", MetaValue::Bool(true)));
            }
            return Ok(Outcome::empty()
                .with_meta("cod_suppressed", MetaValue::Bool(true))
                .with_meta("operandum", MetaValue::Str(operandum)));
        }

        if base_fired && punisher_fired {
            let b = outcome.reinforcer.as_ref().unwrap();
            let p = punish_outcome
                .as_ref()
                .unwrap()
                .reinforcer
                .as_ref()
                .unwrap();
            let net = b.magnitude - p.magnitude;
            let label = if net > 0.0 {
                "SR+"
            } else if net < 0.0 {
                "SR-"
            } else {
                "SR0"
            };
            let mut out = Outcome {
                reinforced: true,
                reinforcer: Some(Reinforcer {
                    time: b.time,
                    magnitude: net,
                    label: label.to_string(),
                }),
                ..Outcome::default()
            };
            out.meta
                .insert("operandum".to_string(), MetaValue::Str(operandum));
            out.meta
                .insert("base_fired".to_string(), MetaValue::Bool(true));
            out.meta
                .insert("punisher_fired".to_string(), MetaValue::Bool(true));
            out.meta
                .insert("net_magnitude".to_string(), MetaValue::Float(net));
            return Ok(out);
        }

        if punisher_fired {
            let p = punish_outcome
                .as_ref()
                .unwrap()
                .reinforcer
                .as_ref()
                .unwrap();
            let mut out = Outcome {
                reinforced: true,
                reinforcer: Some(Reinforcer {
                    time: p.time,
                    magnitude: -p.magnitude,
                    label: "SR-".to_string(),
                }),
                ..Outcome::default()
            };
            out.meta
                .insert("operandum".to_string(), MetaValue::Str(operandum));
            out.meta
                .insert("punisher_fired".to_string(), MetaValue::Bool(true));
            out.meta
                .insert("base_fired".to_string(), MetaValue::Bool(false));
            if cod_suppressed {
                out.meta
                    .insert("cod_suppressed".to_string(), MetaValue::Bool(true));
            }
            return Ok(out);
        }

        Ok(outcome)
    }

    fn reset(&mut self) {
        for (_, component) in self.components.iter_mut() {
            component.reset();
        }
        if let Some(punish) = self.punish.as_mut() {
            for p in punish.values_mut() {
                p.reset();
            }
        }
        self.last_operandum = None;
        self.switch_time = None;
        self.active_cod = self.cod;
        self.consecutive_new_count = 0;
        self.last_now = None;
        self.pending.clear();
    }
}

// ---------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schedules::{
        differential::{DroMode, DRO},
        interval::FI,
        ratio::FR,
        time_based::FT,
    };

    fn make_two(
        left: Box<dyn Schedule>,
        right: Box<dyn Schedule>,
    ) -> IndexMap<String, Box<dyn Schedule>> {
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
        let comps = make_two(Box::new(FR::new(1).unwrap()), Box::new(FR::new(1).unwrap()));
        let err = Concurrent::new(comps, -0.1, 0).unwrap_err();
        assert!(matches!(err, ContingencyError::Config(_)));
    }

    #[test]
    fn config_rejects_nan_cod() {
        let comps = make_two(Box::new(FR::new(1).unwrap()), Box::new(FR::new(1).unwrap()));
        let err = Concurrent::new(comps, f64::NAN, 0).unwrap_err();
        assert!(matches!(err, ContingencyError::Config(_)));
    }

    #[test]
    fn config_defaults_accepted() {
        let comps = make_two(Box::new(FR::new(1).unwrap()), Box::new(FR::new(1).unwrap()));
        let sch = Concurrent::new(comps, 0.0, 0).unwrap();
        assert_eq!(sch.cod(), 0.0);
        assert_eq!(sch.frco(), 0);
    }

    // --- Basic routing ----------------------------------------------------

    #[test]
    fn two_fr_components_independent_reinforcement() {
        let comps = make_two(Box::new(FR::new(3).unwrap()), Box::new(FR::new(5).unwrap()));
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
        let comps = make_two(Box::new(FR::new(1).unwrap()), Box::new(FR::new(1).unwrap()));
        let mut sch = Concurrent::new(comps, 0.0, 0).unwrap();
        let err = sch.step(0.1, Some(&ev("center", 0.1))).unwrap_err();
        assert!(matches!(err, ContingencyError::Config(_)));
    }

    #[test]
    fn tick_without_event_does_not_reinforce_response_based() {
        let comps = make_two(Box::new(FR::new(1).unwrap()), Box::new(FR::new(1).unwrap()));
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
        let comps = make_two(Box::new(FR::new(1).unwrap()), Box::new(FR::new(1).unwrap()));
        let mut sch = Concurrent::new(comps, 1.0, 0).unwrap();
        let out = sch.step(0.0, Some(&ev("left", 0.0))).unwrap();
        assert!(out.reinforced);
    }

    #[test]
    fn switch_within_cod_suppresses_with_meta() {
        let comps = make_two(Box::new(FR::new(1).unwrap()), Box::new(FR::new(1).unwrap()));
        let mut sch = Concurrent::new(comps, 1.0, 0).unwrap();
        sch.step(0.0, Some(&ev("left", 0.0))).unwrap();
        let out = sch.step(0.5, Some(&ev("right", 0.5))).unwrap();
        assert!(!out.reinforced);
        assert!(out.reinforcer.is_none());
        assert_eq!(out.meta.get("cod_suppressed"), Some(&MetaValue::Bool(true)));
        assert_eq!(
            out.meta.get("operandum"),
            Some(&MetaValue::Str("right".into()))
        );
    }

    #[test]
    fn response_after_cod_reinforces() {
        let comps = make_two(Box::new(FR::new(1).unwrap()), Box::new(FR::new(1).unwrap()));
        let mut sch = Concurrent::new(comps, 1.0, 0).unwrap();
        sch.step(0.0, Some(&ev("left", 0.0))).unwrap();
        sch.step(0.5, Some(&ev("right", 0.5))).unwrap();
        let out = sch.step(1.6, Some(&ev("right", 1.6))).unwrap();
        assert!(out.reinforced);
        assert_eq!(out.reinforcer.as_ref().unwrap().time, 1.6);
    }

    #[test]
    fn repeated_switches_reset_cod_timer() {
        let comps = make_two(Box::new(FR::new(1).unwrap()), Box::new(FR::new(1).unwrap()));
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
        let comps = make_two(Box::new(FR::new(1).unwrap()), Box::new(FR::new(1).unwrap()));
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
        let comps = make_two(Box::new(FR::new(1).unwrap()), Box::new(FR::new(1).unwrap()));
        let mut sch = Concurrent::new(comps, 0.0, 0).unwrap();
        sch.step(0.0, Some(&ev("left", 0.0))).unwrap();
        let out = sch.step(0.0001, Some(&ev("right", 0.0001))).unwrap();
        assert!(out.reinforced);
        assert!(!out.meta.contains_key("cod_suppressed"));
    }

    #[test]
    fn cod_exactly_at_boundary_is_not_suppressed() {
        let comps = make_two(Box::new(FR::new(1).unwrap()), Box::new(FR::new(1).unwrap()));
        let mut sch = Concurrent::new(comps, 1.0, 0).unwrap();
        sch.step(0.0, Some(&ev("left", 0.0))).unwrap();
        sch.step(0.5, Some(&ev("right", 0.5))).unwrap();
        // delta == 1.0 exactly → past the edge.
        let out = sch.step(1.5, Some(&ev("right", 1.5))).unwrap();
        assert!(out.reinforced);
    }

    // --- FRCO semantics ----------------------------------------------------

    #[test]
    fn cor_two_first_response_does_not_arm_cod() {
        let comps = make_two(Box::new(FR::new(1).unwrap()), Box::new(FR::new(1).unwrap()));
        let mut sch = Concurrent::new(comps, 1.0, 2).unwrap();
        assert!(sch.step(0.0, Some(&ev("left", 0.0))).unwrap().reinforced);
        let out = sch.step(0.5, Some(&ev("right", 0.5))).unwrap();
        assert!(out.reinforced);
        assert!(!out.meta.contains_key("cod_suppressed"));
    }

    #[test]
    fn cor_two_second_response_arms_cod_and_suppresses_itself() {
        let comps = make_two(Box::new(FR::new(1).unwrap()), Box::new(FR::new(1).unwrap()));
        let mut sch = Concurrent::new(comps, 1.0, 2).unwrap();
        sch.step(0.0, Some(&ev("left", 0.0))).unwrap();
        sch.step(0.5, Some(&ev("right", 0.5))).unwrap();
        let out = sch.step(0.6, Some(&ev("right", 0.6))).unwrap();
        assert!(!out.reinforced);
        assert_eq!(out.meta.get("cod_suppressed"), Some(&MetaValue::Bool(true)));
    }

    #[test]
    fn cor_three_requires_three_consecutive() {
        let comps = make_two(Box::new(FR::new(1).unwrap()), Box::new(FR::new(1).unwrap()));
        let mut sch = Concurrent::new(comps, 1.0, 3).unwrap();
        sch.step(0.0, Some(&ev("left", 0.0))).unwrap();
        assert!(sch.step(0.1, Some(&ev("right", 0.1))).unwrap().reinforced);
        assert!(sch.step(0.2, Some(&ev("right", 0.2))).unwrap().reinforced);
        let out = sch.step(0.3, Some(&ev("right", 0.3))).unwrap();
        assert!(!out.reinforced);
        assert_eq!(out.meta.get("cod_suppressed"), Some(&MetaValue::Bool(true)));
    }

    #[test]
    fn cor_streak_broken_by_returning_to_old_operandum() {
        let comps = make_two(Box::new(FR::new(1).unwrap()), Box::new(FR::new(1).unwrap()));
        let mut sch = Concurrent::new(comps, 1.0, 3).unwrap();
        sch.step(0.0, Some(&ev("left", 0.0))).unwrap();
        sch.step(0.1, Some(&ev("right", 0.1))).unwrap();
        sch.step(0.2, Some(&ev("right", 0.2))).unwrap();
        sch.step(0.3, Some(&ev("left", 0.3))).unwrap();
        sch.step(0.4, Some(&ev("right", 0.4))).unwrap();
        sch.step(0.5, Some(&ev("right", 0.5))).unwrap();
        let out = sch.step(0.6, Some(&ev("right", 0.6))).unwrap();
        assert!(!out.reinforced);
        assert_eq!(out.meta.get("cod_suppressed"), Some(&MetaValue::Bool(true)));
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
        let comps = make_two(Box::new(FR::new(1).unwrap()), Box::new(FR::new(1).unwrap()));
        let mut sch = Concurrent::new(comps, 1.0, 0).unwrap();
        sch.step(0.0, Some(&ev("left", 0.0))).unwrap();
        let out = sch.step(0.5, Some(&ev("right", 0.5))).unwrap();
        assert!(!out.reinforced);
        assert_eq!(out.meta.get("cod_suppressed"), Some(&MetaValue::Bool(true)));
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
        let comps = make_two(Box::new(FR::new(1).unwrap()), Box::new(FR::new(1).unwrap()));
        let mut sch = Concurrent::new(comps, 0.0, 0).unwrap();
        sch.step(1.0, None).unwrap();
        let err = sch.step(0.5, None).unwrap_err();
        assert!(matches!(err, ContingencyError::State(_)));
    }

    #[test]
    fn event_time_mismatch_is_state_error() {
        let comps = make_two(Box::new(FR::new(1).unwrap()), Box::new(FR::new(1).unwrap()));
        let mut sch = Concurrent::new(comps, 0.0, 0).unwrap();
        let err = sch.step(1.0, Some(&ev("left", 1.1))).unwrap_err();
        assert!(matches!(err, ContingencyError::State(_)));
    }

    #[test]
    fn time_within_tolerance_accepted() {
        let comps = make_two(Box::new(FR::new(1).unwrap()), Box::new(FR::new(1).unwrap()));
        let mut sch = Concurrent::new(comps, 0.0, 0).unwrap();
        sch.step(1.0, None).unwrap();
        sch.step(1.0 - 1e-12, None).unwrap();
    }

    // --- Reset ------------------------------------------------------------

    #[test]
    fn reset_clears_changeover_state() {
        let comps = make_two(Box::new(FR::new(1).unwrap()), Box::new(FR::new(1).unwrap()));
        let mut sch = Concurrent::new(comps, 1.0, 0).unwrap();
        sch.step(0.0, Some(&ev("left", 0.0))).unwrap();
        sch.step(0.5, Some(&ev("right", 0.5))).unwrap();
        sch.reset();
        let out = sch.step(0.0, Some(&ev("right", 0.0))).unwrap();
        assert!(out.reinforced);
    }

    #[test]
    fn reset_resets_component_state() {
        let comps = make_two(Box::new(FR::new(3).unwrap()), Box::new(FR::new(3).unwrap()));
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
        let comps = make_two(Box::new(FR::new(1).unwrap()), Box::new(FR::new(1).unwrap()));
        let mut sch = Concurrent::new(comps, 0.0, 0).unwrap();
        sch.step(10.0, None).unwrap();
        sch.reset();
        sch.step(0.0, None).unwrap();
    }

    #[test]
    fn reset_clears_cor_streak() {
        let comps = make_two(Box::new(FR::new(1).unwrap()), Box::new(FR::new(1).unwrap()));
        let mut sch = Concurrent::new(comps, 1.0, 2).unwrap();
        sch.step(0.0, Some(&ev("left", 0.0))).unwrap();
        sch.step(0.1, Some(&ev("right", 0.1))).unwrap();
        sch.reset();
        assert!(sch.step(0.0, Some(&ev("right", 0.0))).unwrap().reinforced);
    }

    // --- Suppression meta -------------------------------------------------

    #[test]
    fn suppressed_outcome_carries_full_meta() {
        let comps = make_two(Box::new(FR::new(1).unwrap()), Box::new(FR::new(1).unwrap()));
        let mut sch = Concurrent::new(comps, 1.0, 0).unwrap();
        sch.step(0.0, Some(&ev("left", 0.0))).unwrap();
        let out = sch.step(0.3, Some(&ev("right", 0.3))).unwrap();
        assert!(!out.reinforced);
        assert!(out.reinforcer.is_none());
        assert_eq!(out.meta.len(), 2);
        assert_eq!(out.meta.get("cod_suppressed"), Some(&MetaValue::Bool(true)));
        assert_eq!(
            out.meta.get("operandum"),
            Some(&MetaValue::Str("right".into()))
        );
    }

    #[test]
    fn non_suppressed_outcome_has_no_cod_meta() {
        let comps = make_two(Box::new(FR::new(1).unwrap()), Box::new(FR::new(1).unwrap()));
        let mut sch = Concurrent::new(comps, 1.0, 0).unwrap();
        let out = sch.step(0.0, Some(&ev("left", 0.0))).unwrap();
        assert!(!out.meta.contains_key("cod_suppressed"));
    }

    // --- Directional COD ------------------------------------------------

    #[test]
    fn directional_cod_rejects_self_transition() {
        let comps = make_two(Box::new(FR::new(1).unwrap()), Box::new(FR::new(1).unwrap()));
        let mut dir: IndexMap<(String, String), f64> = IndexMap::new();
        dir.insert(("left".into(), "left".into()), 2.0);
        let err = Concurrent::with_extensions(comps, 0.0, 0, Some(dir), None).unwrap_err();
        assert!(matches!(err, ContingencyError::Config(_)));
    }

    #[test]
    fn directional_cod_rejects_negative_value() {
        let comps = make_two(Box::new(FR::new(1).unwrap()), Box::new(FR::new(1).unwrap()));
        let mut dir: IndexMap<(String, String), f64> = IndexMap::new();
        dir.insert(("left".into(), "right".into()), -1.0);
        let err = Concurrent::with_extensions(comps, 0.0, 0, Some(dir), None).unwrap_err();
        assert!(matches!(err, ContingencyError::Config(_)));
    }

    #[test]
    fn directional_cod_governs_each_direction() {
        let mut m: IndexMap<String, Box<dyn Schedule>> = IndexMap::new();
        m.insert("a".into(), Box::new(FR::new(1).unwrap()));
        m.insert("b".into(), Box::new(FR::new(1).unwrap()));
        let mut dir: IndexMap<(String, String), f64> = IndexMap::new();
        dir.insert(("a".into(), "b".into()), 2.0);
        dir.insert(("b".into(), "a".into()), 1.0);
        let mut sch = Concurrent::with_extensions(m, 0.0, 0, Some(dir), None).unwrap();

        // Anchor on 'a'.
        assert!(sch.step(0.0, Some(&ev("a", 0.0))).unwrap().reinforced);
        // Switch a->b at t=1.5: within 2s COD.
        let out = sch.step(1.5, Some(&ev("b", 1.5))).unwrap();
        assert!(!out.reinforced);
        assert_eq!(out.meta.get("cod_suppressed"), Some(&MetaValue::Bool(true)));
        // Past 2s COD at t=3.6.
        let out = sch.step(3.6, Some(&ev("b", 3.6))).unwrap();
        assert!(out.reinforced);
        // Switch b->a at t=4.0: 1s COD.
        let out = sch.step(4.0, Some(&ev("a", 4.0))).unwrap();
        assert!(!out.reinforced);
        assert_eq!(out.meta.get("cod_suppressed"), Some(&MetaValue::Bool(true)));
        // Past 1s COD at t=5.1.
        let out = sch.step(5.1, Some(&ev("a", 5.1))).unwrap();
        assert!(out.reinforced);
    }

    #[test]
    fn directional_cod_base_fallback_when_entry_missing() {
        let mut m: IndexMap<String, Box<dyn Schedule>> = IndexMap::new();
        m.insert("a".into(), Box::new(FR::new(1).unwrap()));
        m.insert("b".into(), Box::new(FR::new(1).unwrap()));
        m.insert("c".into(), Box::new(FR::new(1).unwrap()));
        let mut dir: IndexMap<(String, String), f64> = IndexMap::new();
        dir.insert(("a".into(), "b".into()), 2.0);
        let mut sch = Concurrent::with_extensions(m, 0.5, 0, Some(dir), None).unwrap();
        // a->c uses base cod (0.5).
        assert!(sch.step(0.0, Some(&ev("a", 0.0))).unwrap().reinforced);
        let out = sch.step(0.1, Some(&ev("c", 0.1))).unwrap();
        assert!(!out.reinforced);
        assert_eq!(out.meta.get("cod_suppressed"), Some(&MetaValue::Bool(true)));
        let out = sch.step(0.7, Some(&ev("c", 0.7))).unwrap();
        assert!(out.reinforced);
    }

    // --- Punish ---------------------------------------------------------

    #[test]
    fn punish_rejects_unknown_operandum() {
        let comps = make_two(Box::new(FR::new(1).unwrap()), Box::new(FR::new(1).unwrap()));
        let mut p: IndexMap<String, Box<dyn Schedule>> = IndexMap::new();
        p.insert("c".into(), Box::new(FR::new(2).unwrap()));
        let err = Concurrent::with_extensions(comps, 0.0, 0, None, Some(p)).unwrap_err();
        assert!(matches!(err, ContingencyError::Config(_)));
    }

    #[test]
    fn punish_reinforcement_and_punishment_net_magnitude() {
        let mut m: IndexMap<String, Box<dyn Schedule>> = IndexMap::new();
        m.insert("a".into(), Box::new(FR::new(1).unwrap()));
        m.insert("b".into(), Box::new(FR::new(1).unwrap()));
        let mut p: IndexMap<String, Box<dyn Schedule>> = IndexMap::new();
        p.insert("a".into(), Box::new(FR::new(1).unwrap()));
        let mut sch = Concurrent::with_extensions(m, 0.0, 0, None, Some(p)).unwrap();
        let out = sch.step(0.0, Some(&ev("a", 0.0))).unwrap();
        assert!(out.reinforced);
        assert_eq!(out.meta.get("base_fired"), Some(&MetaValue::Bool(true)));
        assert_eq!(out.meta.get("punisher_fired"), Some(&MetaValue::Bool(true)));
        match out.meta.get("net_magnitude") {
            Some(MetaValue::Float(v)) => assert!(v.abs() < 1e-9),
            other => panic!("net_magnitude missing/wrong: {:?}", other),
        }
        let r = out.reinforcer.as_ref().unwrap();
        assert_eq!(r.label, "SR0");
    }

    #[test]
    fn punish_only_fires_when_base_does_not() {
        let mut m: IndexMap<String, Box<dyn Schedule>> = IndexMap::new();
        m.insert("a".into(), Box::new(FR::new(2).unwrap()));
        m.insert("b".into(), Box::new(FR::new(1).unwrap()));
        let mut p: IndexMap<String, Box<dyn Schedule>> = IndexMap::new();
        p.insert("a".into(), Box::new(FR::new(1).unwrap()));
        let mut sch = Concurrent::with_extensions(m, 0.0, 0, None, Some(p)).unwrap();
        let out = sch.step(0.0, Some(&ev("a", 0.0))).unwrap();
        assert!(out.reinforced);
        assert_eq!(out.meta.get("punisher_fired"), Some(&MetaValue::Bool(true)));
        assert_eq!(out.meta.get("base_fired"), Some(&MetaValue::Bool(false)));
        let r = out.reinforcer.as_ref().unwrap();
        assert_eq!(r.label, "SR-");
        assert!((r.magnitude - (-1.0)).abs() < 1e-9);
    }

    #[test]
    fn punish_on_other_operandum_does_not_trigger() {
        let mut m: IndexMap<String, Box<dyn Schedule>> = IndexMap::new();
        m.insert("a".into(), Box::new(FR::new(1).unwrap()));
        m.insert("b".into(), Box::new(FR::new(1).unwrap()));
        let mut p: IndexMap<String, Box<dyn Schedule>> = IndexMap::new();
        p.insert("b".into(), Box::new(FR::new(1).unwrap()));
        let mut sch = Concurrent::with_extensions(m, 0.0, 0, None, Some(p)).unwrap();
        // Respond on 'a': base fires, b's punisher should not advance.
        let out = sch.step(0.0, Some(&ev("a", 0.0))).unwrap();
        assert!(out.reinforced);
        let r = out.reinforcer.as_ref().unwrap();
        assert_eq!(r.label, "SR+");
    }

    #[test]
    fn punish_reset_clears_state() {
        let mut m: IndexMap<String, Box<dyn Schedule>> = IndexMap::new();
        m.insert("a".into(), Box::new(FR::new(2).unwrap()));
        m.insert("b".into(), Box::new(FR::new(1).unwrap()));
        let mut p: IndexMap<String, Box<dyn Schedule>> = IndexMap::new();
        p.insert("a".into(), Box::new(FR::new(2).unwrap()));
        let mut sch = Concurrent::with_extensions(m, 0.0, 0, None, Some(p)).unwrap();
        sch.step(0.0, Some(&ev("a", 0.0))).unwrap();
        sch.reset();
        // After reset: neither base (FR2) nor punisher (FR2) fires on 1st response.
        let out = sch.step(1.0, Some(&ev("a", 1.0))).unwrap();
        assert!(!out.reinforced);
    }

    // --- Held reinforcers (Herrnstein, 1961) -----------------------------

    fn make_ab(a: Box<dyn Schedule>, b: Box<dyn Schedule>) -> IndexMap<String, Box<dyn Schedule>> {
        let mut m: IndexMap<String, Box<dyn Schedule>> = IndexMap::new();
        m.insert("a".into(), a);
        m.insert("b".into(), b);
        m
    }

    #[test]
    fn suppressed_reinforcer_is_held_until_cod_ends() {
        let comps = make_ab(
            Box::new(FR::new(1).unwrap()),
            Box::new(FI::new(5.0).unwrap()),
        );
        let mut sch = Concurrent::new(comps, 1.5, 0).unwrap();
        sch.step(0.0, Some(&ev("a", 0.0))).unwrap();
        // FI 5 on b is set up at t=5. Switch a -> b at t=6: within COD.
        let out = sch.step(6.0, Some(&ev("b", 6.0))).unwrap();
        assert!(!out.reinforced);
        assert_eq!(out.meta.get("cod_suppressed"), Some(&MetaValue::Bool(true)));
        // First b response after the 1.5-s COD collects the held reinforcer.
        let out = sch.step(7.6, Some(&ev("b", 7.6))).unwrap();
        assert!(out.reinforced);
        assert_eq!(out.reinforcer.as_ref().unwrap().time, 7.6);
    }

    #[test]
    fn held_reinforcer_survives_switching_back() {
        let comps = make_ab(
            Box::new(FR::new(100).unwrap()),
            Box::new(FI::new(5.0).unwrap()),
        );
        let mut sch = Concurrent::new(comps, 1.5, 0).unwrap();
        sch.step(0.0, Some(&ev("a", 0.0))).unwrap();
        assert!(!sch.step(6.0, Some(&ev("b", 6.0))).unwrap().reinforced);
        sch.step(6.5, Some(&ev("a", 6.5))).unwrap();
        // Back to b: a new COD starts at t=7.
        assert!(!sch.step(7.0, Some(&ev("b", 7.0))).unwrap().reinforced);
        assert!(sch.step(8.6, Some(&ev("b", 8.6))).unwrap().reinforced);
    }

    #[test]
    fn next_interval_starts_at_delivery_not_at_setup() {
        let comps = make_ab(
            Box::new(FR::new(1).unwrap()),
            Box::new(FI::new(5.0).unwrap()),
        );
        let mut sch = Concurrent::new(comps, 1.5, 0).unwrap();
        sch.step(0.0, Some(&ev("a", 0.0))).unwrap();
        sch.step(6.0, Some(&ev("b", 6.0))).unwrap(); // held
        assert!(sch.step(7.6, Some(&ev("b", 7.6))).unwrap().reinforced);
        // Next FI 5 is timed from the delivery at 7.6, so it sets up at 12.6.
        assert!(!sch.step(12.0, Some(&ev("b", 12.0))).unwrap().reinforced);
        assert!(sch.step(12.7, Some(&ev("b", 12.7))).unwrap().reinforced);
    }

    #[test]
    fn time_based_reinforcer_is_held_not_lost() {
        let comps = make_ab(
            Box::new(FR::new(100).unwrap()),
            Box::new(FT::new(5.0).unwrap()),
        );
        let mut sch = Concurrent::new(comps, 2.0, 0).unwrap();
        sch.step(0.0, Some(&ev("a", 0.0))).unwrap();
        // FT 5 fires on the b response at 5.5, which opens a 2-s COD.
        let out = sch.step(5.5, Some(&ev("b", 5.5))).unwrap();
        assert!(!out.reinforced);
        assert_eq!(out.meta.get("cod_suppressed"), Some(&MetaValue::Bool(true)));
        let out = sch.step(7.6, Some(&ev("b", 7.6))).unwrap();
        assert!(out.reinforced);
        assert_eq!(out.reinforcer.as_ref().unwrap().time, 7.6);
    }

    fn reinforced_on(sch: &mut Concurrent, steps: &[(f64, &str)]) -> Vec<bool> {
        steps
            .iter()
            .map(|&(t, op)| sch.step(t, Some(&ev(op, t))).unwrap().reinforced)
            .collect()
    }

    #[test]
    fn tick_reinforcement_survives_hold_on_event_side() {
        // On a step where COD holds the event-side reinforcer, a
        // reinforcer another component delivers on its tick is still
        // returned.
        let comps = make_ab(
            Box::new(FT::new(5.0).unwrap()),
            Box::new(FI::new(3.0).unwrap()),
        );
        let mut sch = Concurrent::new(comps, 2.0, 0).unwrap();
        let got = reinforced_on(&mut sch, &[(0.0, "a"), (4.0, "b"), (5.0, "b"), (7.0, "b")]);
        assert_eq!(got, vec![false, false, true, true]);
    }

    #[test]
    fn tick_reinforcer_on_hold_step_records_hold() {
        let comps = make_ab(
            Box::new(FT::new(5.0).unwrap()),
            Box::new(FI::new(3.0).unwrap()),
        );
        let mut sch = Concurrent::new(comps, 2.0, 0).unwrap();
        sch.step(0.0, Some(&ev("a", 0.0))).unwrap();
        sch.step(4.0, Some(&ev("b", 4.0))).unwrap();
        let out = sch.step(5.0, Some(&ev("b", 5.0))).unwrap();
        assert_eq!(out.reinforcer.as_ref().unwrap().time, 5.0);
        assert_eq!(out.meta.get("cod_suppressed"), Some(&MetaValue::Bool(true)));
        assert!(!out.meta.contains_key("operandum"));
    }

    #[test]
    fn tick_reinforcement_survives_queued_hold() {
        // Same as above when the event-side reinforcer is queued by
        // `Concurrent` (a component it does not hold in place).
        let comps = make_ab(
            Box::new(FT::new(5.0).unwrap()),
            Box::new(FT::new(1.0).unwrap()),
        );
        let mut sch = Concurrent::new(comps, 2.0, 0).unwrap();
        let got = reinforced_on(&mut sch, &[(0.0, "a"), (4.0, "b"), (5.0, "b"), (7.0, "b")]);
        assert_eq!(got, vec![false, false, true, true]);
    }

    #[test]
    fn punisher_on_hold_step_still_wins_over_tick() {
        // When a punisher fires on the hold step, the punisher outcome is
        // returned and the tick reinforcer is not.
        let comps = make_ab(
            Box::new(FT::new(5.0).unwrap()),
            Box::new(FI::new(3.0).unwrap()),
        );
        let mut p: IndexMap<String, Box<dyn Schedule>> = IndexMap::new();
        p.insert("b".into(), Box::new(FR::new(1).unwrap()));
        let mut sch = Concurrent::with_extensions(comps, 2.0, 0, None, Some(p)).unwrap();
        sch.step(0.0, Some(&ev("a", 0.0))).unwrap();
        sch.step(4.0, Some(&ev("b", 4.0))).unwrap();
        let out = sch.step(5.0, Some(&ev("b", 5.0))).unwrap();
        assert_eq!(out.reinforcer.as_ref().unwrap().label, "SR-");
        assert_eq!(out.meta.get("cod_suppressed"), Some(&MetaValue::Bool(true)));
    }

    #[test]
    fn responses_inside_cod_still_reach_dro() {
        let comps = make_ab(
            Box::new(FR::new(100).unwrap()),
            Box::new(DRO::new(2.0, DroMode::Momentary).unwrap()),
        );
        let mut sch = Concurrent::new(comps, 1.0, 0).unwrap();
        sch.step(0.0, Some(&ev("a", 0.0))).unwrap();
        // The b response at 2.0 lands on the DRO boundary inside COD.
        assert!(!sch.step(2.0, Some(&ev("b", 2.0))).unwrap().reinforced);
        sch.step(3.5, None).unwrap();
        // [2, 4) contains the b response at 2.0, so DRO must not pay at 4.0.
        assert!(!sch.step(4.0, None).unwrap().reinforced);
        // The DRO reinforcer set up at 2.0 is collected by the next b response.
        assert!(sch.step(4.5, Some(&ev("b", 4.5))).unwrap().reinforced);
    }

    #[test]
    fn held_ratio_reinforcer_restarts_count_after_delivery() {
        let comps = make_two(Box::new(FR::new(1).unwrap()), Box::new(FR::new(2).unwrap()));
        let mut sch = Concurrent::new(comps, 10.0, 0).unwrap();
        sch.step(0.0, Some(&ev("left", 0.0))).unwrap();
        sch.step(1.0, Some(&ev("right", 1.0))).unwrap();
        // Ratio completed inside COD: held, not delivered.
        let out = sch.step(1.5, Some(&ev("right", 1.5))).unwrap();
        assert!(!out.reinforced);
        assert_eq!(out.meta.get("cod_suppressed"), Some(&MetaValue::Bool(true)));
        // First response after COD collects it.
        assert!(sch.step(12.0, Some(&ev("right", 12.0))).unwrap().reinforced);
        // The count then restarts: two fresh responses are needed.
        assert!(!sch.step(12.1, Some(&ev("right", 12.1))).unwrap().reinforced);
        assert!(sch.step(12.2, Some(&ev("right", 12.2))).unwrap().reinforced);
    }

    #[test]
    fn reset_clears_held_reinforcers() {
        let comps = make_ab(
            Box::new(FR::new(100).unwrap()),
            Box::new(FT::new(5.0).unwrap()),
        );
        let mut sch = Concurrent::new(comps, 2.0, 0).unwrap();
        sch.step(0.0, Some(&ev("a", 0.0))).unwrap();
        assert!(!sch.step(5.5, Some(&ev("b", 5.5))).unwrap().reinforced); // queued
        sch.reset();
        // Fresh session: no queued reinforcer to collect on b.
        assert!(!sch.step(0.0, Some(&ev("b", 0.0))).unwrap().reinforced);
    }
}

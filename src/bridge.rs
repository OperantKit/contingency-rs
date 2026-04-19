//! AST -> Schedule translator.
//!
//! Mirrors the Python `contingency.bridge.from_dsl` API. Translates a
//! parsed contingency-dsl AST node into an executable `Box<dyn Schedule>`.
//!
//! # References
//!
//! Catania, A. C. (1966). Concurrent performances: Reinforcement
//! interaction and response independence. *Journal of the Experimental
//! Analysis of Behavior*, 9(3), 253-263.
//! <https://doi.org/10.1901/jeab.1966.9-253>
//!
//! Ferster, C. B., & Skinner, B. F. (1957). *Schedules of reinforcement*.
//! Appleton-Century-Crofts.
//!
//! Fleshler, M., & Hoffman, H. S. (1962). A progression for generating
//! variable-interval schedules. *Journal of the Experimental Analysis of
//! Behavior*, 5(4), 529-530. <https://doi.org/10.1901/jeab.1962.5-529>

use indexmap::IndexMap;

use contingency_dsl::ast::{
    self, Atomic, AversiveKind, AversiveParam, CombinatorParamValue, Compound, CompoundParam,
    DirRef, DirectionalCOD, Modifier, ModifierKind, NumericPayload, Program, ProgramSchedule,
    ScheduleExpr, SecondOrderSlot, Special, SpecialKind, TrialParam,
};
use contingency_dsl::enums::{Combinator, Distribution, Domain};
use std::collections::BTreeMap;

use crate::errors::ContingencyError;
use crate::schedule::Schedule;
use crate::schedules::{
    self, AdjustingSchedule, AdjustingTarget, Alternative, Chained, Concurrent, Conjunctive,
    DiscriminatedAvoidance, Escape, GoNoGo, InterlockingSchedule, Interpolate, LimitedHold,
    MatchingToSample, Mixed, Multiple, Overlay, Percentile, PercentileDirection, PercentileTarget,
    ProgressiveRatio, ResponseCost, SecondOrder, Sidman, Tandem, Timeout, DRH, DRL, DRO, EXT, FI,
    FR, FT, RI, RR, RT, VI, VR, VT,
};
use crate::Result;

// --- Time unit resolution --------------------------------------------------

fn time_unit_factor(unit: Option<&str>) -> Result<f64> {
    match unit {
        None => Ok(1.0),
        Some("s") => Ok(1.0),
        Some("ms") => Ok(0.001),
        Some("min") => Ok(60.0),
        Some(other) => Err(ContingencyError::Config(format!(
            "unsupported time_unit {other:?}; expected one of \"s\", \"ms\", \"min\", or None"
        ))),
    }
}

fn scaled_time(value: f64, unit: Option<&str>, time_unit_seconds: f64) -> Result<f64> {
    Ok(value * time_unit_factor(unit)? * time_unit_seconds)
}

fn scaled_payload(p: &NumericPayload, time_unit_seconds: f64) -> Result<f64> {
    scaled_time(p.value, p.time_unit.as_deref(), time_unit_seconds)
}

// --- Public API ------------------------------------------------------------

/// Parse a DSL source string and translate it to an executable schedule.
pub fn from_dsl_str(source: &str, time_unit_seconds: f64) -> Result<Box<dyn Schedule>> {
    let program = contingency_dsl::parse(source)
        .map_err(|e| ContingencyError::Config(format!("DSL parse error: {e}")))?;
    from_dsl_program(&program, time_unit_seconds)
}

/// Translate a `Program` to an executable schedule via its top-level schedule.
pub fn from_dsl_program(program: &Program, time_unit_seconds: f64) -> Result<Box<dyn Schedule>> {
    match &program.schedule {
        ProgramSchedule::Operant(expr) => from_dsl_expr(expr, time_unit_seconds),
        ProgramSchedule::Respondent(_) => Err(ContingencyError::Config(
            "respondent top-level schedule is not yet supported by the bridge".into(),
        )),
        ProgramSchedule::None => Err(ContingencyError::Config(
            "Program has no top-level schedule".into(),
        )),
    }
}

/// Translate a `ScheduleExpr` AST node to an executable schedule.
pub fn from_dsl_expr(node: &ScheduleExpr, time_unit_seconds: f64) -> Result<Box<dyn Schedule>> {
    match node {
        ScheduleExpr::Atomic(a) => from_atomic(a, time_unit_seconds),
        ScheduleExpr::Special(s) => from_special(s),
        ScheduleExpr::Compound(c) => from_compound(c, time_unit_seconds),
        ScheduleExpr::Modifier(m) => from_modifier(m, time_unit_seconds),
        ScheduleExpr::LimitedHold(lh) => from_limited_hold(lh, time_unit_seconds),
        ScheduleExpr::SecondOrder(so) => from_second_order(so, time_unit_seconds),
        ScheduleExpr::TimeoutWrapped(tw) => from_timeout(tw, time_unit_seconds),
        ScheduleExpr::ResponseCostWrapped(rw) => from_response_cost(rw, time_unit_seconds),
        ScheduleExpr::AdjustingSchedule(a) => from_adjusting(a, time_unit_seconds),
        ScheduleExpr::InterlockingSchedule(i) => from_interlocking(i, time_unit_seconds),
        ScheduleExpr::Annotated(an) => from_dsl_expr(&an.expr, time_unit_seconds),
        ScheduleExpr::IdentifierRef(r) => Err(ContingencyError::Config(format!(
            "unresolved IdentifierRef({:?}); call contingency_dsl analyze/expand on the Program first",
            r.name
        ))),
        ScheduleExpr::TrialBased(t) => match t.trial_type.as_str() {
            "MTS" => build_mts(&t.params, time_unit_seconds),
            "GoNoGo" => build_go_nogo(&t.params, time_unit_seconds),
            other => Err(ContingencyError::Config(format!(
                "unknown TrialBased trial_type: {other:?}"
            ))),
        },
        ScheduleExpr::AversiveSchedule(a) => match a.kind {
            AversiveKind::Sidman => build_sidman(&a.params, time_unit_seconds),
            AversiveKind::DiscrimAv => build_discrim_av(&a.params, time_unit_seconds),
            AversiveKind::Escape => build_escape(&a.params, time_unit_seconds),
        },
        ScheduleExpr::Respondent(_) | ScheduleExpr::RespondentExtension(_) => Err(
            ContingencyError::Config("Respondent AST node is not yet supported by the bridge".into()),
        ),
    }
}

// --- Per-node translators --------------------------------------------------

fn from_atomic(node: &Atomic, time_unit_seconds: f64) -> Result<Box<dyn Schedule>> {
    let Atomic {
        schedule_type,
        value,
        time_unit,
    } = node;
    let unit = time_unit.as_deref();
    let v = *value;
    match (schedule_type.distribution, schedule_type.domain) {
        (Distribution::Fixed, Domain::Ratio) => {
            if v.fract() != 0.0 || v < 1.0 {
                return Err(ContingencyError::Config(format!(
                    "FR requires a positive integer ratio, got {v}"
                )));
            }
            Ok(Box::new(FR::new(v as u64)?))
        }
        (Distribution::Fixed, Domain::Interval) => {
            Ok(Box::new(FI::new(scaled_time(v, unit, time_unit_seconds)?)?))
        }
        (Distribution::Fixed, Domain::Time) => {
            Ok(Box::new(FT::new(scaled_time(v, unit, time_unit_seconds)?)?))
        }
        (Distribution::Variable, Domain::Ratio) => Ok(Box::new(VR::new(v, 12, None)?)),
        (Distribution::Variable, Domain::Interval) => Ok(Box::new(VI::new(
            scaled_time(v, unit, time_unit_seconds)?,
            12,
            None,
        )?)),
        (Distribution::Variable, Domain::Time) => Ok(Box::new(VT::new(
            scaled_time(v, unit, time_unit_seconds)?,
            12,
            None,
        )?)),
        (Distribution::Random, Domain::Ratio) => {
            if !v.is_finite() || v <= 0.0 {
                return Err(ContingencyError::Config(format!(
                    "RR requires a positive mean-value to convert to probability, got {v}"
                )));
            }
            Ok(Box::new(RR::new(1.0 / v, None)?))
        }
        (Distribution::Random, Domain::Interval) => Ok(Box::new(RI::new(
            scaled_time(v, unit, time_unit_seconds)?,
            None,
        )?)),
        (Distribution::Random, Domain::Time) => Ok(Box::new(RT::new(
            scaled_time(v, unit, time_unit_seconds)?,
            None,
        )?)),
    }
}

fn from_special(node: &Special) -> Result<Box<dyn Schedule>> {
    match node.kind {
        SpecialKind::Ext => Ok(Box::new(EXT::new())),
        SpecialKind::Crf => Ok(Box::new(schedules::crf())),
    }
}

fn from_compound(node: &Compound, time_unit_seconds: f64) -> Result<Box<dyn Schedule>> {
    let components: Vec<Box<dyn Schedule>> = node
        .components
        .iter()
        .map(|c| from_dsl_expr(c, time_unit_seconds))
        .collect::<Result<Vec<_>>>()?;
    let empty = std::collections::BTreeMap::new();
    let params = node.params.as_ref().unwrap_or(&empty);

    match node.combinator {
        Combinator::Conc => build_concurrent(components, params, time_unit_seconds),
        Combinator::Alt => {
            if components.len() < 2 {
                return Err(ContingencyError::Config(format!(
                    "Alt requires >= 2 components, got {}",
                    components.len()
                )));
            }
            let mut iter = components.into_iter();
            let a = iter.next().unwrap();
            let b = iter.next().unwrap();
            let mut acc: Box<dyn Schedule> = Box::new(Alternative::new(a, b));
            for extra in iter {
                acc = Box::new(Alternative::new(acc, extra));
            }
            Ok(acc)
        }
        Combinator::Conj => {
            if components.len() < 2 {
                return Err(ContingencyError::Config(format!(
                    "Conj requires >= 2 components, got {}",
                    components.len()
                )));
            }
            let mut iter = components.into_iter();
            let a = iter.next().unwrap();
            let b = iter.next().unwrap();
            let mut acc: Box<dyn Schedule> = Box::new(Conjunctive::new(a, b)?);
            for extra in iter {
                acc = Box::new(Conjunctive::new(acc, extra)?);
            }
            Ok(acc)
        }
        Combinator::Mult => {
            let stimuli = default_stimuli(components.len());
            Ok(Box::new(Multiple::new(components, Some(stimuli))?))
        }
        Combinator::Chain => {
            let stimuli = default_stimuli(components.len());
            Ok(Box::new(Chained::new(components, Some(stimuli))?))
        }
        Combinator::Tand => Ok(Box::new(Tandem::new(components)?)),
        Combinator::Mix => Ok(Box::new(Mixed::new(components)?)),
        Combinator::Overlay => {
            if components.len() != 2 {
                return Err(ContingencyError::Config(format!(
                    "Overlay requires exactly 2 components (base, punisher), got {}",
                    components.len()
                )));
            }
            let mut iter = components.into_iter();
            let base = iter.next().unwrap();
            let punisher = iter.next().unwrap();
            Ok(Box::new(Overlay::new(base, punisher)?))
        }
        Combinator::Interpolate | Combinator::Interp => {
            build_interpolate(components, params, time_unit_seconds)
        }
    }
}

fn default_stimuli(n: usize) -> Vec<String> {
    (0..n).map(|i| format!("s{i}")).collect()
}

fn build_interpolate(
    components: Vec<Box<dyn Schedule>>,
    params: &std::collections::BTreeMap<String, CombinatorParamValue>,
    time_unit_seconds: f64,
) -> Result<Box<dyn Schedule>> {
    if components.len() != 2 {
        return Err(ContingencyError::Config(format!(
            "Interpolate requires exactly 2 components, got {}",
            components.len()
        )));
    }
    let mut iter = components.into_iter();
    let base = iter.next().unwrap();
    let probe = iter.next().unwrap();

    let onset_seconds: Option<f64> = match params.get("onset") {
        None => None,
        Some(CombinatorParamValue::Scalar(p)) => {
            Some(scaled_time(p.value, p.time_unit.as_deref(), time_unit_seconds)?)
        }
        Some(other) => {
            return Err(ContingencyError::Config(format!(
                "unsupported onset parameter value: {other:?}"
            )))
        }
    };
    let mut interval_seconds = onset_seconds.unwrap_or(1.0);
    if interval_seconds <= 0.0 {
        interval_seconds = 1.0;
    }
    let probe_duration = interval_seconds / 2.0;
    Ok(Box::new(Interpolate::new(
        base,
        probe,
        interval_seconds,
        probe_duration,
        onset_seconds,
    )?))
}

fn build_concurrent(
    components: Vec<Box<dyn Schedule>>,
    params: &std::collections::BTreeMap<String, CombinatorParamValue>,
    time_unit_seconds: f64,
) -> Result<Box<dyn Schedule>> {
    let mut comp_map: IndexMap<String, Box<dyn Schedule>> = IndexMap::new();
    for (i, c) in components.into_iter().enumerate() {
        comp_map.insert(format!("c{i}"), c);
    }

    let mut cod = 0.0f64;
    let mut cor: u32 = 0;
    let mut cod_directional: Option<IndexMap<(String, String), f64>> = None;

    let resolve_op = |r: &DirRef| -> String {
        match r {
            DirRef::Index(i) => format!("c{}", i - 1),
            DirRef::Name(n) => n.clone(),
        }
    };

    if let Some(val) = params.get("COD") {
        match val {
            CombinatorParamValue::Scalar(CompoundParam { value, time_unit }) => {
                cod = scaled_time(*value, time_unit.as_deref(), time_unit_seconds)?;
            }
            CombinatorParamValue::Directional(DirectionalCOD { base, directional }) => {
                if let Some(b) = base {
                    cod = scaled_time(b.value, b.time_unit.as_deref(), time_unit_seconds)?;
                }
                let mut map: IndexMap<(String, String), f64> = IndexMap::new();
                for entry in directional {
                    let from_op = resolve_op(&entry.from_ref);
                    let to_op = resolve_op(&entry.to_ref);
                    let v = scaled_time(
                        entry.value,
                        entry.time_unit.as_deref(),
                        time_unit_seconds,
                    )?;
                    map.insert((from_op, to_op), v);
                }
                if !map.is_empty() {
                    cod_directional = Some(map);
                }
            }
            other => {
                return Err(ContingencyError::Config(format!(
                    "unsupported COD parameter value: {other:?}"
                )))
            }
        }
    }
    if let Some(val) = params.get("FRCO") {
        match val {
            CombinatorParamValue::Scalar(CompoundParam { value, .. }) => {
                cor = *value as u32;
            }
            other => {
                return Err(ContingencyError::Config(format!(
                    "unsupported FRCO parameter value: {other:?}"
                )))
            }
        }
    }
    if params.contains_key("PUNISH") {
        return Err(ContingencyError::Config(
            "PUNISH parameter is not yet supported by the Rust bridge".into(),
        ));
    }

    Ok(Box::new(Concurrent::with_extensions(
        comp_map,
        cod,
        cor,
        cod_directional,
        None,
    )?))
}

fn from_modifier(node: &Modifier, time_unit_seconds: f64) -> Result<Box<dyn Schedule>> {
    match node.kind {
        ModifierKind::Drl => {
            let v = node
                .value
                .ok_or_else(|| ContingencyError::Config("DRL requires a numeric value".into()))?;
            let t = scaled_time(v, node.time_unit.as_deref(), time_unit_seconds)?;
            Ok(Box::new(DRL::new(t)?))
        }
        ModifierKind::Drh => {
            let v = node
                .value
                .ok_or_else(|| ContingencyError::Config("DRH requires a numeric value".into()))?;
            let window = scaled_time(v, node.time_unit.as_deref(), time_unit_seconds)?;
            Ok(Box::new(DRH::new(2, window)?))
        }
        ModifierKind::Dro => {
            let v = node
                .value
                .ok_or_else(|| ContingencyError::Config("DRO requires a numeric value".into()))?;
            let t = scaled_time(v, node.time_unit.as_deref(), time_unit_seconds)?;
            Ok(Box::new(DRO::resetting(t)?))
        }
        ModifierKind::Pr => from_pr(node),
        ModifierKind::Repeat => {
            let v = node.value.ok_or_else(|| {
                ContingencyError::Config(
                    "Repeat requires an integer value and an inner schedule".into(),
                )
            })?;
            let inner = node.inner.as_ref().ok_or_else(|| {
                ContingencyError::Config(
                    "Repeat requires an integer value and an inner schedule".into(),
                )
            })?;
            let count = v as i64;
            if count < 2 {
                return Err(ContingencyError::Config(format!(
                    "Repeat requires a count >= 2, got {count}"
                )));
            }
            let components: Vec<Box<dyn Schedule>> = (0..count)
                .map(|_| from_dsl_expr(inner, time_unit_seconds))
                .collect::<Result<Vec<_>>>()?;
            let stimuli = default_stimuli(count as usize);
            Ok(Box::new(Chained::new(components, Some(stimuli))?))
        }
        ModifierKind::Pctl => from_pctl(node),
        ModifierKind::Lag => Err(ContingencyError::Config(
            "Lag modifier is not yet supported by the bridge".into(),
        )),
    }
}

fn from_pr(node: &Modifier) -> Result<Box<dyn Schedule>> {
    let start = node.pr_start.unwrap_or(1.0) as u32;
    let step_name = node.pr_step.as_deref();
    match step_name {
        None | Some("linear") | Some("hodos") => {
            let increment = node.pr_increment.unwrap_or(1.0) as u32;
            let step_fn = schedules::arithmetic(start, increment)?;
            Ok(Box::new(ProgressiveRatio::new(step_fn)))
        }
        Some("geometric") | Some("exponential") => {
            let ratio = node.pr_ratio.unwrap_or(2.0);
            let step_fn = schedules::geometric(start, ratio)?;
            Ok(Box::new(ProgressiveRatio::new(step_fn)))
        }
        Some(other) => Err(ContingencyError::Config(format!(
            "unknown PR pr_step: {other:?}"
        ))),
    }
}

fn from_pctl(node: &Modifier) -> Result<Box<dyn Schedule>> {
    let target_str = node
        .pctl_target
        .as_deref()
        .ok_or_else(|| ContingencyError::Config("Pctl requires a target dimension".into()))?;
    let target = match target_str {
        "irt" | "IRT" => PercentileTarget::Irt,
        other => {
            return Err(ContingencyError::Config(format!(
                "Pctl target {other:?} is not yet supported by the bridge"
            )))
        }
    };
    let rank = node
        .pctl_rank
        .ok_or_else(|| ContingencyError::Config("Pctl requires a rank".into()))?;
    if !(1..=100).contains(&rank) {
        return Err(ContingencyError::Config(format!(
            "Pctl rank must be in [1, 100], got {rank}"
        )));
    }
    let window = node.pctl_window.unwrap_or(20) as usize;
    let dir = node.pctl_dir.as_deref().unwrap_or("below");
    let direction = match dir {
        "below" => PercentileDirection::Below,
        "above" => PercentileDirection::Above,
        other => {
            return Err(ContingencyError::Config(format!(
                "Pctl direction {other:?} must be \"above\" or \"below\""
            )))
        }
    };
    Ok(Box::new(Percentile::new(
        target, rank as u8, window, direction,
    )?))
}

fn from_limited_hold(
    node: &ast::LimitedHold,
    time_unit_seconds: f64,
) -> Result<Box<dyn Schedule>> {
    let hold = scaled_time(
        node.hold_duration,
        node.time_unit.as_deref(),
        time_unit_seconds,
    )?;
    // LimitedHold requires an armable inner. Only the interval-family
    // atomics (FI/VI/RI) are armable, so we dispatch on the AST shape
    // and construct concrete wrappers.
    match node.inner.as_ref() {
        ScheduleExpr::Atomic(a) => {
            let unit = a.time_unit.as_deref();
            let v = a.value;
            match (a.schedule_type.distribution, a.schedule_type.domain) {
                (Distribution::Fixed, Domain::Interval) => {
                    let inner = FI::new(scaled_time(v, unit, time_unit_seconds)?)?;
                    Ok(Box::new(LimitedHold::new(inner, hold)?))
                }
                (Distribution::Variable, Domain::Interval) => {
                    let inner = VI::new(scaled_time(v, unit, time_unit_seconds)?, 12, None)?;
                    Ok(Box::new(LimitedHold::new(inner, hold)?))
                }
                (Distribution::Random, Domain::Interval) => {
                    let inner = RI::new(scaled_time(v, unit, time_unit_seconds)?, None)?;
                    Ok(Box::new(LimitedHold::new(inner, hold)?))
                }
                _ => Err(ContingencyError::Config(
                    "LimitedHold requires an interval-family schedule (FI/VI/RI) as inner".into(),
                )),
            }
        }
        _ => Err(ContingencyError::Config(
            "LimitedHold requires an atomic interval-family schedule (FI/VI/RI) as inner".into(),
        )),
    }
}

fn from_second_order(
    node: &ast::SecondOrder,
    time_unit_seconds: f64,
) -> Result<Box<dyn Schedule>> {
    let overall = from_second_order_slot(&node.overall, time_unit_seconds)?;
    let unit = from_second_order_slot(&node.unit, time_unit_seconds)?;
    Ok(Box::new(SecondOrder::new(overall, unit)))
}

fn from_second_order_slot(
    slot: &SecondOrderSlot,
    time_unit_seconds: f64,
) -> Result<Box<dyn Schedule>> {
    match slot {
        SecondOrderSlot::Atomic(a) => from_atomic(a, time_unit_seconds),
        SecondOrderSlot::Ref(name) => Err(ContingencyError::Config(format!(
            "unresolved SecondOrder ref {name:?}; call contingency_dsl analyze/expand first"
        ))),
    }
}

fn from_timeout(node: &ast::TimeoutWrapped, time_unit_seconds: f64) -> Result<Box<dyn Schedule>> {
    let inner = from_dsl_expr(&node.inner, time_unit_seconds)?;
    let duration = scaled_time(
        node.timeout.duration,
        node.timeout.duration_unit.as_deref(),
        time_unit_seconds,
    )?;
    Ok(Box::new(Timeout::new(
        inner,
        duration,
        node.timeout.reset_on_response,
    )?))
}

fn from_response_cost(
    node: &ast::ResponseCostWrapped,
    time_unit_seconds: f64,
) -> Result<Box<dyn Schedule>> {
    let inner = from_dsl_expr(&node.inner, time_unit_seconds)?;
    Ok(Box::new(ResponseCost::new(
        inner,
        node.response_cost.amount,
        node.response_cost.unit.clone(),
        None,
    )?))
}

fn from_adjusting(
    node: &ast::AdjustingSchedule,
    time_unit_seconds: f64,
) -> Result<Box<dyn Schedule>> {
    let target = match node.adj_target.as_str() {
        "ratio" => AdjustingTarget::Ratio,
        "interval" => AdjustingTarget::Interval,
        "delay" => AdjustingTarget::Delay,
        "amount" => AdjustingTarget::Amount,
        other => {
            return Err(ContingencyError::Config(format!(
                "AdjustingSchedule target {other:?} must be ratio/interval/delay/amount"
            )))
        }
    };
    let temporal = matches!(target, AdjustingTarget::Interval | AdjustingTarget::Delay);
    let resolve = |p: &NumericPayload| -> Result<f64> {
        if temporal {
            scaled_payload(p, time_unit_seconds)
        } else {
            if p.time_unit.is_some() {
                return Err(ContingencyError::Config(format!(
                    "AdjustingSchedule dimensionless payload carries time_unit={:?}; expected None",
                    p.time_unit
                )));
            }
            Ok(p.value)
        }
    };
    let start = resolve(&node.adj_start)?;
    let step = resolve(&node.adj_step)?;
    let minimum = node.adj_min.as_ref().map(&resolve).transpose()?;
    let maximum = node.adj_max.as_ref().map(&resolve).transpose()?;
    Ok(Box::new(AdjustingSchedule::new(
        target, start, step, minimum, maximum,
    )?))
}

fn from_interlocking(
    node: &ast::InterlockingSchedule,
    time_unit_seconds: f64,
) -> Result<Box<dyn Schedule>> {
    if node.interlock_r0 < 1 {
        return Err(ContingencyError::Config(format!(
            "InterlockingSchedule requires interlock_r0 >= 1, got {}",
            node.interlock_r0
        )));
    }
    let decay = scaled_payload(&node.interlock_t, time_unit_seconds)?;
    Ok(Box::new(InterlockingSchedule::new(
        node.interlock_r0 as u64,
        decay,
    )?))
}

// --- Aversive / TrialBased helpers ----------------------------------------

fn aversive_numeric(
    params: &BTreeMap<String, AversiveParam>,
    key: &str,
) -> Result<NumericPayload> {
    match params.get(key) {
        Some(AversiveParam::Numeric(n)) => Ok(n.clone()),
        Some(_) => Err(ContingencyError::Config(format!(
            "AversiveSchedule parameter {key:?} must be a numeric payload"
        ))),
        None => Err(ContingencyError::Config(format!(
            "AversiveSchedule requires parameter {key:?}"
        ))),
    }
}

fn aversive_time_param(
    params: &BTreeMap<String, AversiveParam>,
    key: &str,
    time_unit_seconds: f64,
) -> Result<f64> {
    let n = aversive_numeric(params, key)?;
    scaled_payload(&n, time_unit_seconds)
}

fn aversive_magnitude(params: &BTreeMap<String, AversiveParam>) -> Result<f64> {
    match params.get("magnitude") {
        None => Ok(1.0),
        Some(AversiveParam::Numeric(n)) => {
            if n.time_unit.is_some() {
                return Err(ContingencyError::Config(
                    "AversiveSchedule 'magnitude' must not carry a time_unit".into(),
                ));
            }
            Ok(n.value)
        }
        Some(_) => Err(ContingencyError::Config(
            "AversiveSchedule 'magnitude' has unsupported form".into(),
        )),
    }
}

fn build_sidman(
    params: &BTreeMap<String, AversiveParam>,
    time_unit_seconds: f64,
) -> Result<Box<dyn Schedule>> {
    let ssi = aversive_time_param(params, "SSI", time_unit_seconds)?;
    let rsi = aversive_time_param(params, "RSI", time_unit_seconds)?;
    let mag = aversive_magnitude(params)?;
    Ok(Box::new(Sidman::new(ssi, rsi, mag)?))
}

fn build_discrim_av(
    params: &BTreeMap<String, AversiveParam>,
    time_unit_seconds: f64,
) -> Result<Box<dyn Schedule>> {
    let warning = aversive_time_param(params, "CSUSInterval", time_unit_seconds)?;
    let iti = aversive_time_param(params, "ITI", time_unit_seconds)?;
    let mag = aversive_magnitude(params)?;
    Ok(Box::new(DiscriminatedAvoidance::new(warning, iti, mag)?))
}

fn build_escape(
    params: &BTreeMap<String, AversiveParam>,
    time_unit_seconds: f64,
) -> Result<Box<dyn Schedule>> {
    let iti = aversive_time_param(params, "SafeDuration", time_unit_seconds)?;
    let trial_duration = match params.get("MaxShock") {
        Some(AversiveParam::Numeric(n)) => scaled_payload(n, time_unit_seconds)?,
        Some(_) => {
            return Err(ContingencyError::Config(
                "AversiveSchedule 'MaxShock' must be a numeric payload".into(),
            ))
        }
        None => 1.0e9,
    };
    let mag = aversive_magnitude(params)?;
    Ok(Box::new(Escape::new(trial_duration, iti, mag)?))
}

// --- TrialBased helpers ---------------------------------------------------

fn trial_param_f64(param: &TrialParam) -> Result<f64> {
    match param {
        TrialParam::Float(v) => Ok(*v),
        TrialParam::Int(v) => Ok(*v as f64),
        TrialParam::Numeric(n) => Ok(n.value),
        other => Err(ContingencyError::Config(format!(
            "TrialBased numeric parameter has unsupported form: {other:?}"
        ))),
    }
}

fn trial_param_unit(param: &TrialParam) -> Result<Option<&str>> {
    match param {
        TrialParam::Str(s) => Ok(Some(s.as_str())),
        other => Err(ContingencyError::Config(format!(
            "TrialBased unit parameter must be a string, got {other:?}"
        ))),
    }
}

fn trial_based_time(
    params: &BTreeMap<String, TrialParam>,
    value_key: &str,
    unit_key: &str,
    time_unit_seconds: f64,
    default: Option<f64>,
) -> Result<Option<f64>> {
    let Some(val_param) = params.get(value_key) else {
        return Ok(default);
    };
    let v = trial_param_f64(val_param)?;
    let unit = match params.get(unit_key) {
        Some(u) => trial_param_unit(u)?,
        None => None,
    };
    Ok(Some(scaled_time(v, unit, time_unit_seconds)?))
}

fn bridge_nested(
    param: Option<&TrialParam>,
    time_unit_seconds: f64,
) -> Result<Option<Box<dyn Schedule>>> {
    match param {
        None => Ok(None),
        Some(TrialParam::Schedule(expr)) => {
            let s = from_dsl_expr(expr, time_unit_seconds)?;
            Ok(Some(s))
        }
        Some(other) => Err(ContingencyError::Config(format!(
            "TrialBased nested schedule parameter has unsupported form: {other:?}"
        ))),
    }
}

fn build_mts(
    params: &BTreeMap<String, TrialParam>,
    time_unit_seconds: f64,
) -> Result<Box<dyn Schedule>> {
    let comparisons = match params.get("comparisons") {
        Some(TrialParam::Int(n)) => *n,
        Some(TrialParam::Float(v)) if v.fract() == 0.0 && *v >= 0.0 => *v as i64,
        Some(other) => {
            return Err(ContingencyError::Config(format!(
                "MTS 'comparisons' must be an integer, got {other:?}"
            )))
        }
        None => {
            return Err(ContingencyError::Config(
                "MTS requires a 'comparisons' parameter".into(),
            ))
        }
    };
    if comparisons < 0 || comparisons > u32::MAX as i64 {
        return Err(ContingencyError::Config(format!(
            "MTS 'comparisons' out of range: {comparisons}"
        )));
    }
    let iti =
        trial_based_time(params, "ITI", "ITI_unit", time_unit_seconds, Some(0.0))?
            .unwrap_or(0.0);
    let sample_duration =
        trial_based_time(params, "delay", "delay_unit", time_unit_seconds, Some(0.0))?
            .unwrap_or(0.0);
    let choice_timeout = trial_based_time(
        params,
        "limitedHold",
        "limitedHoldUnit",
        time_unit_seconds,
        None,
    )?
    .unwrap_or(5.0);
    let consequence = bridge_nested(params.get("consequence"), time_unit_seconds)?;
    let incorrect = bridge_nested(params.get("incorrect"), time_unit_seconds)?;
    Ok(Box::new(MatchingToSample::new(
        comparisons as u32,
        sample_duration,
        choice_timeout,
        consequence,
        incorrect,
        iti,
        None,
    )?))
}

fn build_go_nogo(
    params: &BTreeMap<String, TrialParam>,
    time_unit_seconds: f64,
) -> Result<Box<dyn Schedule>> {
    let response_window = trial_based_time(
        params,
        "responseWindow",
        "responseWindowUnit",
        time_unit_seconds,
        None,
    )?
    .ok_or_else(|| {
        ContingencyError::Config("GoNoGo requires a 'responseWindow' parameter".into())
    })?;
    let iti =
        trial_based_time(params, "ITI", "ITI_unit", time_unit_seconds, Some(0.0))?
            .unwrap_or(0.0);
    let go_probability = match params.get("go_probability") {
        Some(p) => trial_param_f64(p)?,
        None => 0.5,
    };
    let consequence = bridge_nested(params.get("consequence"), time_unit_seconds)?;
    let correct_nogo = bridge_nested(params.get("incorrect"), time_unit_seconds)?;
    let false_alarm = bridge_nested(params.get("falseAlarm"), time_unit_seconds)?;
    Ok(Box::new(GoNoGo::new(
        go_probability,
        response_window,
        iti,
        consequence,
        correct_nogo,
        false_alarm,
        None,
    )?))
}

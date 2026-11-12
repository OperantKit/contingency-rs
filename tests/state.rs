//! Tests for `Schedule::state` — the observable position of compound
//! schedules after the last step.

use serde_json::{json, Value};

use contingency::spec::from_spec;
use contingency::types::MetaValue;
use contingency::{ResponseEvent, Schedule};

fn build(spec: Value) -> Box<dyn Schedule> {
    from_spec(&spec).unwrap_or_else(|e| panic!("from_spec({spec}) failed: {e}"))
}

fn respond(schedule: &mut dyn Schedule, now: f64) {
    let ev = ResponseEvent::on("main", now);
    schedule.step(now, Some(&ev)).unwrap();
}

fn get(schedule: &dyn Schedule, key: &str) -> Option<MetaValue> {
    schedule.state().get(key).cloned()
}

fn fr(n: u64) -> Value {
    json!({"type": "FR", "params": {"n": n}})
}

#[test]
fn atomic_schedules_report_no_state() {
    let s = build(fr(3));
    assert!(s.state().is_empty());
}

#[test]
fn sequences_report_the_active_component() {
    for ty in ["Multiple", "Chained", "Tandem", "Mixed"] {
        let mut s = build(json!({"type": ty, "params": {"components": [fr(1), fr(2)]}}));
        assert_eq!(
            get(s.as_ref(), "active_index"),
            Some(MetaValue::Int(0)),
            "{ty}"
        );
        respond(s.as_mut(), 1.0);
        assert_eq!(
            get(s.as_ref(), "active_index"),
            Some(MetaValue::Int(1)),
            "{ty}"
        );
        respond(s.as_mut(), 2.0);
        respond(s.as_mut(), 3.0);
        assert_eq!(
            get(s.as_ref(), "active_index"),
            Some(MetaValue::Int(0)),
            "{ty}"
        );
        respond(s.as_mut(), 4.0);
        s.reset();
        assert_eq!(
            get(s.as_ref(), "active_index"),
            Some(MetaValue::Int(0)),
            "{ty}"
        );
    }
}

#[test]
fn second_order_reports_unit_completions() {
    let mut s = build(json!({"type": "SecondOrder", "params": {"overall": fr(3), "unit": fr(2)}}));
    assert_eq!(get(s.as_ref(), "unit_completions"), Some(MetaValue::Int(0)));
    for i in 1..=4 {
        respond(s.as_mut(), f64::from(i));
    }
    assert_eq!(get(s.as_ref(), "unit_completions"), Some(MetaValue::Int(2)));
    s.reset();
    assert_eq!(get(s.as_ref(), "unit_completions"), Some(MetaValue::Int(0)));
}

#[test]
fn timeout_reports_whether_a_timeout_runs() {
    let mut s = build(json!({"type": "Timeout", "params": {"inner": fr(1), "duration": 5.0}}));
    assert_eq!(get(s.as_ref(), "in_timeout"), Some(MetaValue::Bool(false)));
    respond(s.as_mut(), 1.0);
    assert_eq!(get(s.as_ref(), "in_timeout"), Some(MetaValue::Bool(true)));
    s.reset();
    assert_eq!(get(s.as_ref(), "in_timeout"), Some(MetaValue::Bool(false)));
}

#[test]
fn response_cost_reports_a_limited_balance_only() {
    let mut s = build(json!({"type": "ResponseCost", "params": {
        "inner": fr(5), "amount": 1.0, "initial_balance": 3.0
    }}));
    assert_eq!(get(s.as_ref(), "balance"), Some(MetaValue::Float(3.0)));
    respond(s.as_mut(), 1.0);
    assert_eq!(get(s.as_ref(), "balance"), Some(MetaValue::Float(2.0)));
    s.reset();
    assert_eq!(get(s.as_ref(), "balance"), Some(MetaValue::Float(3.0)));

    let unlimited =
        build(json!({"type": "ResponseCost", "params": {"inner": fr(5), "amount": 1.0}}));
    assert_eq!(get(unlimited.as_ref(), "balance"), None);
}

#[test]
fn a_boxed_schedule_forwards_its_state() {
    let mut s = build(json!({"type": "Tandem", "params": {"components": [fr(1), fr(2)]}}));
    respond(s.as_mut(), 1.0);
    // `Schedule for Box<T>` is the path the language bindings call.
    let state = <Box<dyn Schedule> as Schedule>::state(&s);
    assert_eq!(state.get("active_index"), Some(&MetaValue::Int(1)));
}

#[test]
fn stateful_atomic_schedules_report_their_position() {
    let mut s = build(json!({"type": "Sidman", "params": {
        "shock_shock_interval": 5.0, "response_shock_interval": 20.0}}));
    assert_eq!(
        get(s.as_ref(), "next_shock_time"),
        Some(MetaValue::Float(5.0))
    );
    respond(s.as_mut(), 1.0);
    assert_eq!(
        get(s.as_ref(), "next_shock_time"),
        Some(MetaValue::Float(21.0))
    );

    let mut s = build(json!({"type": "DiscriminatedAvoidance", "params": {
        "warning_duration": 2.0, "iti": 3.0}}));
    assert_eq!(get(s.as_ref(), "phase"), Some(MetaValue::Str("iti".into())));
    s.step(3.0, None).unwrap();
    assert_eq!(
        get(s.as_ref(), "phase"),
        Some(MetaValue::Str("warning".into()))
    );

    let s = build(json!({"type": "Escape", "params": {"trial_duration": 2.0, "iti": 3.0}}));
    assert_eq!(
        get(s.as_ref(), "phase"),
        Some(MetaValue::Str("shock".into()))
    );

    let mut s = build(json!({"type": "AdjustingSchedule", "params": {
        "target": "ratio", "start": 2.0, "step": 1.0}}));
    assert_eq!(
        get(s.as_ref(), "current_value"),
        Some(MetaValue::Float(2.0))
    );
    respond(s.as_mut(), 1.0);
    respond(s.as_mut(), 2.0);
    assert_eq!(
        get(s.as_ref(), "current_value"),
        Some(MetaValue::Float(3.0))
    );
    s.reset();
    assert_eq!(
        get(s.as_ref(), "current_value"),
        Some(MetaValue::Float(2.0))
    );

    let mut s = build(json!({"type": "InterlockingSchedule", "params": {
        "initial_ratio": 5, "decay_time": 100.0}}));
    assert_eq!(get(s.as_ref(), "response_count"), Some(MetaValue::Int(0)));
    assert_eq!(get(s.as_ref(), "anchor"), None);
    respond(s.as_mut(), 1.0);
    assert_eq!(get(s.as_ref(), "response_count"), Some(MetaValue::Int(1)));
    assert_eq!(get(s.as_ref(), "anchor"), Some(MetaValue::Float(1.0)));

    let mut s = build(json!({"type": "Percentile", "params": {
        "target": "irt", "rank": 50, "window": 3}}));
    assert_eq!(
        get(s.as_ref(), "samples"),
        Some(MetaValue::FloatList(vec![]))
    );
    respond(s.as_mut(), 1.0);
    respond(s.as_mut(), 3.0);
    assert_eq!(
        get(s.as_ref(), "samples"),
        Some(MetaValue::FloatList(vec![2.0]))
    );
}

#[test]
fn trial_based_schedules_report_their_position() {
    let s = build(json!({"type": "MatchingToSample", "params": {"n_comparisons": 3, "seed": 1}}));
    let st = s.state();
    assert_eq!(st.get("phase"), Some(&MetaValue::Str("choice".into())));
    assert_eq!(st.get("trial_index"), Some(&MetaValue::Int(0)));
    assert!(
        matches!(st.get("correct_operandum"), Some(MetaValue::Str(c)) if c.starts_with("choice_"))
    );
    assert_eq!(st.get("current_type_index"), None);

    let s = build(json!({"type": "GoNoGo", "params": {"seed": 1}}));
    let st = s.state();
    assert!(matches!(st.get("phase"), Some(MetaValue::Str(p)) if p == "go" || p == "nogo"));
    assert_eq!(st.get("trial_index"), Some(&MetaValue::Int(0)));
    assert!(matches!(st.get("current_is_go"), Some(MetaValue::Bool(_))));
}

#[test]
fn adjusting_ratio_rounds_the_requirement_half_to_even() {
    // 2.0, 2.5, 3.0, 3.5 -> requirements 2, 2, 3, 4 (Python `round`).
    let mut s = build(json!({"type": "AdjustingSchedule", "params": {
        "target": "ratio", "start": 2.0, "step": 0.5}}));
    let mut reinforced_at = Vec::new();
    for i in 1..=11 {
        let now = i as f64;
        let ev = ResponseEvent::on("main", now);
        if s.step(now, Some(&ev)).unwrap().reinforced {
            reinforced_at.push(i);
        }
    }
    assert_eq!(reinforced_at, vec![2, 4, 7, 11]);
}

#[test]
fn adjusting_ratio_start_of_one_half_rounds_to_zero_and_is_rejected() {
    // Python `round(0.5)` is 0, so contingency-py rejects this start.
    let spec = json!({"type": "AdjustingSchedule", "params": {
        "target": "ratio", "start": 0.5, "step": 1.0}});
    assert!(from_spec(&spec).is_err());
}

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

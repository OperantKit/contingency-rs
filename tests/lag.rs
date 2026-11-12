//! Behaviour of the `Lag` schedule built from its spec.
//!
//! A unit is `length` consecutive responses, read as the sequence of
//! their operanda. Units do not overlap. At the end of each unit the
//! schedule reinforces iff the unit differs from every one of the
//! previous `n` units (fewer while fewer have been emitted). Every
//! completed unit enters the comparison list, reinforced or not.

use serde_json::json;

use contingency::spec::from_spec;
use contingency::{ResponseEvent, Schedule};

fn run(spec: serde_json::Value, ops: &[&str]) -> Vec<bool> {
    let mut s = from_spec(&spec).unwrap();
    ops.iter()
        .enumerate()
        .map(|(i, op)| {
            let now = (i + 1) as f64;
            let ev = ResponseEvent::on(*op, now);
            s.step(now, Some(&ev)).unwrap().reinforced
        })
        .collect()
}

#[test]
fn lag1_single_responses() {
    let got = run(
        json!({"type": "Lag", "params": {"n": 1}}),
        &["L", "R", "R", "L"],
    );
    assert_eq!(got, vec![true, true, false, true]);
}

#[test]
fn lag2_length2_units() {
    // Units LL, LR, LL, RR, LL.
    let ops = ["L", "L", "L", "R", "L", "L", "R", "R", "L", "L"];
    let got = run(
        json!({"type": "Lag", "params": {"n": 2, "length": 2}}),
        &ops,
    );
    let ends: Vec<bool> = got.iter().skip(1).step_by(2).copied().collect();
    assert_eq!(ends, vec![true, true, false, true, false]);
    // Responses inside a unit are never reinforced.
    assert!(got.iter().step_by(2).all(|r| !r));
}

#[test]
fn unreinforced_units_stay_in_the_comparison_list() {
    // Lag 2: L (yes), L (no: = L), R (yes), L (no: L is among R, L).
    let got = run(
        json!({"type": "Lag", "params": {"n": 2}}),
        &["L", "L", "R", "L"],
    );
    assert_eq!(got, vec![true, false, true, false]);
}

#[test]
fn lag0_reinforces_every_unit() {
    let got = run(json!({"type": "Lag", "params": {"n": 0}}), &["L", "L", "L"]);
    assert_eq!(got, vec![true, true, true]);
    let got = run(
        json!({"type": "Lag", "params": {"n": 0, "length": 2}}),
        &["L", "L", "L", "L"],
    );
    assert_eq!(got, vec![false, true, false, true]);
}

#[test]
fn single_operandum_lag1_stops_after_first() {
    let got = run(json!({"type": "Lag", "params": {"n": 1}}), &["main"; 4]);
    assert_eq!(got, vec![true, false, false, false]);
}

#[test]
fn ticks_do_not_advance_and_reset_clears_history() {
    let mut s = from_spec(&json!({"type": "Lag", "params": {"n": 1}})).unwrap();
    assert!(!s.step(0.5, None).unwrap().reinforced);
    let ev = ResponseEvent::on("L", 1.0);
    assert!(s.step(1.0, Some(&ev)).unwrap().reinforced);
    s.reset();
    let ev = ResponseEvent::on("L", 0.2);
    assert!(s.step(0.2, Some(&ev)).unwrap().reinforced);
}

#[test]
fn rejects_non_monotonic_time() {
    let mut s = from_spec(&json!({"type": "Lag", "params": {"n": 1}})).unwrap();
    let ev = ResponseEvent::on("L", 2.0);
    s.step(2.0, Some(&ev)).unwrap();
    let ev = ResponseEvent::on("L", 1.0);
    assert!(s.step(1.0, Some(&ev)).is_err());
}

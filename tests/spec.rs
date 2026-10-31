//! Tests for `contingency::spec::from_spec`.
//!
//! One test per spec `type` builds that schedule from JSON and steps it
//! through a short trajectory. A second group covers parameters that the
//! conformance fixtures do not exercise (`LimitedHold` over VI/RI,
//! `Percentile` below, the geometric and Richardson-Roberts progressive
//! ratios, `AdjustingSchedule` on intervals with a minimum,
//! `shock_magnitude` other than 1, `frco` > 0, and nested trial-based
//! consequences). The last group checks that malformed specs return
//! `ContingencyError::Config` instead of panicking.

use std::collections::HashSet;

use serde_json::{json, Value};

use contingency::spec::{from_spec, SPEC_TYPES};
use contingency::{ContingencyError, Outcome, ResponseEvent, Schedule};

fn build(spec: Value) -> Box<dyn Schedule> {
    from_spec(&spec).unwrap_or_else(|e| panic!("from_spec({spec}) failed: {e}"))
}

/// Step `schedule` through `(now, operandum)` pairs; `None` is a tick
/// with no response.
fn run(schedule: &mut dyn Schedule, steps: &[(f64, Option<&str>)]) -> Vec<Outcome> {
    steps
        .iter()
        .map(|(now, op)| {
            let ev = op.map(|o| ResponseEvent::on(o, *now));
            schedule
                .step(*now, ev.as_ref())
                .unwrap_or_else(|e| panic!("step at {now} failed: {e}"))
        })
        .collect()
}

/// Indices of the reinforced steps.
fn reinforced_at(outcomes: &[Outcome]) -> Vec<usize> {
    outcomes
        .iter()
        .enumerate()
        .filter(|(_, o)| o.reinforced)
        .map(|(i, _)| i)
        .collect()
}

/// One response on `main` every 0.5 time units, `n` responses.
fn responses(n: usize) -> Vec<(f64, Option<&'static str>)> {
    (1..=n).map(|i| (i as f64 * 0.5, Some("main"))).collect()
}

/// Build the spec, step it through `steps`, and record its type as
/// covered.
fn smoke(
    covered: &mut HashSet<&'static str>,
    ty: &'static str,
    params: Value,
    steps: &[(f64, Option<&str>)],
) -> Vec<Outcome> {
    let mut s = build(json!({"type": ty, "params": params}));
    covered.insert(ty);
    run(s.as_mut(), steps)
}

// -----------------------------------------------------------------------------
// Every spec type builds from JSON
// -----------------------------------------------------------------------------

#[test]
fn every_spec_type_builds_and_steps() {
    let mut c = HashSet::new();
    let r10 = responses(10);
    let fr = |n: u64| json!({"type": "FR", "params": {"n": n}});

    // Atomic.
    assert_eq!(
        reinforced_at(&smoke(&mut c, "FR", json!({"n": 3}), &r10)),
        vec![2, 5, 8]
    );
    assert_eq!(
        reinforced_at(&smoke(&mut c, "CRF", json!({}), &r10[..3])),
        vec![0, 1, 2]
    );
    assert!(reinforced_at(&smoke(&mut c, "EXT", json!({}), &r10)).is_empty());
    smoke(
        &mut c,
        "VR",
        json!({"mean": 3.0, "n_intervals": 6, "seed": 1}),
        &r10,
    );
    smoke(&mut c, "RR", json!({"probability": 0.5, "seed": 1}), &r10);
    assert_eq!(
        reinforced_at(&smoke(&mut c, "FI", json!({"interval": 2.0}), &r10)),
        vec![3, 7]
    );
    smoke(
        &mut c,
        "VI",
        json!({"mean_interval": 2.0, "n_intervals": 6, "seed": 2}),
        &r10,
    );
    smoke(&mut c, "RI", json!({"mean_interval": 2.0, "seed": 3}), &r10);
    let ticks: Vec<_> = (0..10).map(|i| (i as f64, None)).collect();
    assert_eq!(
        reinforced_at(&smoke(&mut c, "FT", json!({"interval": 3.0}), &ticks)),
        vec![3, 6, 9]
    );
    smoke(
        &mut c,
        "VT",
        json!({"mean_interval": 2.0, "n_intervals": 6, "seed": 4}),
        &ticks,
    );
    smoke(
        &mut c,
        "RT",
        json!({"mean_interval": 2.0, "seed": 5}),
        &ticks,
    );

    // Wrappers.
    smoke(
        &mut c,
        "LimitedHold",
        json!({"inner": {"type": "FI", "params": {"interval": 2.0}}, "hold": 1.0}),
        &r10,
    );
    smoke(
        &mut c,
        "Timeout",
        json!({"inner": fr(2), "duration": 1.0, "reset_on_response": true}),
        &r10,
    );
    smoke(
        &mut c,
        "ResponseCost",
        json!({"inner": fr(2), "amount": 1.0, "unit": "token", "initial_balance": 3.0}),
        &r10,
    );

    // Differential.
    smoke(
        &mut c,
        "DRO",
        json!({"interval": 2.0, "dro_type": "momentary"}),
        &ticks,
    );
    smoke(&mut c, "DRL", json!({"interval": 1.0}), &r10);
    smoke(
        &mut c,
        "DRH",
        json!({"response_count": 2, "time_window": 1.0}),
        &r10,
    );
    smoke(
        &mut c,
        "Percentile",
        json!({"target": "irt", "rank": 50, "window": 3}),
        &r10,
    );

    // Compound.
    let conc_steps = [
        (0.5, Some("left")),
        (1.0, Some("right")),
        (1.5, Some("left")),
        (2.0, Some("left")),
    ];
    smoke(
        &mut c,
        "Concurrent",
        json!({"components": {"left": fr(1), "right": fr(1)}, "cod": 0.0}),
        &conc_steps,
    );
    smoke(
        &mut c,
        "Alternative",
        json!({"first": fr(3), "second": {"type": "FT", "params": {"interval": 10.0}}}),
        &r10,
    );
    smoke(
        &mut c,
        "Conjunctive",
        json!({"first": fr(2), "second": {"type": "FI", "params": {"interval": 2.0}}}),
        &r10,
    );
    smoke(
        &mut c,
        "Multiple",
        json!({"components": [fr(2), fr(3)], "stimuli": ["red", "green"]}),
        &r10,
    );
    smoke(&mut c, "Mixed", json!({"components": [fr(2), fr(3)]}), &r10);
    smoke(
        &mut c,
        "Chained",
        json!({"components": [fr(2), fr(3)]}),
        &r10,
    );
    assert_eq!(
        reinforced_at(&smoke(
            &mut c,
            "Tandem",
            json!({"components": [fr(2), fr(3)]}),
            &r10
        )),
        vec![4, 9]
    );
    smoke(
        &mut c,
        "Interpolate",
        json!({"base": fr(2), "probe": {"type": "EXT"}, "interval": 2.0, "probe_duration": 1.0}),
        &r10,
    );
    smoke(
        &mut c,
        "Overlay",
        json!({"base": fr(2), "punisher": fr(3)}),
        &r10,
    );
    smoke(
        &mut c,
        "SecondOrder",
        json!({"overall": fr(2), "unit": fr(2)}),
        &r10,
    );

    // Progressive and adjusting.
    assert_eq!(
        reinforced_at(&smoke(
            &mut c,
            "ProgressiveRatio",
            json!({"step": "arithmetic", "start": 1, "step_size": 1}),
            &r10
        )),
        vec![0, 2, 5, 9]
    );
    smoke(
        &mut c,
        "InterlockingSchedule",
        json!({"initial_ratio": 4, "decay_time": 8.0}),
        &r10,
    );
    smoke(
        &mut c,
        "AdjustingSchedule",
        json!({"target": "ratio", "start": 2.0, "step": 1.0, "maximum": 3.0}),
        &r10,
    );

    // Aversive.
    smoke(
        &mut c,
        "Sidman",
        json!({"shock_shock_interval": 2.0, "response_shock_interval": 3.0}),
        &ticks,
    );
    smoke(
        &mut c,
        "DiscriminatedAvoidance",
        json!({"warning_duration": 2.0, "iti": 2.0}),
        &ticks,
    );
    smoke(
        &mut c,
        "Escape",
        json!({"trial_duration": 2.0, "iti": 1.0}),
        &ticks,
    );

    // Trial-based.
    smoke(
        &mut c,
        "MatchingToSample",
        json!({"n_comparisons": 2, "seed": 7}),
        &ticks,
    );
    smoke(&mut c, "GoNoGo", json!({"seed": 1}), &ticks);

    let all: HashSet<&str> = SPEC_TYPES.iter().copied().collect();
    assert_eq!(SPEC_TYPES.len(), 36);
    assert_eq!(all.len(), 36, "SPEC_TYPES has duplicates");
    let missing: Vec<_> = SPEC_TYPES.iter().filter(|t| !c.contains(*t)).collect();
    assert!(
        missing.is_empty(),
        "spec types without a build test: {missing:?}"
    );
    assert_eq!(c.len(), 36);
}

#[test]
fn params_may_be_absent_or_null() {
    for spec in [
        json!({"type": "EXT"}),
        json!({"type": "CRF", "params": null}),
        json!({"type": "CRF", "params": {}}),
    ] {
        build(spec);
    }
}

// -----------------------------------------------------------------------------
// Parameters the conformance fixtures do not exercise
// -----------------------------------------------------------------------------

#[test]
fn limited_hold_accepts_vi_and_ri_inner() {
    for inner in [
        json!({"type": "VI", "params": {"mean_interval": 2.0, "n_intervals": 6, "seed": 7}}),
        json!({"type": "RI", "params": {"mean_interval": 2.0, "seed": 7}}),
    ] {
        let mut s = build(json!({"type": "LimitedHold", "params": {"inner": inner, "hold": 0.5}}));
        run(s.as_mut(), &responses(20));
    }
}

#[test]
fn percentile_direction_below_builds() {
    let mut s = build(json!({"type": "Percentile", "params": {
        "target": "irt", "rank": 25, "window": 3, "direction": "below"}}));
    run(s.as_mut(), &responses(10));
}

#[test]
fn progressive_ratio_geometric_and_richardson_roberts() {
    // Geometric start 1, ratio 2: requirements 1, 2, 4.
    let mut s = build(json!({"type": "ProgressiveRatio", "params": {
        "step": "geometric", "start": 1, "ratio": 2.0}}));
    assert_eq!(
        reinforced_at(&run(s.as_mut(), &responses(7))),
        vec![0, 2, 6]
    );

    let mut s =
        build(json!({"type": "ProgressiveRatio", "params": {"step": "richardson_roberts"}}));
    assert!(!reinforced_at(&run(s.as_mut(), &responses(10))).is_empty());
}

#[test]
fn adjusting_interval_with_minimum_builds() {
    let mut s = build(json!({"type": "AdjustingSchedule", "params": {
        "target": "interval", "start": 2.0, "step": 1.0, "minimum": 1.0, "maximum": null}}));
    run(s.as_mut(), &responses(20));
}

#[test]
fn shock_magnitude_is_passed_through() {
    let mut s = build(json!({"type": "Sidman", "params": {
        "shock_shock_interval": 2.0, "response_shock_interval": 3.0, "shock_magnitude": 2.5}}));
    let ticks: Vec<_> = (1..=4).map(|i| (i as f64, None)).collect();
    let outcomes = run(s.as_mut(), &ticks);
    let shock = outcomes
        .iter()
        .find_map(|o| o.reinforcer.as_ref())
        .expect("Sidman delivers a shock without responses");
    assert!(
        (shock.magnitude.abs() - 2.5).abs() < 1e-9,
        "magnitude {}",
        shock.magnitude
    );
}

#[test]
fn concurrent_frco_is_passed_through() {
    let spec = |frco: u64| {
        json!({"type": "Concurrent", "params": {
            "components": {"a": {"type": "CRF"}, "b": {"type": "CRF"}},
            "cod": 1.0, "frco": frco}})
    };
    // With frco 0 the first response on `b` is a changeover and falls
    // inside the COD. With frco 2 it is not yet a changeover, so it is
    // reinforced; the second response on `b` completes the changeover
    // and is COD-gated.
    let steps = [(0.5, Some("a")), (1.0, Some("b")), (1.5, Some("b"))];
    let mut s0 = build(spec(0));
    let mut s2 = build(spec(2));
    assert_eq!(reinforced_at(&run(s0.as_mut(), &steps)), vec![0]);
    assert_eq!(reinforced_at(&run(s2.as_mut(), &steps)), vec![0, 1]);
}

#[test]
fn trial_based_nested_specs_build() {
    let fr1 = json!({"type": "FR", "params": {"n": 1}});
    let mut mts = build(json!({"type": "MatchingToSample", "params": {
        "n_comparisons": 3, "sample_duration": 0.0, "choice_timeout": 2.0,
        "consequence": fr1, "incorrect": {"type": "EXT"}, "iti": 1.0, "seed": 3}}));
    run(
        mts.as_mut(),
        &[(0.0, None), (0.5, Some("choice_0")), (3.0, None)],
    );

    let mut gng = build(json!({"type": "GoNoGo", "params": {
        "go_probability": 0.5, "response_window": 2.0, "iti": 1.0,
        "correct_go_schedule": fr1, "correct_nogo_schedule": {"type": "EXT"},
        "false_alarm": {"type": "EXT"}, "seed": 1}}));
    run(
        gng.as_mut(),
        &[
            (0.0, None),
            (0.5, Some("main")),
            (3.0, None),
            (4.5, Some("main")),
        ],
    );
}

// -----------------------------------------------------------------------------
// Malformed specs return Config errors
// -----------------------------------------------------------------------------

fn config_error(spec: Value) -> String {
    match from_spec(&spec) {
        Err(ContingencyError::Config(msg)) => msg,
        Err(other) => panic!("{spec}: expected Config error, got {other:?}"),
        Ok(_) => panic!("{spec}: expected Config error, got a schedule"),
    }
}

#[test]
fn malformed_specs_are_config_errors() {
    let cases = [
        json!(5),
        json!({"params": {}}),
        json!({"type": "Nope"}),
        json!({"type": "fr", "params": {"n": 5}}),
        json!({"type": "FR", "params": []}),
        json!({"type": "FR", "params": {}}),
        json!({"type": "FR", "params": {"n": 5.0}}),
        json!({"type": "FR", "params": {"n": -1}}),
        json!({"type": "FR", "params": {"n": 0}}),
        json!({"type": "FI", "params": {"interval": "10"}}),
        json!({"type": "VI", "params": {"mean_interval": 2.0, "seed": 1.5}}),
        json!({"type": "DRO", "params": {"interval": 2.0, "dro_type": "whole"}}),
        json!({"type": "Percentile", "params": {"target": "IRT", "rank": 50, "window": 3}}),
        json!({"type": "Percentile", "params": {"target": "irt", "rank": 300, "window": 3}}),
        json!({"type": "AdjustingSchedule", "params": {"target": "delay", "start": 1.0, "step": 1.0}}),
        json!({"type": "ProgressiveRatio", "params": {"step": "linear"}}),
        json!({"type": "LimitedHold", "params": {"inner": {"type": "FR", "params": {"n": 2}}, "hold": 1.0}}),
        json!({"type": "Timeout", "params": {"duration": 1.0}}),
        json!({"type": "Concurrent", "params": {"components": [{"type": "CRF"}]}}),
        json!({"type": "Concurrent", "params": {"components": {"a": {"type": "CRF"}}}}),
        json!({"type": "Multiple", "params": {"components": [{"type": "CRF"}], "stimuli": [1]}}),
        json!({"type": "MatchingToSample", "params": {"n_comparisons": 2, "consequence": {"type": "Nope"}}}),
    ];
    for spec in cases {
        config_error(spec);
    }
}

#[test]
fn nested_errors_name_their_path() {
    let msg = config_error(json!({"type": "Alternative", "params": {
        "first": {"type": "FR", "params": {"n": 2}},
        "second": {"type": "FI", "params": {}}}}));
    assert!(msg.contains("Alternative.second"), "{msg}");
    assert!(msg.contains("interval"), "{msg}");
}

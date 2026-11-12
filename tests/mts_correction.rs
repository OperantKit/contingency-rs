//! Correction trials of `MatchingToSample` built from its spec.
//!
//! With `correction` active, a baseline trial that ends incorrect (a
//! wrong choice or a timeout) is followed, after the ITI, by the same
//! trial again: the same sample, the same comparison positions and the
//! same correct operandum, with no draw from the random stream.
//! `{"mode": "unlimited"}` repeats until correct; `{"mode": "bounded",
//! "limit": N}` repeats at most N times, then the next trial is drawn.
//! Probe trials are never repeated.

use serde_json::{json, Value};

use contingency::spec::from_spec;
use contingency::types::MetaValue;
use contingency::{Outcome, ResponseEvent, Schedule};

fn trial_types() -> Value {
    json!([
        {"sample": "A1", "correct": "B1", "comparisons": ["B1", "B2", "B3"]},
        {"sample": "A2", "correct": "B2", "comparisons": ["B1", "B2", "B3"]},
        {"sample": "A3", "correct": "B3", "comparisons": ["B1", "B2", "B3"]},
    ])
}

fn mts(correction: Option<Value>) -> Box<dyn Schedule> {
    let mut params = json!({
        "n_comparisons": 3,
        "sample_duration": 0.0,
        "choice_timeout": 5.0,
        "iti": 1.0,
        "seed": 11,
        "trial_types": trial_types(),
    });
    if let Some(c) = correction {
        params["correction"] = c;
    }
    from_spec(&json!({"type": "MatchingToSample", "params": params})).unwrap()
}

fn s(meta: &MetaValue) -> String {
    match meta {
        MetaValue::Str(v) => v.clone(),
        other => panic!("not a string: {other:?}"),
    }
}

fn state_str(sched: &dyn Schedule, key: &str) -> String {
    s(sched.state().get(key).unwrap())
}

fn positions(sched: &dyn Schedule) -> MetaValue {
    sched.state().get("current_positions").unwrap().clone()
}

/// One trial: answer `correct` or a wrong comparison at `t`, then tick
/// through the 1 s ITI. Returns the choice outcome.
fn answer(sched: &mut dyn Schedule, t: f64, correct: bool) -> Outcome {
    let right = state_str(sched, "correct_operandum");
    let op = if correct {
        right
    } else {
        (0..3)
            .map(|i| format!("choice_{i}"))
            .find(|c| *c != right)
            .unwrap()
    };
    let out = sched.step(t, Some(&ResponseEvent::on(op, t))).unwrap();
    sched.step(t + 1.0, None).unwrap();
    out
}

/// (correct operandum, positions) of the current trial.
fn trial(sched: &dyn Schedule) -> (String, MetaValue) {
    (state_str(sched, "correct_operandum"), positions(sched))
}

#[test]
fn unlimited_repeats_the_same_trial_until_correct() {
    let mut m = mts(Some(json!({"mode": "unlimited"})));
    m.step(0.0, None).unwrap();
    let first = trial(m.as_ref());
    let mut t = 0.5;
    for _ in 0..4 {
        answer(m.as_mut(), t, false);
        t += 2.0;
        assert_eq!(trial(m.as_ref()), first);
    }
    let out = answer(m.as_mut(), t, true);
    assert!(out.reinforced);
    assert_eq!(
        out.meta.get("correction_trial"),
        Some(&MetaValue::Bool(true))
    );
}

#[test]
fn bounded_two_draws_a_new_trial_after_two_repeats() {
    // The same seed without correction gives the trial sequence the
    // schedule would draw; bounded correction must reach the second
    // drawn trial on the fourth trial after three incorrect answers.
    let mut plain = mts(None);
    plain.step(0.0, None).unwrap();
    answer(plain.as_mut(), 0.5, false);
    let drawn_second = trial(plain.as_ref());

    let mut m = mts(Some(json!({"mode": "bounded", "limit": 2})));
    m.step(0.0, None).unwrap();
    let first = trial(m.as_ref());
    let mut flags = Vec::new();
    let mut t = 0.5;
    for _ in 0..3 {
        assert_eq!(trial(m.as_ref()), first);
        let out = answer(m.as_mut(), t, false);
        flags.push(out.meta.get("correction_trial").cloned());
        t += 2.0;
    }
    assert_eq!(
        flags,
        vec![
            Some(MetaValue::Bool(false)),
            Some(MetaValue::Bool(true)),
            Some(MetaValue::Bool(true)),
        ]
    );
    let fourth = trial(m.as_ref());
    assert_eq!(fourth, drawn_second);
    assert_ne!(fourth, first);
}

#[test]
fn a_timeout_counts_as_incorrect_for_correction() {
    let mut m = mts(Some(json!({"mode": "unlimited"})));
    m.step(0.0, None).unwrap();
    let first = trial(m.as_ref());
    // Choice opens at 0 and times out after 5 s; ITI 1 s.
    m.step(5.5, None).unwrap();
    m.step(6.6, None).unwrap();
    assert_eq!(trial(m.as_ref()), first);
    assert_eq!(state_str(m.as_ref(), "phase"), "choice");
}

#[test]
fn the_iti_runs_before_the_correction_trial() {
    let mut m = mts(Some(json!({"mode": "unlimited"})));
    m.step(0.0, None).unwrap();
    let wrong = (0..3)
        .map(|i| format!("choice_{i}"))
        .find(|c| *c != state_str(m.as_ref(), "correct_operandum"))
        .unwrap();
    m.step(0.5, Some(&ResponseEvent::on(wrong, 0.5))).unwrap();
    m.step(1.0, None).unwrap();
    assert_eq!(state_str(m.as_ref(), "phase"), "iti");
    m.step(1.5, None).unwrap();
    assert_eq!(state_str(m.as_ref(), "phase"), "choice");
}

#[test]
fn disabled_and_omitted_match_the_schedule_without_correction() {
    let script = |m: &mut dyn Schedule| {
        let mut outs = Vec::new();
        outs.push(m.step(0.0, None).unwrap());
        let mut t = 0.5;
        for k in 0..6 {
            outs.push(answer(m, t, k % 3 == 2));
            t += 2.0;
        }
        format!("{outs:?}")
    };
    let mut omitted = mts(None);
    let mut disabled = mts(Some(json!({"mode": "disabled"})));
    let a = script(omitted.as_mut());
    let b = script(disabled.as_mut());
    assert_eq!(a, b);
    assert!(!a.contains("correction_trial"));
}

#[test]
fn reset_clears_a_pending_correction() {
    let mut m = mts(Some(json!({"mode": "unlimited"})));
    m.step(0.0, None).unwrap();
    let first = trial(m.as_ref());
    answer(m.as_mut(), 0.5, false);
    m.reset();
    m.step(0.0, None).unwrap();
    assert_eq!(trial(m.as_ref()), first);
    let out = answer(m.as_mut(), 0.5, true);
    assert_eq!(
        out.meta.get("correction_trial"),
        Some(&MetaValue::Bool(false))
    );
}

#[test]
fn works_without_trial_types() {
    let mut m = from_spec(&json!({"type": "MatchingToSample", "params": {
        "n_comparisons": 4, "iti": 1.0, "seed": 1,
        "correction": {"mode": "bounded", "limit": 1},
    }}))
    .unwrap();
    // Seed 1 draws choice_1 then choice_0, so without correction the
    // second trial would have another correct operandum.
    m.step(0.0, None).unwrap();
    let first = state_str(m.as_ref(), "correct_operandum");
    assert_eq!(first, "choice_1");
    answer(m.as_mut(), 0.5, false);
    assert_eq!(state_str(m.as_ref(), "correct_operandum"), first);
    answer(m.as_mut(), 2.5, false);
    assert_eq!(state_str(m.as_ref(), "correct_operandum"), "choice_0");
}

#[test]
fn bad_correction_specs_are_rejected() {
    for bad in [
        json!({"mode": "bounded"}),
        json!({"mode": "bounded", "limit": 0}),
        json!({"mode": "bounded", "limit": -1}),
        json!({"mode": "unlimited", "limit": 2}),
        json!({"mode": "disabled", "limit": 2}),
        json!({"mode": "sometimes"}),
        json!({"limit": 2}),
        json!(true),
        json!(2),
        json!({"mode": "bounded", "limit": 2, "extra": 1}),
    ] {
        let spec = json!({"type": "MatchingToSample", "params": {
            "n_comparisons": 2, "correction": bad,
        }});
        assert!(from_spec(&spec).is_err(), "accepted {bad}");
    }
}

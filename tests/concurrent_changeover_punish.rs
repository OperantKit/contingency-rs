//! Changeover punishment of `Concurrent` built from its spec.
//!
//! `punish_changeover` is one punishment schedule shared by every
//! changeover direction; `punish_directional` gives a punishment
//! schedule per `(from, to)` direction. A punishment schedule receives
//! a response only when that response confirms a changeover (the same
//! test that arms COD, so with FRCO the confirming response is the
//! FRCO-th one on the new operandum); every other step is a time tick.

use serde_json::{json, Value};

use contingency::spec::from_spec;
use contingency::types::MetaValue;
use contingency::{Outcome, ResponseEvent, Schedule};

fn conc(extra: Value) -> Box<dyn Schedule> {
    let mut params = json!({
        "components": {
            "c0": {"type": "EXT"},
            "c1": {"type": "EXT"},
        },
        "cod": 2.0,
    });
    for (k, v) in extra.as_object().unwrap() {
        params[k] = v.clone();
    }
    from_spec(&json!({"type": "Concurrent", "params": params})).unwrap()
}

fn run(schedule: &mut Box<dyn Schedule>, ops: &[&str]) -> Vec<Outcome> {
    ops.iter()
        .enumerate()
        .map(|(i, op)| {
            let t = (i + 1) as f64;
            let ev = ResponseEvent::on(*op, t);
            schedule.step(t, Some(&ev)).unwrap()
        })
        .collect()
}

fn punished(outs: &[Outcome]) -> Vec<bool> {
    outs.iter()
        .map(|o| o.meta.get("punisher_fired") == Some(&MetaValue::Bool(true)))
        .collect()
}

#[test]
fn changeover_punishes_every_changeover_and_nothing_else() {
    let mut s = conc(json!({"punish_changeover": {"type": "FR", "params": {"n": 1}}}));
    let outs = run(&mut s, &["c0", "c0", "c1", "c1", "c0"]);
    assert_eq!(punished(&outs), vec![false, false, true, false, true]);
    for i in [2, 4] {
        let r = outs[i].reinforcer.as_ref().unwrap();
        assert_eq!(r.label, "SR-");
        assert_eq!(r.magnitude, -1.0);
        assert_eq!(
            outs[i].meta.get("changeover_punished"),
            Some(&MetaValue::Bool(true))
        );
    }
}

#[test]
fn changeover_schedule_is_shared_across_directions() {
    // FR 2 shared: the 2nd changeover overall is punished, whatever its
    // direction (c0->c1 then c1->c0).
    let mut s = conc(json!({"punish_changeover": {"type": "FR", "params": {"n": 2}}}));
    let outs = run(&mut s, &["c0", "c1", "c0", "c1", "c0"]);
    assert_eq!(punished(&outs), vec![false, false, true, false, true]);
}

#[test]
fn directional_punishes_only_its_direction() {
    let mut s = conc(json!({"punish_directional": [
        {"from": "c0", "to": "c1", "schedule": {"type": "FR", "params": {"n": 1}}}
    ]}));
    let outs = run(&mut s, &["c0", "c0", "c1", "c1", "c0"]);
    assert_eq!(punished(&outs), vec![false, false, true, false, false]);
}

#[test]
fn directional_schedules_are_separate_per_direction() {
    let mut s = conc(json!({"punish_directional": [
        {"from": "c0", "to": "c1", "schedule": {"type": "FR", "params": {"n": 2}}},
        {"from": "c1", "to": "c0", "schedule": {"type": "FR", "params": {"n": 1}}}
    ]}));
    // c0->c1 (1st of FR2), c1->c0 (FR1 fires), c0->c1 (2nd of FR2 fires)
    let outs = run(&mut s, &["c0", "c1", "c0", "c1"]);
    assert_eq!(punished(&outs), vec![false, false, true, true]);
}

#[test]
fn frco_punishes_the_confirming_response() {
    let mut s = conc(json!({
        "frco": 2,
        "punish_changeover": {"type": "FR", "params": {"n": 1}}
    }));
    // c1 #1 is not yet a changeover; c1 #2 confirms it.
    let outs = run(&mut s, &["c0", "c1", "c1", "c0", "c1"]);
    assert_eq!(punished(&outs), vec![false, false, true, false, false]);
}

#[test]
fn component_and_directional_punishers_add_up() {
    let mut s = conc(json!({
        "punish": {"c1": {"type": "FR", "params": {"n": 1}}},
        "punish_directional": [
            {"from": "c0", "to": "c1", "schedule": {"type": "FR", "params": {"n": 1}}}
        ]
    }));
    let outs = run(&mut s, &["c0", "c1", "c1"]);
    assert_eq!(punished(&outs), vec![false, true, true]);
    assert_eq!(outs[1].reinforcer.as_ref().unwrap().magnitude, -2.0);
    assert_eq!(outs[2].reinforcer.as_ref().unwrap().magnitude, -1.0);
    assert_eq!(outs[2].meta.get("changeover_punished"), None);
}

#[test]
fn reinforcer_on_a_changeover_response_nets_with_the_punisher() {
    // No COD: the changeover response on c1 earns CRF and is punished.
    let spec = json!({"type": "Concurrent", "params": {
        "components": {"c0": {"type": "EXT"}, "c1": {"type": "CRF"}},
        "punish_changeover": {"type": "FR", "params": {"n": 1}}
    }});
    let mut s = from_spec(&spec).unwrap();
    let outs = run(&mut s, &["c0", "c1"]);
    let r = outs[1].reinforcer.as_ref().unwrap();
    assert_eq!(r.magnitude, 0.0);
    assert_eq!(r.label, "SR0");
    assert_eq!(outs[1].meta.get("base_fired"), Some(&MetaValue::Bool(true)));
}

#[test]
fn reset_clears_changeover_punishers() {
    let mut s = conc(json!({"punish_changeover": {"type": "FR", "params": {"n": 2}}}));
    let _ = run(&mut s, &["c0", "c1"]);
    s.reset();
    let outs = run(&mut s, &["c0", "c1", "c0"]);
    assert_eq!(punished(&outs), vec![false, false, true]);
}

#[test]
fn without_changeover_punishment_output_is_unchanged() {
    let mut a = conc(json!({}));
    let mut b = conc(json!({"punish_directional": []}));
    let ops = ["c0", "c1", "c0", "c1"];
    let (oa, ob) = (run(&mut a, &ops), run(&mut b, &ops));
    for (x, y) in oa.iter().zip(ob.iter()) {
        assert_eq!(x.reinforced, y.reinforced);
        assert_eq!(x.meta, y.meta);
    }
}

#[test]
fn malformed_specs_are_rejected() {
    let base = json!({"c0": {"type": "EXT"}, "c1": {"type": "EXT"}});
    let fr1 = json!({"type": "FR", "params": {"n": 1}});
    let bad = [
        // both forms at once
        json!({"punish_changeover": fr1, "punish_directional": [
            {"from": "c0", "to": "c1", "schedule": fr1}]}),
        // unknown operandum
        json!({"punish_directional": [{"from": "c0", "to": "c9", "schedule": fr1}]}),
        // self-transition
        json!({"punish_directional": [{"from": "c0", "to": "c0", "schedule": fr1}]}),
        // repeated pair
        json!({"punish_directional": [
            {"from": "c0", "to": "c1", "schedule": fr1},
            {"from": "c0", "to": "c1", "schedule": fr1}]}),
        // wrong shapes
        json!({"punish_directional": {"from": "c0"}}),
        json!({"punish_directional": [{"from": "c0", "to": "c1"}]}),
        json!({"punish_directional": [{"from": "c0", "to": "c1", "schedule": fr1, "x": 1}]}),
        json!({"punish_changeover": 3}),
    ];
    for extra in bad {
        let mut params = json!({"components": base.clone()});
        for (k, v) in extra.as_object().unwrap() {
            params[k] = v.clone();
        }
        let spec = json!({"type": "Concurrent", "params": params});
        assert!(from_spec(&spec).is_err(), "accepted {spec}");
    }
}

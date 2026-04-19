//! Integration tests for `contingency::bridge::from_dsl_*`.
//!
//! Mirrors key cases of `contingency-py/tests/test_bridge.py`.

use contingency::{from_dsl_expr, from_dsl_str, ResponseEvent, Schedule};
use contingency_dsl::ast::{
    Annotation, AnnotationValue, AnnotatedSchedule, Atomic, CombinatorParamValue, Compound,
    CompoundParam, DirRef, DirectionalCOD, DirectionalEntry, IdentifierRef, Modifier, ModifierKind,
    NumericPayload, ScheduleExpr, ScheduleType, SecondOrder as DslSecondOrder, SecondOrderSlot,
    Special, SpecialKind, Timeout as DslTimeout, TimeoutWrapped, ResponseCost as DslResponseCost,
    ResponseCostWrapped, AdjustingSchedule as DslAdjusting, InterlockingSchedule as DslInterlocking,
    TrialBased, AversiveSchedule, AversiveKind, LimitedHold as DslLimitedHold,
};
use contingency_dsl::enums::{Combinator, Distribution, Domain};
use std::collections::BTreeMap;

fn atomic(dist: Distribution, dom: Domain, value: f64, unit: Option<&str>) -> ScheduleExpr {
    ScheduleExpr::Atomic(Atomic {
        schedule_type: ScheduleType::new(dist, dom),
        value,
        time_unit: unit.map(str::to_string),
    })
}

fn respond<S: Schedule + ?Sized>(s: &mut S, now: f64) -> bool {
    let ev = ResponseEvent::new(now);
    s.step(now, Some(&ev)).unwrap().reinforced
}

fn tick<S: Schedule + ?Sized>(s: &mut S, now: f64) -> bool {
    s.step(now, None).unwrap().reinforced
}

// ---- Atomic ---------------------------------------------------------------

#[test]
fn atomic_fr5_reinforces_on_fifth_response() {
    let node = atomic(Distribution::Fixed, Domain::Ratio, 5.0, None);
    let mut s = from_dsl_expr(&node, 1.0).unwrap();
    let mut fired = 0;
    for i in 1..=10u64 {
        if respond(&mut *s, i as f64) {
            fired += 1;
        }
    }
    assert_eq!(fired, 2);
}

#[test]
fn atomic_fi30s_reinforces_on_response_after_interval() {
    let node = atomic(Distribution::Fixed, Domain::Interval, 30.0, Some("s"));
    let mut s = from_dsl_expr(&node, 1.0).unwrap();
    assert!(!respond(&mut *s, 10.0));
    assert!(respond(&mut *s, 30.0));
}

#[test]
fn atomic_ft5s_delivers_reinforcer_on_tick() {
    let node = atomic(Distribution::Fixed, Domain::Time, 5.0, Some("s"));
    let mut s = from_dsl_expr(&node, 1.0).unwrap();
    let mut fired = false;
    for t in [1.0, 2.0, 3.0, 4.0, 5.0, 6.0].into_iter() {
        if tick(&mut *s, t) {
            fired = true;
        }
    }
    assert!(fired);
}

#[test]
fn atomic_vr10_builds_and_runs() {
    let node = atomic(Distribution::Variable, Domain::Ratio, 10.0, None);
    let mut s = from_dsl_expr(&node, 1.0).unwrap();
    let mut fired = 0;
    for i in 1..=200u64 {
        if respond(&mut *s, i as f64) {
            fired += 1;
        }
    }
    assert!(fired >= 5, "VR10 should fire several times in 200 responses");
}

#[test]
fn atomic_vi60s_builds_and_runs() {
    let node = atomic(Distribution::Variable, Domain::Interval, 60.0, Some("s"));
    let mut s = from_dsl_expr(&node, 1.0).unwrap();
    let mut fired = 0;
    for i in 1..=2000u64 {
        if respond(&mut *s, i as f64) {
            fired += 1;
        }
    }
    assert!(fired >= 5);
}

#[test]
fn atomic_vt60s_builds() {
    let node = atomic(Distribution::Variable, Domain::Time, 60.0, Some("s"));
    let mut s = from_dsl_expr(&node, 1.0).unwrap();
    for t in 1..=300u64 {
        let _ = tick(&mut *s, t as f64);
    }
}

#[test]
fn atomic_rr10_builds_and_fires() {
    let node = atomic(Distribution::Random, Domain::Ratio, 10.0, None);
    let mut s = from_dsl_expr(&node, 1.0).unwrap();
    let mut fired = 0;
    for i in 1..=1000u64 {
        if respond(&mut *s, i as f64) {
            fired += 1;
        }
    }
    assert!((30..=200).contains(&fired));
}

#[test]
fn atomic_ri60s_builds() {
    let node = atomic(Distribution::Random, Domain::Interval, 60.0, Some("s"));
    let _ = from_dsl_expr(&node, 1.0).unwrap();
}

#[test]
fn atomic_rt60s_builds() {
    let node = atomic(Distribution::Random, Domain::Time, 60.0, Some("s"));
    let _ = from_dsl_expr(&node, 1.0).unwrap();
}

// ---- Specials -------------------------------------------------------------

#[test]
fn special_ext_never_reinforces() {
    let node = ScheduleExpr::Special(Special { kind: SpecialKind::Ext });
    let mut s = from_dsl_expr(&node, 1.0).unwrap();
    for i in 1..=50u64 {
        assert!(!respond(&mut *s, i as f64));
    }
}

#[test]
fn special_crf_reinforces_every_response() {
    let node = ScheduleExpr::Special(Special { kind: SpecialKind::Crf });
    let mut s = from_dsl_expr(&node, 1.0).unwrap();
    for i in 1..=5u64 {
        assert!(respond(&mut *s, i as f64));
    }
}

// ---- Time units -----------------------------------------------------------

#[test]
fn time_unit_ms_scales_to_seconds() {
    let node = atomic(Distribution::Fixed, Domain::Interval, 500.0, Some("ms"));
    // FI500ms == 0.5s; response at 0.5 should fire.
    let mut s = from_dsl_expr(&node, 1.0).unwrap();
    assert!(respond(&mut *s, 0.5));
}

#[test]
fn time_unit_min_scales_to_seconds() {
    let node = atomic(Distribution::Fixed, Domain::Interval, 2.0, Some("min"));
    // FI2min == 120s.
    let mut s = from_dsl_expr(&node, 1.0).unwrap();
    assert!(!respond(&mut *s, 60.0));
    assert!(respond(&mut *s, 120.0));
}

#[test]
fn time_unit_unknown_errors() {
    let node = atomic(Distribution::Fixed, Domain::Interval, 1.0, Some("weeks"));
    assert!(from_dsl_expr(&node, 1.0).is_err());
}

#[test]
fn time_unit_seconds_remap_factor() {
    // DSL FI1s with time_unit_seconds=1000 -> 1000 runtime clock units.
    let node = atomic(Distribution::Fixed, Domain::Interval, 1.0, Some("s"));
    let mut s = from_dsl_expr(&node, 1000.0).unwrap();
    assert!(!respond(&mut *s, 500.0));
    assert!(respond(&mut *s, 1000.0));
}

// ---- Compound -------------------------------------------------------------

fn compound(
    combinator: Combinator,
    components: Vec<ScheduleExpr>,
    params: Option<BTreeMap<String, CombinatorParamValue>>,
) -> ScheduleExpr {
    ScheduleExpr::Compound(Compound {
        combinator,
        components,
        params,
    })
}

#[test]
fn compound_conc_fr5_vi30s_builds() {
    let node = compound(
        Combinator::Conc,
        vec![
            atomic(Distribution::Fixed, Domain::Ratio, 5.0, None),
            atomic(Distribution::Variable, Domain::Interval, 30.0, Some("s")),
        ],
        None,
    );
    let _ = from_dsl_expr(&node, 1.0).unwrap();
}

#[test]
fn compound_conc_with_cod_builds() {
    let mut params = BTreeMap::new();
    params.insert(
        "COD".to_string(),
        CombinatorParamValue::Scalar(CompoundParam {
            value: 1.0,
            time_unit: Some("s".into()),
        }),
    );
    let node = compound(
        Combinator::Conc,
        vec![
            atomic(Distribution::Fixed, Domain::Ratio, 5.0, None),
            atomic(Distribution::Variable, Domain::Interval, 30.0, Some("s")),
        ],
        Some(params),
    );
    let _ = from_dsl_expr(&node, 1.0).unwrap();
}

#[test]
fn compound_conc_directional_cod_builds() {
    let mut params = BTreeMap::new();
    params.insert(
        "COD".to_string(),
        CombinatorParamValue::Directional(DirectionalCOD {
            base: Some(CompoundParam {
                value: 0.5,
                time_unit: Some("s".into()),
            }),
            directional: vec![DirectionalEntry {
                from_ref: DirRef::Index(1),
                to_ref: DirRef::Index(2),
                value: 2.0,
                time_unit: Some("s".into()),
            }],
        }),
    );
    let node = compound(
        Combinator::Conc,
        vec![
            atomic(Distribution::Fixed, Domain::Ratio, 5.0, None),
            atomic(Distribution::Fixed, Domain::Ratio, 10.0, None),
        ],
        Some(params),
    );
    let _ = from_dsl_expr(&node, 1.0).unwrap();
}

#[test]
fn compound_alt_of_two_builds() {
    let node = compound(
        Combinator::Alt,
        vec![
            atomic(Distribution::Fixed, Domain::Ratio, 3.0, None),
            atomic(Distribution::Fixed, Domain::Time, 5.0, Some("s")),
        ],
        None,
    );
    let _ = from_dsl_expr(&node, 1.0).unwrap();
}

#[test]
fn compound_alt_of_three_nests_left_associative() {
    let node = compound(
        Combinator::Alt,
        vec![
            atomic(Distribution::Fixed, Domain::Ratio, 3.0, None),
            atomic(Distribution::Fixed, Domain::Ratio, 5.0, None),
            atomic(Distribution::Fixed, Domain::Ratio, 7.0, None),
        ],
        None,
    );
    let _ = from_dsl_expr(&node, 1.0).unwrap();
}

#[test]
fn compound_chain_fr3_fr5_builds_and_reinforces_terminal() {
    let node = compound(
        Combinator::Chain,
        vec![
            atomic(Distribution::Fixed, Domain::Ratio, 3.0, None),
            atomic(Distribution::Fixed, Domain::Ratio, 5.0, None),
        ],
        None,
    );
    let mut s = from_dsl_expr(&node, 1.0).unwrap();
    // First 3 responses complete link 0 (conditioned), next 5 complete link 1 (primary).
    let mut primary_fires = 0;
    for i in 1..=8u64 {
        if respond(&mut *s, i as f64) {
            primary_fires += 1;
        }
    }
    assert_eq!(primary_fires, 1);
}

#[test]
fn compound_tand_fr3_fr5_builds() {
    let node = compound(
        Combinator::Tand,
        vec![
            atomic(Distribution::Fixed, Domain::Ratio, 3.0, None),
            atomic(Distribution::Fixed, Domain::Ratio, 5.0, None),
        ],
        None,
    );
    let _ = from_dsl_expr(&node, 1.0).unwrap();
}

#[test]
fn compound_mult_fr3_fr5_builds() {
    let node = compound(
        Combinator::Mult,
        vec![
            atomic(Distribution::Fixed, Domain::Ratio, 3.0, None),
            atomic(Distribution::Fixed, Domain::Ratio, 5.0, None),
        ],
        None,
    );
    let _ = from_dsl_expr(&node, 1.0).unwrap();
}

#[test]
fn compound_mix_fr3_fr5_builds() {
    let node = compound(
        Combinator::Mix,
        vec![
            atomic(Distribution::Fixed, Domain::Ratio, 3.0, None),
            atomic(Distribution::Fixed, Domain::Ratio, 5.0, None),
        ],
        None,
    );
    let _ = from_dsl_expr(&node, 1.0).unwrap();
}

#[test]
fn compound_conj_fr5_fr3_builds() {
    let node = compound(
        Combinator::Conj,
        vec![
            atomic(Distribution::Fixed, Domain::Ratio, 5.0, None),
            atomic(Distribution::Fixed, Domain::Ratio, 3.0, None),
        ],
        None,
    );
    let _ = from_dsl_expr(&node, 1.0).unwrap();
}

#[test]
fn compound_overlay_requires_exactly_two() {
    let node = compound(
        Combinator::Overlay,
        vec![
            atomic(Distribution::Fixed, Domain::Ratio, 5.0, None),
            atomic(Distribution::Fixed, Domain::Ratio, 10.0, None),
        ],
        None,
    );
    let _ = from_dsl_expr(&node, 1.0).unwrap();

    let bad = compound(
        Combinator::Overlay,
        vec![atomic(Distribution::Fixed, Domain::Ratio, 5.0, None)],
        None,
    );
    assert!(from_dsl_expr(&bad, 1.0).is_err());
}

// ---- Modifiers ------------------------------------------------------------

fn modifier(kind: ModifierKind, value: Option<f64>, unit: Option<&str>) -> ScheduleExpr {
    ScheduleExpr::Modifier(Modifier {
        kind,
        value,
        time_unit: unit.map(str::to_string),
        inner: None,
        pr_step: None,
        pr_start: None,
        pr_increment: None,
        pr_ratio: None,
        pctl_target: None,
        pctl_rank: None,
        pctl_window: None,
        pctl_dir: None,
        length: None,
    })
}

#[test]
fn modifier_drl10s_builds() {
    let node = modifier(ModifierKind::Drl, Some(10.0), Some("s"));
    let _ = from_dsl_expr(&node, 1.0).unwrap();
}

#[test]
fn modifier_dro5s_builds() {
    let node = modifier(ModifierKind::Dro, Some(5.0), Some("s"));
    let _ = from_dsl_expr(&node, 1.0).unwrap();
}

#[test]
fn modifier_drh2s_builds_as_response_count_2() {
    let node = modifier(ModifierKind::Drh, Some(2.0), Some("s"));
    let _ = from_dsl_expr(&node, 1.0).unwrap();
}

#[test]
fn modifier_repeat_chains_inner() {
    let inner = atomic(Distribution::Fixed, Domain::Ratio, 5.0, None);
    let node = ScheduleExpr::Modifier(Modifier {
        kind: ModifierKind::Repeat,
        value: Some(3.0),
        time_unit: None,
        inner: Some(Box::new(inner)),
        pr_step: None,
        pr_start: None,
        pr_increment: None,
        pr_ratio: None,
        pctl_target: None,
        pctl_rank: None,
        pctl_window: None,
        pctl_dir: None,
        length: None,
    });
    let _ = from_dsl_expr(&node, 1.0).unwrap();
}

#[test]
fn modifier_pr_linear_builds() {
    let node = ScheduleExpr::Modifier(Modifier {
        kind: ModifierKind::Pr,
        value: None,
        time_unit: None,
        inner: None,
        pr_step: Some("linear".into()),
        pr_start: Some(1.0),
        pr_increment: Some(2.0),
        pr_ratio: None,
        pctl_target: None,
        pctl_rank: None,
        pctl_window: None,
        pctl_dir: None,
        length: None,
    });
    let mut s = from_dsl_expr(&node, 1.0).unwrap();
    // With start=1, step=2: requirements are 1, 3, 5, 7, ...
    // So 1 + 3 = 4 responses produce 2 reinforcers.
    let mut fired = 0;
    for i in 1..=4u64 {
        if respond(&mut *s, i as f64) {
            fired += 1;
        }
    }
    assert_eq!(fired, 2);
}

#[test]
fn modifier_pr_geometric_builds() {
    let node = ScheduleExpr::Modifier(Modifier {
        kind: ModifierKind::Pr,
        value: None,
        time_unit: None,
        inner: None,
        pr_step: Some("geometric".into()),
        pr_start: Some(1.0),
        pr_increment: None,
        pr_ratio: Some(2.0),
        pctl_target: None,
        pctl_rank: None,
        pctl_window: None,
        pctl_dir: None,
        length: None,
    });
    let _ = from_dsl_expr(&node, 1.0).unwrap();
}

#[test]
fn modifier_pctl_irt_above_builds() {
    let node = ScheduleExpr::Modifier(Modifier {
        kind: ModifierKind::Pctl,
        value: None,
        time_unit: None,
        inner: None,
        pr_step: None,
        pr_start: None,
        pr_increment: None,
        pr_ratio: None,
        pctl_target: Some("irt".into()),
        pctl_rank: Some(50),
        pctl_window: Some(20),
        pctl_dir: Some("above".into()),
        length: None,
    });
    let _ = from_dsl_expr(&node, 1.0).unwrap();
}

// ---- LimitedHold ----------------------------------------------------------

#[test]
fn limited_hold_fi30s_lh10s_builds() {
    let inner = atomic(Distribution::Fixed, Domain::Interval, 30.0, Some("s"));
    let node = ScheduleExpr::LimitedHold(DslLimitedHold {
        hold_duration: 10.0,
        inner: Box::new(inner),
        time_unit: Some("s".into()),
    });
    let _ = from_dsl_expr(&node, 1.0).unwrap();
}

#[test]
fn limited_hold_over_non_interval_errors() {
    let inner = atomic(Distribution::Fixed, Domain::Ratio, 5.0, None);
    let node = ScheduleExpr::LimitedHold(DslLimitedHold {
        hold_duration: 10.0,
        inner: Box::new(inner),
        time_unit: Some("s".into()),
    });
    assert!(from_dsl_expr(&node, 1.0).is_err());
}

// ---- SecondOrder ----------------------------------------------------------

#[test]
fn second_order_fr5_fi30s_builds() {
    let overall = Atomic {
        schedule_type: ScheduleType::new(Distribution::Fixed, Domain::Ratio),
        value: 5.0,
        time_unit: None,
    };
    let unit = Atomic {
        schedule_type: ScheduleType::new(Distribution::Fixed, Domain::Interval),
        value: 30.0,
        time_unit: Some("s".into()),
    };
    let node = ScheduleExpr::SecondOrder(DslSecondOrder {
        overall: SecondOrderSlot::Atomic(overall),
        unit: SecondOrderSlot::Atomic(unit),
    });
    let _ = from_dsl_expr(&node, 1.0).unwrap();
}

// ---- Timeout / ResponseCost ----------------------------------------------

#[test]
fn timeout_wraps_fr5() {
    let inner = atomic(Distribution::Fixed, Domain::Ratio, 5.0, None);
    let node = ScheduleExpr::TimeoutWrapped(TimeoutWrapped {
        inner: Box::new(inner),
        timeout: DslTimeout {
            duration: 10.0,
            duration_unit: Some("s".into()),
            reset_on_response: false,
        },
    });
    let _ = from_dsl_expr(&node, 1.0).unwrap();
}

#[test]
fn response_cost_wraps_fr5() {
    let inner = atomic(Distribution::Fixed, Domain::Ratio, 5.0, None);
    let node = ScheduleExpr::ResponseCostWrapped(ResponseCostWrapped {
        inner: Box::new(inner),
        response_cost: DslResponseCost {
            amount: 2.0,
            unit: "token".into(),
        },
    });
    let _ = from_dsl_expr(&node, 1.0).unwrap();
}

// ---- Adjusting / Interlocking --------------------------------------------

#[test]
fn adjusting_ratio_builds() {
    let node = ScheduleExpr::AdjustingSchedule(DslAdjusting {
        adj_target: "ratio".into(),
        adj_start: NumericPayload { value: 5.0, time_unit: None },
        adj_step: NumericPayload { value: 1.0, time_unit: None },
        adj_min: None,
        adj_max: Some(NumericPayload { value: 20.0, time_unit: None }),
    });
    let _ = from_dsl_expr(&node, 1.0).unwrap();
}

#[test]
fn adjusting_interval_scales_time() {
    let node = ScheduleExpr::AdjustingSchedule(DslAdjusting {
        adj_target: "interval".into(),
        adj_start: NumericPayload { value: 30.0, time_unit: Some("s".into()) },
        adj_step: NumericPayload { value: 5.0, time_unit: Some("s".into()) },
        adj_min: Some(NumericPayload { value: 10.0, time_unit: Some("s".into()) }),
        adj_max: Some(NumericPayload { value: 60.0, time_unit: Some("s".into()) }),
    });
    let _ = from_dsl_expr(&node, 1.0).unwrap();
}

#[test]
fn interlocking_builds() {
    let node = ScheduleExpr::InterlockingSchedule(DslInterlocking {
        interlock_r0: 20,
        interlock_t: NumericPayload { value: 60.0, time_unit: Some("s".into()) },
    });
    let _ = from_dsl_expr(&node, 1.0).unwrap();
}

// ---- Unsupported ---------------------------------------------------------

#[test]
fn trial_based_errors() {
    let node = ScheduleExpr::TrialBased(TrialBased {
        trial_type: "MTS".into(),
        params: BTreeMap::new(),
    });
    assert!(from_dsl_expr(&node, 1.0).is_err());
}

#[test]
fn aversive_schedule_errors() {
    let node = ScheduleExpr::AversiveSchedule(AversiveSchedule {
        kind: AversiveKind::Sidman,
        params: BTreeMap::new(),
    });
    assert!(from_dsl_expr(&node, 1.0).is_err());
}

#[test]
fn identifier_ref_errors() {
    let node = ScheduleExpr::IdentifierRef(IdentifierRef { name: "my_sched".into() });
    assert!(from_dsl_expr(&node, 1.0).is_err());
}

// ---- Annotated wrapper transparency --------------------------------------

#[test]
fn annotated_schedule_unwraps_to_inner() {
    let inner = atomic(Distribution::Fixed, Domain::Ratio, 5.0, None);
    let node = ScheduleExpr::Annotated(Box::new(AnnotatedSchedule {
        expr: inner,
        annotations: vec![Annotation {
            keyword: "operandum".into(),
            positional: Some(AnnotationValue::Str("lever".into())),
            params: None,
            label: None,
        }],
    }));
    let mut s = from_dsl_expr(&node, 1.0).unwrap();
    let mut fired = 0;
    for i in 1..=5u64 {
        if respond(&mut *s, i as f64) {
            fired += 1;
        }
    }
    assert_eq!(fired, 1);
}

// ---- End-to-end from source string ---------------------------------------

#[test]
fn from_dsl_str_fr5() {
    let mut s = from_dsl_str("FR5", 1.0).unwrap();
    let mut fired = 0;
    for i in 1..=10u64 {
        if respond(&mut *s, i as f64) {
            fired += 1;
        }
    }
    assert_eq!(fired, 2);
}

#[test]
fn from_dsl_str_fi30s_reinforces_after_interval() {
    let mut s = from_dsl_str("FI30s", 1.0).unwrap();
    assert!(!respond(&mut *s, 10.0));
    assert!(respond(&mut *s, 30.0));
}

#[test]
fn from_dsl_str_ext() {
    let mut s = from_dsl_str("EXT", 1.0).unwrap();
    for i in 1..=20u64 {
        assert!(!respond(&mut *s, i as f64));
    }
}

#[test]
fn from_dsl_str_invalid_errors() {
    assert!(from_dsl_str("not a valid schedule ???", 1.0).is_err());
}

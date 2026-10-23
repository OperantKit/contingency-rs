//! Cross-language conformance replay tests.
//!
//! Loads each JSON fixture under `../contingency-py/conformance/` and
//! replays its event sequence against the Rust `contingency` runtime.
//!
//! Every fixture (atomic, compound, differential, progressive, wrapper —
//! Timeout / ResponseCost — aversive — Sidman / DiscriminatedAvoidance /
//! Escape — and trial-based) is asserted **bit-equivalent** against
//! Python observed outcomes: both `Outcome.reinforced` and (when
//! reinforced) the `Reinforcer` time, magnitude and label must match
//! within a 1e-9 float tolerance. `Outcome.meta` is not compared.
//!
//! This includes the seeded stochastic fixtures (VR, VI, RI, RR, VT, RT,
//! and the trial-based MatchingToSample / GoNoGo, whose correct
//! comparison and trial type are PRNG draws): those schedules draw from
//! `PyRandom`, a port of CPython's `random.Random`, so the same integer
//! seed yields the same draws as the Python generator.
//!
//! # Parity corpus is single-source
//!
//! The directory `../contingency-py/conformance/` is the **single
//! source of truth** for the cross-language parity corpus:
//! `contingency-py` emits every fixture, and this Rust harness is
//! obligated to replay **every fixture** bit-equivalently through
//! [`run_fixture`]. That obligation is enforced mechanically by
//! `conformance_corpus_completeness`, which walks the corpus and fails
//! if any fixture lacks a `run_fixture(...)` call. A newly generated
//! fixture therefore cannot land un-replayed, and commenting out a
//! `run_fixture(...)` line trips the same guard.
//!
//! # References
//!
//! Fleshler, M., & Hoffman, H. S. (1962). A progression for generating
//! variable-interval schedules. *Journal of the Experimental Analysis
//! of Behavior*, 5(4), 529-530. <https://doi.org/10.1901/jeab.1962.5-529>

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use serde::Deserialize;

use contingency::schedules::{
    arithmetic, crf, geometric, richardson_roberts, AdjustingSchedule, AdjustingTarget,
    Alternative, Chained, Concurrent, Conjunctive, DiscriminatedAvoidance, DroMode, Escape, GoNoGo,
    InterlockingSchedule, Interpolate, LimitedHold, MatchingToSample, Mixed, Multiple, Overlay,
    Percentile, PercentileDirection, PercentileTarget, ProgressiveRatio, ResponseCost, SecondOrder,
    Sidman, Tandem, Timeout, DRH, DRL, DRO, EXT, FI, FR, FT, RI, RR, RT, VI, VR, VT,
};
use contingency::{Outcome, Reinforcer, ResponseEvent, Schedule};

// -----------------------------------------------------------------------------
// Fixture schema
// -----------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct Fixture {
    #[allow(dead_code)]
    name: String,
    #[serde(default)]
    #[allow(dead_code)]
    description: String,
    schedule: ScheduleSpec,
    steps: Vec<StepSpec>,
}

#[derive(Debug, Deserialize)]
struct ScheduleSpec {
    #[serde(rename = "type")]
    ty: String,
    #[serde(default)]
    params: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct StepSpec {
    now: f64,
    event: Option<ResponseEvent>,
    expect: ExpectSpec,
}

#[derive(Debug, Deserialize)]
struct ExpectSpec {
    reinforced: bool,
    #[serde(default)]
    reinforcer: Option<Reinforcer>,
}

// -----------------------------------------------------------------------------
// Path resolution
// -----------------------------------------------------------------------------

fn conformance_dir() -> PathBuf {
    let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    crate_root
        .join("..")
        .join("contingency-py")
        .join("conformance")
}

fn load_fixture(relative: &str) -> Fixture {
    let path = conformance_dir().join(relative);
    let bytes = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read fixture {}: {e}", path.display()));
    serde_json::from_str(&bytes)
        .unwrap_or_else(|e| panic!("failed to parse fixture {}: {e}", path.display()))
}

// -----------------------------------------------------------------------------
// Spec → Schedule translator
// -----------------------------------------------------------------------------

/// Build a boxed `Schedule` from a fixture's schedule spec. Mirrors
/// `contingency-py`'s `build_schedule_from_spec` (see
/// `../contingency-py/scripts/generate_conformance.py`).
fn build_schedule(spec: &ScheduleSpec) -> Box<dyn Schedule> {
    let params = &spec.params;
    match spec.ty.as_str() {
        "FR" => {
            let n = require_u64(params, "n");
            Box::new(FR::new(n).expect("FR params"))
        }
        "CRF" => Box::new(crf()),
        "EXT" => Box::new(EXT::new()),
        "VR" => {
            let mean = require_f64(params, "mean");
            let n_intervals = optional_usize(params, "n_intervals").unwrap_or(12);
            let seed = optional_u64(params, "seed");
            Box::new(VR::new(mean, n_intervals, seed).expect("VR params"))
        }
        "RR" => {
            let probability = require_f64(params, "probability");
            let seed = optional_u64(params, "seed");
            Box::new(RR::new(probability, seed).expect("RR params"))
        }
        "FI" => {
            let interval = require_f64(params, "interval");
            Box::new(FI::new(interval).expect("FI params"))
        }
        "VI" => {
            let mean = require_f64(params, "mean_interval");
            let n_intervals = optional_usize(params, "n_intervals").unwrap_or(12);
            let seed = optional_u64(params, "seed");
            Box::new(VI::new(mean, n_intervals, seed).expect("VI params"))
        }
        "RI" => {
            let mean = require_f64(params, "mean_interval");
            let seed = optional_u64(params, "seed");
            Box::new(RI::new(mean, seed).expect("RI params"))
        }
        "FT" => {
            let interval = require_f64(params, "interval");
            Box::new(FT::new(interval).expect("FT params"))
        }
        "VT" => {
            let mean = require_f64(params, "mean_interval");
            let n_intervals = optional_usize(params, "n_intervals").unwrap_or(12);
            let seed = optional_u64(params, "seed");
            Box::new(VT::new(mean, n_intervals, seed).expect("VT params"))
        }
        "RT" => {
            let mean = require_f64(params, "mean_interval");
            let seed = optional_u64(params, "seed");
            Box::new(RT::new(mean, seed).expect("RT params"))
        }
        "LimitedHold" => {
            let hold = require_f64(params, "hold");
            let inner_spec = inner_spec(params, "inner");
            build_limited_hold(&inner_spec, hold)
        }
        "DRO" => {
            let interval = require_f64(params, "interval");
            let mode = match params
                .get("dro_type")
                .and_then(|v| v.as_str())
                .unwrap_or("resetting")
            {
                "resetting" => DroMode::Resetting,
                "momentary" => DroMode::Momentary,
                other => panic!("unsupported DRO dro_type: {other}"),
            };
            Box::new(DRO::new(interval, mode).expect("DRO params"))
        }
        "DRL" => {
            let interval = require_f64(params, "interval");
            Box::new(DRL::new(interval).expect("DRL params"))
        }
        "DRH" => {
            let response_count = require_u64(params, "response_count") as u32;
            let time_window = require_f64(params, "time_window");
            Box::new(DRH::new(response_count, time_window).expect("DRH params"))
        }
        "Concurrent" => {
            let components_val = params
                .get("components")
                .and_then(|v| v.as_object())
                .expect("Concurrent requires components object");
            // Preserve JSON insertion order via IndexMap by consuming the
            // object iterator directly.
            let mut components: IndexMap<String, Box<dyn Schedule>> = IndexMap::new();
            for (key, cspec_val) in components_val.iter() {
                let cspec: ScheduleSpec = serde_json::from_value(cspec_val.clone())
                    .expect("Concurrent component must be a ScheduleSpec");
                components.insert(key.clone(), build_schedule(&cspec));
            }
            let cod = optional_f64(params, "cod").unwrap_or(0.0);
            let frco = optional_u64(params, "frco").unwrap_or(0) as u32;
            Box::new(Concurrent::new(components, cod, frco).expect("Concurrent params"))
        }
        "Alternative" => {
            let first_spec = inner_spec(params, "first");
            let second_spec = inner_spec(params, "second");
            Box::new(Alternative::new(
                build_schedule(&first_spec),
                build_schedule(&second_spec),
            ))
        }
        "Multiple" => {
            let components = build_component_list(params);
            let stimuli = optional_str_vec(params, "stimuli");
            Box::new(Multiple::new(components, stimuli).expect("Multiple params"))
        }
        "Chained" => {
            let components = build_component_list(params);
            let stimuli = optional_str_vec(params, "stimuli");
            Box::new(Chained::new(components, stimuli).expect("Chained params"))
        }
        "Tandem" => {
            let components = build_component_list(params);
            Box::new(Tandem::new(components).expect("Tandem params"))
        }
        "ProgressiveRatio" => {
            let step_name = params
                .get("step")
                .and_then(|v| v.as_str())
                .expect("ProgressiveRatio requires step name");
            let step_fn = match step_name {
                "arithmetic" => {
                    let start = optional_u64(params, "start").unwrap_or(1) as u32;
                    let step_size = optional_u64(params, "step_size").unwrap_or(1) as u32;
                    arithmetic(start, step_size).expect("arithmetic params")
                }
                "geometric" => {
                    let start = optional_u64(params, "start").unwrap_or(1) as u32;
                    let ratio = optional_f64(params, "ratio").unwrap_or(2.0);
                    geometric(start, ratio).expect("geometric params")
                }
                "richardson_roberts" => richardson_roberts(),
                other => panic!("unsupported PR step: {other}"),
            };
            Box::new(ProgressiveRatio::new(step_fn))
        }
        "Timeout" => {
            let inner = build_schedule(&inner_spec(params, "inner"));
            let duration = require_f64(params, "duration");
            let reset_on_response = optional_bool(params, "reset_on_response").unwrap_or(false);
            Box::new(Timeout::new(inner, duration, reset_on_response).expect("Timeout params"))
        }
        "ResponseCost" => {
            let inner = build_schedule(&inner_spec(params, "inner"));
            let amount = require_f64(params, "amount");
            let unit = params
                .get("unit")
                .and_then(|v| v.as_str())
                .unwrap_or("token")
                .to_string();
            let initial_balance = optional_f64(params, "initial_balance");
            Box::new(
                ResponseCost::new(inner, amount, unit, initial_balance)
                    .expect("ResponseCost params"),
            )
        }
        "Conjunctive" => {
            let first = build_schedule(&inner_spec(params, "first"));
            let second = build_schedule(&inner_spec(params, "second"));
            Box::new(Conjunctive::new(first, second).expect("Conjunctive params"))
        }
        "Mixed" => {
            let components = build_component_list(params);
            Box::new(Mixed::new(components).expect("Mixed params"))
        }
        "InterlockingSchedule" => {
            let initial_ratio = require_u64(params, "initial_ratio");
            let decay_time = require_f64(params, "decay_time");
            Box::new(
                InterlockingSchedule::new(initial_ratio, decay_time)
                    .expect("InterlockingSchedule params"),
            )
        }
        "AdjustingSchedule" => {
            let target = match require_str(params, "target") {
                "ratio" => AdjustingTarget::Ratio,
                "interval" => AdjustingTarget::Interval,
                other => panic!("unsupported AdjustingSchedule target: {other}"),
            };
            let start = require_f64(params, "start");
            let step = require_f64(params, "step");
            let minimum = optional_f64(params, "minimum");
            let maximum = optional_f64(params, "maximum");
            Box::new(
                AdjustingSchedule::new(target, start, step, minimum, maximum)
                    .expect("AdjustingSchedule params"),
            )
        }
        "Interpolate" => {
            let base = build_schedule(&inner_spec(params, "base"));
            let probe = build_schedule(&inner_spec(params, "probe"));
            let interval = require_f64(params, "interval");
            let probe_duration = require_f64(params, "probe_duration");
            let first_probe_at = optional_f64(params, "first_probe_at");
            Box::new(
                Interpolate::new(base, probe, interval, probe_duration, first_probe_at)
                    .expect("Interpolate params"),
            )
        }
        "Overlay" => {
            let base = build_schedule(&inner_spec(params, "base"));
            let punisher = build_schedule(&inner_spec(params, "punisher"));
            Box::new(Overlay::new(base, punisher).expect("Overlay params"))
        }
        "Percentile" => {
            let target = match require_str(params, "target") {
                "irt" => PercentileTarget::Irt,
                other => panic!("unsupported Percentile target: {other}"),
            };
            let direction = match params
                .get("direction")
                .and_then(|v| v.as_str())
                .unwrap_or("above")
            {
                "above" => PercentileDirection::Above,
                "below" => PercentileDirection::Below,
                other => panic!("unsupported Percentile direction: {other}"),
            };
            let rank = u8::try_from(require_u64(params, "rank")).expect("Percentile rank fits u8");
            let window = require_u64(params, "window") as usize;
            Box::new(Percentile::new(target, rank, window, direction).expect("Percentile params"))
        }
        "SecondOrder" => {
            let overall = build_schedule(&inner_spec(params, "overall"));
            let unit = build_schedule(&inner_spec(params, "unit"));
            Box::new(SecondOrder::new(overall, unit))
        }
        "Sidman" => {
            let ssi = require_f64(params, "shock_shock_interval");
            let rsi = require_f64(params, "response_shock_interval");
            let magnitude = optional_f64(params, "shock_magnitude").unwrap_or(1.0);
            Box::new(Sidman::new(ssi, rsi, magnitude).expect("Sidman params"))
        }
        "DiscriminatedAvoidance" => {
            let warning_duration = require_f64(params, "warning_duration");
            let iti = require_f64(params, "iti");
            let magnitude = optional_f64(params, "shock_magnitude").unwrap_or(1.0);
            Box::new(
                DiscriminatedAvoidance::new(warning_duration, iti, magnitude)
                    .expect("DiscriminatedAvoidance params"),
            )
        }
        "Escape" => {
            let trial_duration = require_f64(params, "trial_duration");
            let iti = require_f64(params, "iti");
            let magnitude = optional_f64(params, "shock_magnitude").unwrap_or(1.0);
            Box::new(Escape::new(trial_duration, iti, magnitude).expect("Escape params"))
        }
        "MatchingToSample" => {
            let n_comparisons = u32::try_from(require_u64(params, "n_comparisons"))
                .expect("n_comparisons fits u32");
            let sample_duration = optional_f64(params, "sample_duration").unwrap_or(0.0);
            let choice_timeout = optional_f64(params, "choice_timeout").unwrap_or(5.0);
            let consequence = optional_schedule(params, "consequence");
            let incorrect = optional_schedule(params, "incorrect");
            let iti = optional_f64(params, "iti").unwrap_or(1.0);
            let seed = optional_u64(params, "seed");
            Box::new(
                MatchingToSample::new(
                    n_comparisons,
                    sample_duration,
                    choice_timeout,
                    consequence,
                    incorrect,
                    iti,
                    seed,
                )
                .expect("MatchingToSample params"),
            )
        }
        "GoNoGo" => {
            let go_probability = optional_f64(params, "go_probability").unwrap_or(0.5);
            let response_window = optional_f64(params, "response_window").unwrap_or(5.0);
            let iti = optional_f64(params, "iti").unwrap_or(5.0);
            let correct_go = optional_schedule(params, "correct_go_schedule");
            let correct_nogo = optional_schedule(params, "correct_nogo_schedule");
            let false_alarm = optional_schedule(params, "false_alarm");
            let seed = optional_u64(params, "seed");
            Box::new(
                GoNoGo::new(
                    go_probability,
                    response_window,
                    iti,
                    correct_go,
                    correct_nogo,
                    false_alarm,
                    seed,
                )
                .expect("GoNoGo params"),
            )
        }
        other => panic!("unsupported schedule type in fixture: {other}"),
    }
}

/// Build a `LimitedHold` with a concrete inner type. `LimitedHold<S>` is
/// generic over `S: ArmableSchedule`, so we dispatch on the inner spec's
/// type tag here. The conformance corpus currently wraps only `FI`; new
/// inner variants can be added as the corpus grows.
fn build_limited_hold(inner: &ScheduleSpec, hold: f64) -> Box<dyn Schedule> {
    match inner.ty.as_str() {
        "FI" => {
            let interval = require_f64(&inner.params, "interval");
            let fi = FI::new(interval).expect("inner FI params");
            Box::new(LimitedHold::new(fi, hold).expect("LimitedHold params"))
        }
        "VI" => {
            let mean = require_f64(&inner.params, "mean_interval");
            let n_intervals = optional_usize(&inner.params, "n_intervals").unwrap_or(12);
            let seed = optional_u64(&inner.params, "seed");
            let vi = VI::new(mean, n_intervals, seed).expect("inner VI params");
            Box::new(LimitedHold::new(vi, hold).expect("LimitedHold params"))
        }
        "RI" => {
            let mean = require_f64(&inner.params, "mean_interval");
            let seed = optional_u64(&inner.params, "seed");
            let ri = RI::new(mean, seed).expect("inner RI params");
            Box::new(LimitedHold::new(ri, hold).expect("LimitedHold params"))
        }
        other => panic!("LimitedHold inner type not supported in conformance: {other}"),
    }
}

fn build_component_list(params: &serde_json::Value) -> Vec<Box<dyn Schedule>> {
    params
        .get("components")
        .and_then(|v| v.as_array())
        .expect("expected components array")
        .iter()
        .map(|v| {
            let cspec: ScheduleSpec =
                serde_json::from_value(v.clone()).expect("component must be a ScheduleSpec");
            build_schedule(&cspec)
        })
        .collect()
}

fn inner_spec(params: &serde_json::Value, key: &str) -> ScheduleSpec {
    let v = params
        .get(key)
        .unwrap_or_else(|| panic!("missing inner spec key: {key}"));
    serde_json::from_value(v.clone())
        .unwrap_or_else(|e| panic!("inner spec {key} deserialize error: {e}"))
}

fn require_f64(params: &serde_json::Value, key: &str) -> f64 {
    params
        .get(key)
        .and_then(|v| v.as_f64())
        .unwrap_or_else(|| panic!("missing or non-numeric param: {key}"))
}

fn require_u64(params: &serde_json::Value, key: &str) -> u64 {
    params
        .get(key)
        .and_then(|v| v.as_u64())
        .unwrap_or_else(|| panic!("missing or non-integer param: {key}"))
}

fn require_str<'a>(params: &'a serde_json::Value, key: &str) -> &'a str {
    params
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("missing or non-string param: {key}"))
}

fn optional_bool(params: &serde_json::Value, key: &str) -> Option<bool> {
    params.get(key).and_then(|v| v.as_bool())
}

/// A nested schedule spec that may be absent or JSON `null` (both map
/// to `None`, matching `_optional_spec` on the Python side).
fn optional_schedule(params: &serde_json::Value, key: &str) -> Option<Box<dyn Schedule>> {
    match params.get(key) {
        None | Some(serde_json::Value::Null) => None,
        Some(v) => {
            let spec: ScheduleSpec = serde_json::from_value(v.clone())
                .unwrap_or_else(|e| panic!("nested spec {key} deserialize error: {e}"));
            Some(build_schedule(&spec))
        }
    }
}

fn optional_f64(params: &serde_json::Value, key: &str) -> Option<f64> {
    params.get(key).and_then(|v| v.as_f64())
}

fn optional_u64(params: &serde_json::Value, key: &str) -> Option<u64> {
    params.get(key).and_then(|v| v.as_u64())
}

fn optional_usize(params: &serde_json::Value, key: &str) -> Option<usize> {
    optional_u64(params, key).map(|x| x as usize)
}

fn optional_str_vec(params: &serde_json::Value, key: &str) -> Option<Vec<String>> {
    params.get(key).and_then(|v| v.as_array()).map(|arr| {
        arr.iter()
            .filter_map(|x| x.as_str().map(|s| s.to_string()))
            .collect()
    })
}

// -----------------------------------------------------------------------------
// Replay helpers
// -----------------------------------------------------------------------------

/// Float tolerance mirroring the Python conformance README (1e-9).
const FLOAT_TOL: f64 = 1e-9;

fn approx_eq(a: f64, b: f64) -> bool {
    (a - b).abs() <= FLOAT_TOL
}

/// Strict replay: bit-equivalent match on `reinforced` and reinforcer
/// fields (ignoring `meta`, which the Python generator does not
/// serialise for this corpus).
fn run_fixture(relative: &str) {
    let fixture = load_fixture(relative);
    let mut schedule = build_schedule(&fixture.schedule);
    for (i, step) in fixture.steps.iter().enumerate() {
        let outcome: Outcome = schedule
            .step(step.now, step.event.as_ref())
            .unwrap_or_else(|e| panic!("{} step {}: schedule.step raised error: {e}", relative, i));
        assert_eq!(
            outcome.reinforced, step.expect.reinforced,
            "{} step {}: reinforced mismatch (now={}, event={:?})",
            relative, i, step.now, step.event
        );
        match (&outcome.reinforcer, &step.expect.reinforcer) {
            (None, None) => {}
            (Some(actual), Some(expected)) => {
                assert!(
                    approx_eq(actual.time, expected.time),
                    "{} step {}: reinforcer.time mismatch: {} vs {}",
                    relative,
                    i,
                    actual.time,
                    expected.time
                );
                assert!(
                    approx_eq(actual.magnitude, expected.magnitude),
                    "{} step {}: reinforcer.magnitude mismatch: {} vs {}",
                    relative,
                    i,
                    actual.magnitude,
                    expected.magnitude
                );
                assert_eq!(
                    actual.label, expected.label,
                    "{} step {}: reinforcer.label mismatch",
                    relative, i
                );
            }
            (actual, expected) => panic!(
                "{} step {}: reinforcer shape mismatch: {:?} vs {:?}",
                relative, i, actual, expected
            ),
        }
    }
}

// -----------------------------------------------------------------------------
// Deterministic fixtures — strict bit-equivalent replay
// -----------------------------------------------------------------------------

#[test]
fn atomic_fr_basic() {
    run_fixture("atomic/fr_basic.json");
}

#[test]
fn atomic_crf_basic() {
    run_fixture("atomic/crf_basic.json");
}

#[test]
fn atomic_ext_basic() {
    run_fixture("atomic/ext_basic.json");
}

#[test]
fn atomic_fi_basic() {
    run_fixture("atomic/fi_basic.json");
}

#[test]
fn atomic_ft_basic() {
    run_fixture("atomic/ft_basic.json");
}

#[test]
fn atomic_limited_hold_fi() {
    run_fixture("atomic/limited_hold_fi.json");
}

#[test]
fn compound_concurrent_cod() {
    run_fixture("compound/concurrent_cod.json");
}

#[test]
fn compound_concurrent_cod_tick() {
    run_fixture("compound/concurrent_cod_tick.json");
}

#[test]
fn compound_chained_fr2_fr3() {
    run_fixture("compound/chained_fr2_fr3.json");
}

#[test]
fn compound_chained_fr2_fi5() {
    run_fixture("compound/chained_fr2_fi5.json");
}

#[test]
fn compound_alternative_fr_ft() {
    run_fixture("compound/alternative_fr_ft.json");
}

#[test]
fn compound_multiple_fr_fr() {
    run_fixture("compound/multiple_fr_fr.json");
}

#[test]
fn compound_tandem_fr_fr() {
    run_fixture("compound/tandem_fr_fr.json");
}

#[test]
fn differential_dro_resetting() {
    run_fixture("differential/dro_resetting.json");
}

#[test]
fn differential_dro_momentary() {
    run_fixture("differential/dro_momentary.json");
}

#[test]
fn differential_drl_basic() {
    run_fixture("differential/drl_basic.json");
}

#[test]
fn differential_drh_basic() {
    run_fixture("differential/drh_basic.json");
}

#[test]
fn progressive_pr_arithmetic() {
    run_fixture("progressive/pr_arithmetic.json");
}

#[test]
fn progressive_interlocking_r4_t8() {
    run_fixture("progressive/interlocking_r4_t8.json");
}

#[test]
fn progressive_adjusting_ratio_s2_max3() {
    run_fixture("progressive/adjusting_ratio_s2_max3.json");
}

#[test]
fn compound_mixed_fr2_fr3() {
    run_fixture("compound/mixed_fr2_fr3.json");
}

#[test]
fn compound_conjunctive_fr3_fi5() {
    run_fixture("compound/conjunctive_fr3_fi5.json");
}

#[test]
fn compound_interpolate_fr2_ext() {
    run_fixture("compound/interpolate_fr2_ext.json");
}

#[test]
fn compound_overlay_fr2_fr3() {
    run_fixture("compound/overlay_fr2_fr3.json");
}

#[test]
fn compound_second_order_fr2_fr3() {
    run_fixture("compound/second_order_fr2_fr3.json");
}

#[test]
fn differential_percentile_irt_r75_w3() {
    run_fixture("differential/percentile_irt_r75_w3.json");
}

#[test]
fn wrapper_timeout_fr2_to3() {
    run_fixture("wrapper/timeout_fr2_to3.json");
}

#[test]
fn wrapper_response_cost_fr2() {
    run_fixture("wrapper/response_cost_fr2.json");
}

#[test]
fn aversive_sidman_ssi5_rsi3() {
    run_fixture("aversive/sidman_ssi5_rsi3.json");
}

#[test]
fn aversive_discriminated_avoidance_w3_iti4() {
    run_fixture("aversive/discriminated_avoidance_w3_iti4.json");
}

#[test]
fn aversive_escape_trial3_iti2() {
    run_fixture("aversive/escape_trial3_iti2.json");
}

// -----------------------------------------------------------------------------
// Seeded stochastic fixtures — strict bit-equivalent replay
// -----------------------------------------------------------------------------
//
// The seeded schedules (VR, VI, VT, RR, RI, RT, MatchingToSample,
// GoNoGo) draw from `PyRandom`, a port of CPython's `random.Random`
// (MT19937 seeded through `init_by_array`, `random()` = genrand_res53,
// `_randbelow` by rejection on `getrandbits`, `expovariate` as
// `-log(1 - random()) / lambd`). The same integer seed therefore yields
// the same draw stream as the Python generator, and these fixtures are
// replayed exactly like the deterministic ones.

#[test]
fn atomic_vr_seeded_42() {
    run_fixture("atomic/vr_seeded_42.json");
}

#[test]
fn atomic_vi_seeded_7() {
    run_fixture("atomic/vi_seeded_7.json");
}

#[test]
fn atomic_vt_seeded_3() {
    run_fixture("atomic/vt_seeded_3.json");
}

#[test]
fn atomic_rr_seeded_99() {
    run_fixture("atomic/rr_seeded_99.json");
}

#[test]
fn atomic_ri_seeded_5() {
    run_fixture("atomic/ri_seeded_5.json");
}

#[test]
fn atomic_rt_seeded_11() {
    run_fixture("atomic/rt_seeded_11.json");
}

#[test]
fn trial_mts_n2_seed7() {
    run_fixture("trial/mts_n2_seed7.json");
}

#[test]
fn trial_gonogo_p05_seed1() {
    run_fixture("trial/gonogo_p05_seed1.json");
}

// -----------------------------------------------------------------------------
// Completeness guard — no fixture may go un-replayed
// -----------------------------------------------------------------------------
//
// The parity corpus lives entirely under
// `../contingency-py/conformance/`; `contingency-py` is its single
// source of truth. This guard fails the build whenever a fixture exists
// in the corpus without a corresponding bit-equivalent
// `run_fixture(...)` replay call in this harness. It catches two drift
// modes mechanically:
//
//   1. A newly generated fixture is committed but no `run_fixture(...)`
//      test is added for it.
//   2. An existing `run_fixture(...)` call is removed or commented out.

/// Recursively collect every `*.json` fixture under `dir`, returning
/// paths relative to `root` with `/` separators (e.g.
/// `compound/multiple_fr_fr.json`).
fn collect_fixtures(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let entries = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("failed to read dir {}: {e}", dir.display()));
    for entry in entries {
        let path = entry
            .unwrap_or_else(|e| panic!("dir entry error under {}: {e}", dir.display()))
            .path();
        if path.is_dir() {
            collect_fixtures(root, &path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("json") {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            out.push(rel);
        }
    }
}

/// Parse this harness's own source and return the set of fixture paths
/// passed to `run_fixture("...")`. Commented-out calls are excluded
/// (only the code portion of each line, before any `//`, is scanned),
/// so commenting a call out makes the corresponding fixture appear
/// un-replayed — exactly the RED signal the completeness guard exists
/// to surface. The word-boundary check below rejects any
/// identifier-prefixed lookalike such as `other_run_fixture("...")`.
fn replayed_fixtures() -> HashSet<String> {
    const NEEDLE: &str = "run_fixture(\"";
    let src_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("conformance.rs");
    let src = std::fs::read_to_string(&src_path)
        .unwrap_or_else(|e| panic!("failed to read harness source {}: {e}", src_path.display()));

    let mut out = HashSet::new();
    for line in src.lines() {
        // Strip any line comment so commented-out calls do not count.
        let code = match line.find("//") {
            Some(idx) => &line[..idx],
            None => line,
        };
        let mut from = 0;
        while let Some(rel_pos) = code[from..].find(NEEDLE) {
            let start = from + rel_pos;
            // Word boundary: the char before `run_fixture(` must not be
            // part of a longer identifier (defends against any
            // `*_run_fixture("` lookalike).
            let prev_is_ident = code[..start]
                .chars()
                .next_back()
                .map(|c| c.is_alphanumeric() || c == '_')
                .unwrap_or(false);
            let arg_start = start + NEEDLE.len();
            match code[arg_start..].find('"') {
                Some(end_rel) => {
                    if !prev_is_ident {
                        out.insert(code[arg_start..arg_start + end_rel].to_string());
                    }
                    from = arg_start + end_rel + 1;
                }
                None => break,
            }
        }
    }
    out
}

/// Every fixture in the parity corpus must have a bit-equivalent
/// `run_fixture(...)` replay call. Fails listing any fixture that is
/// missing one.
#[test]
fn conformance_corpus_completeness() {
    let dir = conformance_dir();
    let mut fixtures = Vec::new();
    collect_fixtures(&dir, &dir, &mut fixtures);
    fixtures.sort();
    assert!(
        !fixtures.is_empty(),
        "no fixtures discovered under {} — corpus path resolution is broken",
        dir.display()
    );

    let covered = replayed_fixtures();

    let missing: Vec<&String> = fixtures
        .iter()
        .filter(|rel| !covered.contains(*rel))
        .collect();

    assert!(
        missing.is_empty(),
        "conformance fixtures with no bit-equivalent run_fixture(...) \
         replay call: {missing:?}\n\
         Every fixture under {} must be replayed. Add a \
         `run_fixture(\"<relative-path>\")` test for each.",
        dir.display()
    );
}

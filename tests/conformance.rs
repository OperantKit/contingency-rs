//! Cross-language conformance replay tests.
//!
//! Loads each JSON fixture under `../contingency-py/conformance/` and
//! replays its event sequence against the Rust `contingency` runtime.
//!
//! Deterministic fixtures (FR, FI, FT, CRF, EXT, LimitedHold-wrapping-FI,
//! Concurrent with COD, Chained, Alternative with FR+FT, DRO resetting
//! and momentary, DRL, DRH, ProgressiveRatio arithmetic) are asserted
//! **bit-equivalent** against Python observed outcomes: both
//! `Outcome.reinforced` and (when reinforced) every field of the
//! `Reinforcer` must match within a 1e-9 float tolerance.
//!
//! Stochastic fixtures (seeded VR, VI, RI, RR, VT, RT) cannot be
//! bit-reproduced because Python's Mersenne Twister and Rust's
//! `SmallRng` are different PRNGs. The Python conformance README labels
//! these as "trajectory templates." For Rust, the stochastic fixture
//! tests are marked `#[ignore]` with an explicit relaxation rationale;
//! see each test's doc comment.
//!
//! # References
//!
//! Fleshler, M., & Hoffman, H. S. (1962). A progression for generating
//! variable-interval schedules. *Journal of the Experimental Analysis
//! of Behavior*, 5(4), 529-530. <https://doi.org/10.1901/jeab.1962.5-529>

use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use serde::Deserialize;

use contingency::schedules::{
    arithmetic, crf, geometric, richardson_roberts, Alternative, Chained, Concurrent, DroMode,
    LimitedHold, Multiple, ProgressiveRatio, Tandem, DRH, DRL, DRO, EXT, FI, FR, FT, RI, RR, RT,
    VI, VR, VT,
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
            let cor = optional_u64(params, "cor").unwrap_or(0) as u32;
            Box::new(Concurrent::new(components, cod, cor).expect("Concurrent params"))
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

/// Relaxed replay for stochastic fixtures: does **not** require bit
/// equivalence with the Python-recorded trajectory, because Rust's
/// `SmallRng` differs from Python's Mersenne Twister.
///
/// Instead, verifies that:
/// 1. The schedule replays without raising an error.
/// 2. The total number of reinforcers produced by Rust is within a
///    tolerance band around the Python-recorded count (50%).
/// 3. For interval / time families, no reinforcer is emitted before the
///    first step's `now` (causality sanity).
fn run_stochastic_fixture(relative: &str) {
    let fixture = load_fixture(relative);
    let mut schedule = build_schedule(&fixture.schedule);
    let mut rust_count = 0usize;
    let mut first_rein_time: Option<f64> = None;
    for (i, step) in fixture.steps.iter().enumerate() {
        let outcome = schedule
            .step(step.now, step.event.as_ref())
            .unwrap_or_else(|e| panic!("{} step {}: schedule.step raised: {e}", relative, i));
        if outcome.reinforced {
            rust_count += 1;
            if first_rein_time.is_none() {
                first_rein_time = outcome.reinforcer.as_ref().map(|r| r.time);
            }
        }
    }
    let expected_count = fixture.steps.iter().filter(|s| s.expect.reinforced).count();
    // Loose tolerance: PRNG trajectories differ, so only assert the
    // order of magnitude matches. 50% band around the Python count.
    let lower = (expected_count as f64 * 0.5).floor() as usize;
    let upper = (expected_count as f64 * 1.5).ceil() as usize + 1;
    assert!(
        rust_count >= lower && rust_count <= upper,
        "{}: rust reinforcer count {} outside [{}, {}] (python recorded {})",
        relative,
        rust_count,
        lower,
        upper,
        expected_count
    );
    if let (Some(first), Some(first_step)) = (first_rein_time, fixture.steps.first()) {
        assert!(
            first >= first_step.now - FLOAT_TOL,
            "{}: first reinforcer time {} precedes first step now {}",
            relative,
            first,
            first_step.now
        );
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
fn compound_chained_fr2_fr3() {
    run_fixture("compound/chained_fr2_fr3.json");
}

#[test]
fn compound_alternative_fr_ft() {
    run_fixture("compound/alternative_fr_ft.json");
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

// -----------------------------------------------------------------------------
// Stochastic fixtures — ignored by default
// -----------------------------------------------------------------------------
//
// Python's `random.Random` is a Mersenne Twister; Rust's `SmallRng` is a
// different PRNG. Even under the same integer seed these produce
// different Bernoulli / exponential draw sequences, so bit-equivalent
// replay is impossible without serialising the draw pool itself. These
// tests are run via `cargo test -- --ignored` as *relaxed* structural
// checks only: they assert the Rust schedule runs without error and
// that the number of reinforcements is in the same order of magnitude
// as the Python-recorded count (50% tolerance band).

#[test]
#[ignore = "stochastic: PRNG differs between Python MT and Rust SmallRng"]
fn atomic_vr_seeded_42() {
    run_stochastic_fixture("atomic/vr_seeded_42.json");
}

#[test]
#[ignore = "stochastic: PRNG differs between Python MT and Rust SmallRng"]
fn atomic_vi_seeded_7() {
    run_stochastic_fixture("atomic/vi_seeded_7.json");
}

#[test]
#[ignore = "stochastic: PRNG differs between Python MT and Rust SmallRng"]
fn atomic_vt_seeded_3() {
    run_stochastic_fixture("atomic/vt_seeded_3.json");
}

#[test]
#[ignore = "stochastic: PRNG differs between Python MT and Rust SmallRng"]
fn atomic_rr_seeded_99() {
    run_stochastic_fixture("atomic/rr_seeded_99.json");
}

#[test]
#[ignore = "stochastic: PRNG differs between Python MT and Rust SmallRng"]
fn atomic_ri_seeded_5() {
    run_stochastic_fixture("atomic/ri_seeded_5.json");
}

#[test]
#[ignore = "stochastic: PRNG differs between Python MT and Rust SmallRng"]
fn atomic_rt_seeded_11() {
    run_stochastic_fixture("atomic/rt_seeded_11.json");
}

//! Cross-language conformance replay tests.
//!
//! Loads each JSON fixture under `../contingency-py/conformance/` and
//! replays its event sequence against the Rust `contingency` runtime.
//! Each fixture's `"schedule"` object is built with the public
//! [`contingency::spec::from_spec`] entry point.
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

use serde::Deserialize;

use contingency::spec::from_spec;
use contingency::{Outcome, Reinforcer, ResponseEvent};

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
    schedule: serde_json::Value,
    steps: Vec<StepSpec>,
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
    let mut schedule = from_spec(&fixture.schedule)
        .unwrap_or_else(|e| panic!("{relative}: from_spec failed: {e}"));
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

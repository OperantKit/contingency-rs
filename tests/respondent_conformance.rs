//! Respondent Tier A conformance replay tests.
//!
//! Loads each JSON case under `../contingency-py/conformance/respondent/`
//! and checks that [`contingency::respondent::expand`] turns the case's
//! `spec` and `options` into its `expected` trial list, or into the same
//! `unresolved` error. The cases are written by contingency-py, whose
//! respondent execution contract defines the trial shape and the random
//! draws; this harness only replays them.
//!
//! Set `CONTINGENCY_RESPONDENT_CONFORMANCE_DIR` to read the cases from
//! another directory (for example a checkout of a different branch).
//! A missing directory is a failure, not a skip: a case that is never
//! read must not count as passing.
//!
//! Numbers are compared within 1e-9, the tolerance of the schedule
//! conformance corpus; every other value must be equal.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde_json::Value;

use contingency::respondent::{expand, ExpandError, ExpandOptions};

const FLOAT_TOL: f64 = 1e-9;

fn respondent_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("CONTINGENCY_RESPONDENT_CONFORMANCE_DIR") {
        return PathBuf::from(dir);
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("contingency-py")
        .join("conformance")
        .join("respondent")
}

fn load_case(name: &str) -> Value {
    let path = respondent_dir().join(name);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "failed to read respondent case {}: {e} \
             (set CONTINGENCY_RESPONDENT_CONFORMANCE_DIR to the corpus)",
            path.display()
        )
    });
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("failed to parse respondent case {}: {e}", path.display()))
}

/// Structural equality with numbers compared within [`FLOAT_TOL`].
fn same(expected: &Value, observed: &Value, at: &str) -> Result<(), String> {
    match (expected, observed) {
        (Value::Number(a), Value::Number(b)) => {
            let (a, b) = (a.as_f64().unwrap(), b.as_f64().unwrap());
            if (a - b).abs() <= FLOAT_TOL {
                Ok(())
            } else {
                Err(format!("{at}: expected {a}, got {b}"))
            }
        }
        (Value::Array(a), Value::Array(b)) => {
            if a.len() != b.len() {
                return Err(format!("{at}: expected {} items, got {}", a.len(), b.len()));
            }
            for (i, (x, y)) in a.iter().zip(b).enumerate() {
                same(x, y, &format!("{at}[{i}]"))?;
            }
            Ok(())
        }
        (Value::Object(a), Value::Object(b)) => {
            let ka: HashSet<&String> = a.keys().collect();
            let kb: HashSet<&String> = b.keys().collect();
            if ka != kb {
                return Err(format!("{at}: expected keys {ka:?}, got {kb:?}"));
            }
            for (k, x) in a {
                same(x, &b[k], &format!("{at}.{k}"))?;
            }
            Ok(())
        }
        _ if expected == observed => Ok(()),
        _ => Err(format!("{at}: expected {expected}, got {observed}")),
    }
}

fn run_case(name: &str) {
    let case = load_case(name);
    let options = ExpandOptions::from_value(&case["options"])
        .unwrap_or_else(|e| panic!("{name}: options not accepted: {e}"));
    let observed = match expand(&case["spec"], &options) {
        Ok(trials) => serde_json::json!({ "trials": trials }),
        Err(ExpandError::Unresolved(missing)) => {
            serde_json::json!({ "error": "unresolved", "missing": missing })
        }
        Err(e) => panic!("{name}: expansion failed: {e}"),
    };
    if let Err(msg) = same(&case["expected"], &observed, "expected") {
        panic!("{name}: {msg}");
    }
}

#[test]
fn pair_forward_delay() {
    run_case("pair_forward_delay.json");
}

#[test]
fn pair_forward_delay_compound_cs() {
    run_case("pair_forward_delay_compound_cs.json");
}

#[test]
fn pair_forward_trace() {
    run_case("pair_forward_trace.json");
}

#[test]
fn pair_forward_trace_with_cs_duration() {
    run_case("pair_forward_trace_with_cs_duration.json");
}

#[test]
fn pair_simultaneous() {
    run_case("pair_simultaneous.json");
}

#[test]
fn pair_backward() {
    run_case("pair_backward.json");
}

#[test]
fn extinction() {
    run_case("extinction.json");
}

#[test]
fn cs_only() {
    run_case("cs_only.json");
}

#[test]
fn us_only() {
    run_case("us_only.json");
}

#[test]
fn contingency_positive() {
    run_case("contingency_positive.json");
}

#[test]
fn contingency_negative() {
    run_case("contingency_negative.json");
}

#[test]
fn truly_random() {
    run_case("truly_random.json");
}

#[test]
fn truly_random_without_p() {
    run_case("truly_random_without_p.json");
}

#[test]
fn explicitly_unpaired() {
    run_case("explicitly_unpaired.json");
}

#[test]
fn compound() {
    run_case("compound.json");
}

#[test]
fn serial() {
    run_case("serial.json");
}

#[test]
fn iti_fixed() {
    run_case("iti_fixed.json");
}

#[test]
fn iti_uniform_minutes() {
    run_case("iti_uniform_minutes.json");
}

#[test]
fn iti_exponential() {
    run_case("iti_exponential.json");
}

#[test]
fn differential() {
    run_case("differential.json");
}

#[test]
fn differential_short_form_without_us() {
    run_case("differential_short_form_without_us.json");
}

/// Case files named in `run_case("...")` calls of this source, with
/// commented-out calls excluded.
fn replayed_cases() -> HashSet<String> {
    const NEEDLE: &str = "run_case(\"";
    let src_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("respondent_conformance.rs");
    let src = std::fs::read_to_string(&src_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", src_path.display()));
    let mut out = HashSet::new();
    for line in src.lines() {
        let code = line.split("//").next().unwrap_or("");
        let mut rest = code;
        while let Some(pos) = rest.find(NEEDLE) {
            let prev_is_ident = rest[..pos]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_');
            let after = &rest[pos + NEEDLE.len()..];
            let Some(end) = after.find('"') else { break };
            if !prev_is_ident {
                out.insert(after[..end].to_string());
            }
            rest = &after[end + 1..];
        }
    }
    out
}

/// Every case in the respondent corpus has a `run_case(...)` call.
#[test]
fn respondent_corpus_completeness() {
    let dir = respondent_dir();
    let entries = std::fs::read_dir(&dir).unwrap_or_else(|e| {
        panic!(
            "failed to read the respondent corpus {}: {e} \
             (set CONTINGENCY_RESPONDENT_CONFORMANCE_DIR to the corpus)",
            dir.display()
        )
    });
    let mut cases: Vec<String> = entries
        .map(|e| e.expect("dir entry").path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("json"))
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    cases.sort();
    assert!(!cases.is_empty(), "no cases under {}", dir.display());
    let covered = replayed_cases();
    let missing: Vec<&String> = cases.iter().filter(|c| !covered.contains(*c)).collect();
    assert!(
        missing.is_empty(),
        "respondent cases with no run_case(...) call: {missing:?}"
    );
}

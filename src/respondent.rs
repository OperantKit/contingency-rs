//! Respondent (Pavlovian) Tier A expansion.
//!
//! [`expand`] turns one Tier A respondent expression — a node of the
//! contingency-dsl respondent AST given as JSON, the same way
//! [`crate::from_spec`] takes an operant schedule — into the list of
//! trials it stands for: which CSs, which US, and when. The trial shape,
//! the reading of each element and the random draws follow the respondent
//! execution contract of contingency-py, and the cases in its
//! `conformance/respondent/` directory pin the output.
//!
//! Tier A is the respondent layer of contingency-dsl:
//! `PairForwardDelay`, `PairForwardTrace`, `PairSimultaneous`,
//! `PairBackward`, `Extinction`, `CSOnly`, `USOnly`, `Contingency`,
//! `TrulyRandom`, `ExplicitlyUnpaired`, `Compound`, `Serial`, `ITI` and
//! `Differential`. Procedures built from them (Tier B) are expanded by
//! their own extension packages, not here.
//!
//! Values the expression leaves to the program (`trials`, `seed`,
//! `labels.cs`, `labels.us`) come from [`ExpandOptions`]. A value that an
//! element needs and nobody supplies is [`ExpandError::Unresolved`];
//! the expansion never chooses a default for it.
//!
//! # Random draws
//!
//! Each call starts a new [`PyRandom`] from `|seed|`, the same stream as
//! CPython's `random.Random(seed)`. A draw is taken for every period even
//! when its probability is 0 or 1.

#[allow(unused_imports)]
use crate::prelude::*;

use serde::Serialize;
use serde_json::{Map, Value};

use crate::helpers::py_random::PyRandom;

/// Why an expression could not be expanded.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExpandError {
    /// A value the element needs is supplied neither by the expression nor
    /// by the options. The payload names it: an option name (`trials`,
    /// `seed`, `labels.cs`, `labels.us`) or the element's own argument
    /// (such as `p`).
    #[error("unresolved: {0}")]
    Unresolved(String),
    /// The expression is not a valid Tier A node.
    #[error("invalid respondent spec: {0}")]
    Invalid(String),
}

type Result<T> = core::result::Result<T, ExpandError>;

fn invalid(msg: String) -> ExpandError {
    ExpandError::Invalid(msg)
}

/// CS and US labels the program supplies (from `@cs` / `@us`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Labels {
    /// Label of the CS, used by `Contingency`.
    pub cs: Option<String>,
    /// Label of the US, used by `Contingency` and the short form of
    /// `Differential`.
    pub us: Option<String>,
}

/// Values the expression leaves to the program.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExpandOptions {
    /// Trial count for elements without their own `trials` argument.
    pub trials: Option<u64>,
    /// Seed for `Contingency`, `TrulyRandom` and `ITI(uniform/exponential)`.
    pub seed: Option<i64>,
    /// CS and US labels.
    pub labels: Labels,
}

impl ExpandOptions {
    /// Read options from a JSON object
    /// `{"trials": int, "seed": int, "labels": {"cs": str, "us": str}}`.
    /// Every key is optional; `null` counts as absent.
    pub fn from_value(value: &Value) -> Result<Self> {
        let obj = match value {
            Value::Null => return Ok(Self::default()),
            Value::Object(m) => m,
            other => return Err(invalid(format!("options must be an object, got {other}"))),
        };
        let trials = match obj.get("trials") {
            None | Some(Value::Null) => None,
            Some(v) => Some(count(v, "options.trials")?),
        };
        let seed = match obj.get("seed") {
            None | Some(Value::Null) => None,
            Some(v) => Some(
                v.as_i64()
                    .ok_or_else(|| invalid(format!("options.seed must be an integer, got {v}")))?,
            ),
        };
        let labels = match obj.get("labels") {
            None | Some(Value::Null) => Labels::default(),
            Some(Value::Object(m)) => Labels {
                cs: opt_str(m, "cs", "options.labels.cs")?,
                us: opt_str(m, "us", "options.labels.us")?,
            },
            Some(other) => {
                return Err(invalid(format!(
                    "options.labels must be an object, got {other}"
                )))
            }
        };
        Ok(Self {
            trials,
            seed,
            labels,
        })
    }
}

/// The interval between two times of a trial whose absolute values are
/// open.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Gap {
    /// Start point: `cs_off`, `us_off` or `any_cs` (every CS in the list).
    pub from: &'static str,
    /// End point: `us_on` or `cs_on`.
    pub to: &'static str,
    /// Interval in seconds.
    pub s: f64,
    /// `true` when the interval is a lower bound.
    pub at_least: bool,
}

/// Times within a trial, in seconds from the trial start; `None` when the
/// expression does not fix them.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct TrialTime {
    /// CS onset.
    pub cs_on: Option<f64>,
    /// CS offset.
    pub cs_off: Option<f64>,
    /// US onset.
    pub us_on: Option<f64>,
    /// US offset.
    pub us_off: Option<f64>,
    /// Interval fixed between two open times.
    pub gap: Option<Gap>,
}

/// One trial (or period) of a respondent expansion.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Trial {
    /// Stage name; `"main"` for a bare Tier A expression.
    pub stage: String,
    /// CS labels presented, sorted.
    pub cs: Vec<String>,
    /// US label, or `None` when no US is delivered.
    pub us: Option<String>,
    /// Context label; always `None` for Tier A.
    pub context: Option<String>,
    /// Presentation order of the CSs for `Serial`.
    pub order: Option<Vec<String>>,
    /// Times within the trial.
    pub time: TrialTime,
    /// Interval before this trial, set only by `ITI`.
    pub iti_s: Option<f64>,
}

fn trial(mut cs: Vec<String>, us: Option<String>, time: TrialTime) -> Trial {
    cs.sort();
    Trial {
        stage: "main".to_string(),
        cs,
        us,
        context: None,
        order: None,
        time,
        iti_s: None,
    }
}

fn at_cs(cs_on: f64) -> TrialTime {
    TrialTime {
        cs_on: Some(cs_on),
        ..TrialTime::default()
    }
}

fn at_us(us_on: f64) -> TrialTime {
    TrialTime {
        us_on: Some(us_on),
        ..TrialTime::default()
    }
}

// ---------------------------------------------------------------------------
// Reading the spec
// ---------------------------------------------------------------------------

fn count(v: &Value, what: &str) -> Result<u64> {
    v.as_u64()
        .ok_or_else(|| invalid(format!("{what} must be a non-negative integer, got {v}")))
}

fn opt_str(m: &Map<String, Value>, key: &str, what: &str) -> Result<Option<String>> {
    match m.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(other) => Err(invalid(format!("{what} must be a string, got {other}"))),
    }
}

fn req_str(m: &Map<String, Value>, ty: &str, key: &str) -> Result<String> {
    opt_str(m, key, &format!("{ty}.{key}"))?.ok_or_else(|| invalid(format!("{ty} needs `{key}`")))
}

fn req_f64(m: &Map<String, Value>, ty: &str, key: &str) -> Result<f64> {
    m.get(key)
        .and_then(Value::as_f64)
        .ok_or_else(|| invalid(format!("{ty} needs a number `{key}`")))
}

/// A duration `{"value": x, "unit": "s"}` (or `time_unit`) in seconds.
fn seconds(m: &Map<String, Value>, ty: &str, key: &str) -> Result<Option<f64>> {
    let d = match m.get(key) {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::Object(d)) => d,
        Some(other) => {
            return Err(invalid(format!(
                "{ty}.{key} must be a duration, got {other}"
            )))
        }
    };
    let unit = d
        .get("unit")
        .or_else(|| d.get("time_unit"))
        .and_then(Value::as_str);
    let scale = match unit {
        Some("s") => 1.0,
        Some("ms") => 0.001,
        Some("min") => 60.0,
        Some("h") => 3600.0,
        _ => return Err(invalid(format!("{ty}.{key}: duration unit not understood"))),
    };
    let value = d
        .get("value")
        .and_then(Value::as_f64)
        .ok_or_else(|| invalid(format!("{ty}.{key} needs a number `value`")))?;
    Ok(Some(value * scale))
}

fn req_seconds(m: &Map<String, Value>, ty: &str, key: &str) -> Result<f64> {
    seconds(m, ty, key)?.ok_or_else(|| invalid(format!("{ty} needs `{key}`")))
}

fn str_list(m: &Map<String, Value>, ty: &str, key: &str) -> Result<Vec<String>> {
    let items = m
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| invalid(format!("{ty} needs a list `{key}`")))?;
    items
        .iter()
        .map(|v| {
            v.as_str()
                .map(str::to_string)
                .ok_or_else(|| invalid(format!("{ty}.{key} must hold strings, got {v}")))
        })
        .collect()
}

/// CSs of a simultaneous `Compound` node.
fn compound_cs(m: &Map<String, Value>) -> Result<Vec<String>> {
    match m.get("mode") {
        None | Some(Value::Null) => {}
        Some(Value::String(s)) if s == "Simultaneous" => {}
        Some(other) => return Err(invalid(format!("Compound mode is not Tier A: {other}"))),
    }
    str_list(m, "Compound", "cs_list")
}

/// The CS argument of a pair: a label or a simultaneous `Compound`.
fn cs_arg(m: &Map<String, Value>, ty: &str) -> Result<Vec<String>> {
    match m.get("cs") {
        Some(Value::String(s)) => Ok(vec![s.clone()]),
        Some(Value::Object(c)) if c.get("type").and_then(Value::as_str) == Some("Compound") => {
            compound_cs(c)
        }
        other => Err(invalid(format!("{ty}.cs not understood: {other:?}"))),
    }
}

fn label(opt: &Option<String>, name: &str) -> Result<String> {
    opt.clone()
        .ok_or_else(|| ExpandError::Unresolved(name.to_string()))
}

fn draws(seed: Option<i64>, n: usize) -> Result<Vec<f64>> {
    let seed = seed.ok_or_else(|| ExpandError::Unresolved("seed".to_string()))?;
    let mut rng = PyRandom::from_seed(seed.unsigned_abs());
    Ok((0..n).map(|_| rng.random()).collect())
}

/// `n` CS periods alternating with `n` no-CS periods, CS first; one draw
/// per period in that order.
fn periods(
    cs: &str,
    us: &str,
    p_cs: f64,
    p_no_cs: f64,
    n: usize,
    seed: Option<i64>,
) -> Result<Vec<Trial>> {
    let u = draws(seed, 2 * n)?;
    let pick = |x: f64, p: f64| (x < p).then(|| us.to_string());
    let mut out = Vec::with_capacity(2 * n);
    for k in 0..n {
        out.push(trial(
            vec![cs.to_string()],
            pick(u[2 * k], p_cs),
            at_cs(0.0),
        ));
        out.push(trial(
            Vec::new(),
            pick(u[2 * k + 1], p_no_cs),
            TrialTime::default(),
        ));
    }
    Ok(out)
}

fn iti_values(m: &Map<String, Value>, n: usize, seed: Option<i64>) -> Result<Vec<f64>> {
    let mean = req_seconds(m, "ITI", "mean")?;
    match m.get("distribution").and_then(Value::as_str) {
        Some("fixed") => Ok(vec![mean; n]),
        Some("uniform") => Ok(draws(seed, n)?
            .into_iter()
            .map(|x| 2.0 * mean * x)
            .collect()),
        Some("exponential") => Ok(draws(seed, n)?
            .into_iter()
            .map(|x| -mean * (1.0 - x).ln())
            .collect()),
        _ => Err(invalid("ITI distribution not understood".to_string())),
    }
}

fn repeat(n: usize, t: Trial) -> Vec<Trial> {
    vec![t; n]
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Expand one Tier A respondent expression into its trial list.
///
/// `spec` is a contingency-dsl respondent AST node such as
/// `{"type": "PairForwardDelay", "cs": "tone", "us": "shock",
/// "isi": {"value": 10, "unit": "s"}, "cs_duration": {"value": 15, "unit": "s"}}`.
/// The trial count is the node's own `trials` when it has one, otherwise
/// `options.trials`.
///
/// # Errors
///
/// [`ExpandError::Unresolved`] when a value the element needs is missing;
/// [`ExpandError::Invalid`] when `spec` is not a Tier A node.
pub fn expand(spec: &Value, options: &ExpandOptions) -> Result<Vec<Trial>> {
    let m = spec
        .as_object()
        .ok_or_else(|| invalid(format!("spec must be an object, got {spec}")))?;
    let ty = m
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("spec needs a string `type`".to_string()))?;
    let n = match m.get("trials") {
        Some(v) if !v.is_null() => count(v, &format!("{ty}.trials"))?,
        _ => options
            .trials
            .ok_or_else(|| ExpandError::Unresolved("trials".to_string()))?,
    };
    let n = usize::try_from(n).map_err(|_| invalid(format!("{ty}.trials is too large")))?;
    let seed = options.seed;
    let us_of = |key: &str| req_str(m, ty, key).map(Some);
    match ty {
        "PairForwardDelay" => {
            let time = TrialTime {
                cs_on: Some(0.0),
                cs_off: Some(req_seconds(m, ty, "cs_duration")?),
                us_on: Some(req_seconds(m, ty, "isi")?),
                ..TrialTime::default()
            };
            Ok(repeat(n, trial(cs_arg(m, ty)?, us_of("us")?, time)))
        }
        "PairForwardTrace" => {
            let cs_off = seconds(m, ty, "cs_duration")?;
            let trace = req_seconds(m, ty, "trace_interval")?;
            let time = TrialTime {
                cs_on: Some(0.0),
                cs_off,
                us_on: cs_off.map(|c| c + trace),
                us_off: None,
                gap: Some(Gap {
                    from: "cs_off",
                    to: "us_on",
                    s: trace,
                    at_least: false,
                }),
            };
            Ok(repeat(n, trial(cs_arg(m, ty)?, us_of("us")?, time)))
        }
        "PairSimultaneous" => {
            let time = TrialTime {
                cs_on: Some(0.0),
                us_on: Some(0.0),
                ..TrialTime::default()
            };
            Ok(repeat(n, trial(cs_arg(m, ty)?, us_of("us")?, time)))
        }
        "PairBackward" => {
            let time = TrialTime {
                us_on: Some(0.0),
                gap: Some(Gap {
                    from: "us_off",
                    to: "cs_on",
                    s: req_seconds(m, ty, "isi")?,
                    at_least: false,
                }),
                ..TrialTime::default()
            };
            Ok(repeat(n, trial(cs_arg(m, ty)?, us_of("us")?, time)))
        }
        "Extinction" | "CSOnly" => Ok(repeat(n, trial(cs_arg(m, ty)?, None, at_cs(0.0)))),
        "USOnly" => Ok(repeat(n, trial(Vec::new(), us_of("us")?, at_us(0.0)))),
        "Compound" => Ok(repeat(n, trial(compound_cs(m)?, None, at_cs(0.0)))),
        "Serial" => {
            let order = str_list(m, ty, "cs_list")?;
            let time = TrialTime {
                cs_on: Some(0.0),
                gap: Some(Gap {
                    from: "cs_off",
                    to: "cs_on",
                    s: req_seconds(m, ty, "isi")?,
                    at_least: false,
                }),
                ..TrialTime::default()
            };
            let mut t = trial(order.clone(), None, time);
            t.order = Some(order);
            Ok(repeat(n, t))
        }
        "Contingency" => {
            let cs = label(&options.labels.cs, "labels.cs")?;
            let us = label(&options.labels.us, "labels.us")?;
            let p_cs = req_f64(m, ty, "p_us_given_cs")?;
            let p_no_cs = req_f64(m, ty, "p_us_given_no_cs")?;
            periods(&cs, &us, p_cs, p_no_cs, n, seed)
        }
        "TrulyRandom" => {
            let p = match m.get("p") {
                None | Some(Value::Null) => return Err(ExpandError::Unresolved("p".to_string())),
                Some(v) => v
                    .as_f64()
                    .ok_or_else(|| invalid(format!("TrulyRandom.p must be a number, got {v}")))?,
            };
            let cs = req_str(m, ty, "cs")?;
            let us = req_str(m, ty, "us")?;
            periods(&cs, &us, p, p, n, seed)
        }
        "ExplicitlyUnpaired" => {
            let cs = req_str(m, ty, "cs")?;
            let us = req_str(m, ty, "us")?;
            let gap = seconds(m, ty, "min_separation")?.map(|s| Gap {
                from: "any_cs",
                to: "us_on",
                s,
                at_least: true,
            });
            let us_time = TrialTime {
                us_on: Some(0.0),
                gap,
                ..TrialTime::default()
            };
            let mut out = Vec::with_capacity(2 * n);
            for _ in 0..n {
                out.push(trial(vec![cs.clone()], None, at_cs(0.0)));
                out.push(trial(Vec::new(), Some(us.clone()), us_time.clone()));
            }
            Ok(out)
        }
        "Differential" => {
            // An empty `us` is the short form, as in the reference reading.
            let us = match opt_str(m, "us", "Differential.us")? {
                Some(us) if !us.is_empty() => us,
                _ => label(&options.labels.us, "labels.us")?,
            };
            let plus = req_str(m, ty, "cs_positive")?;
            let minus = req_str(m, ty, "cs_negative")?;
            let mut out = Vec::with_capacity(2 * n);
            for _ in 0..n {
                out.push(trial(vec![plus.clone()], Some(us.clone()), at_cs(0.0)));
                out.push(trial(vec![minus.clone()], None, at_cs(0.0)));
            }
            Ok(out)
        }
        "ITI" => Ok(iti_values(m, n, seed)?
            .into_iter()
            .map(|v| Trial {
                iti_s: Some(v),
                ..trial(Vec::new(), None, TrialTime::default())
            })
            .collect()),
        other => Err(invalid(format!(
            "not a Tier A respondent expression: {other:?}"
        ))),
    }
}

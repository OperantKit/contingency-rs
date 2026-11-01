//! Schedule spec → runtime [`Schedule`] builder.
//!
//! A *schedule spec* is the JSON object that the conformance fixtures
//! store under `"schedule"`:
//!
//! ```json
//! {"type": "FR", "params": {"n": 5}}
//! ```
//!
//! [`from_spec`] is the single mapping from that object to a boxed
//! [`Schedule`]. It mirrors `build_schedule_from_spec` in contingency-py
//! (`contingency.conformance.spec`) and accepts the same 36 `type` tags:
//! the 35 schedule classes plus `CRF` (an alias for `FR 1`).
//!
//! # Rules
//!
//! * `type` is case-sensitive. `params` may be omitted or `null` when a
//!   type takes no parameters (`CRF`, `EXT`).
//! * Nested schedules use the same `{"type", "params"}` shape. An
//!   optional nested schedule may be absent or `null`; both mean "none".
//! * Integer parameters (`n`, `n_intervals`, `seed`, `response_count`,
//!   `frco`, `rank`, `window`, `initial_ratio`, `n_comparisons`, and the
//!   `start` / `step_size` of a progressive ratio) must be JSON integers;
//!   `5.0` is rejected.
//! * Times are unitless reals in the engine's clock unit.
//! * `Percentile.target` accepts only `"irt"`; `AdjustingSchedule.target`
//!   accepts only `"ratio"` and `"interval"`. `Percentile.direction`
//!   defaults to `"above"`.
//! * `LimitedHold.inner` must be `FI`, `VI`, or `RI` (the armable
//!   schedules).
//!
//! Every malformed spec — unknown type, missing or mistyped parameter —
//! returns [`ContingencyError::Config`]; parameter values that the
//! schedule constructors reject surface their own `Config` error.
//!
//! # Example
//!
//! ```
//! use contingency::spec::from_spec;
//! use contingency::{ResponseEvent, Schedule};
//!
//! let spec = serde_json::json!({"type": "FR", "params": {"n": 2}});
//! let mut schedule = from_spec(&spec).unwrap();
//! let ev = ResponseEvent::new(0.5);
//! assert!(!schedule.step(0.5, Some(&ev)).unwrap().reinforced);
//! let ev = ResponseEvent::new(1.0);
//! assert!(schedule.step(1.0, Some(&ev)).unwrap().reinforced);
//! ```

#[allow(unused_imports)]
use crate::prelude::*;

use serde_json::{Map, Value};

use crate::schedules::{
    arithmetic, crf, geometric, richardson_roberts, AdjustingSchedule, AdjustingTarget,
    Alternative, Chained, Concurrent, Conjunctive, DiscriminatedAvoidance, DroMode, Escape, GoNoGo,
    InterlockingSchedule, Interpolate, LimitedHold, MatchingToSample, Mixed, Multiple, Overlay,
    Percentile, PercentileDirection, PercentileTarget, ProgressiveRatio, ResponseCost, SecondOrder,
    Sidman, Tandem, Timeout, TrialType, DRH, DRL, DRO, EXT, FI, FR, FT, RI, RR, RT, VI, VR, VT,
};
use crate::{ContingencyError, Result, Schedule};

/// Every `type` tag that [`from_spec`] accepts, in the order of the
/// spec tables (atomic, wrappers, differential, compound, progressive
/// and adjusting, aversive, trial-based).
pub const SPEC_TYPES: [&str; 36] = [
    "FR",
    "CRF",
    "EXT",
    "VR",
    "RR",
    "FI",
    "VI",
    "RI",
    "FT",
    "VT",
    "RT",
    "LimitedHold",
    "Timeout",
    "ResponseCost",
    "DRO",
    "DRL",
    "DRH",
    "Percentile",
    "Concurrent",
    "Alternative",
    "Conjunctive",
    "Multiple",
    "Mixed",
    "Chained",
    "Tandem",
    "Interpolate",
    "Overlay",
    "SecondOrder",
    "ProgressiveRatio",
    "InterlockingSchedule",
    "AdjustingSchedule",
    "Sidman",
    "DiscriminatedAvoidance",
    "Escape",
    "MatchingToSample",
    "GoNoGo",
];

/// Build a boxed [`Schedule`] from a schedule spec
/// (`{"type": ..., "params": {...}}`).
///
/// # Errors
///
/// Returns [`ContingencyError::Config`] when the spec is not an object,
/// names an unknown `type`, lacks a required parameter, carries a
/// parameter of the wrong JSON type, or holds a value the schedule's
/// constructor rejects.
pub fn from_spec(spec: &Value) -> Result<Box<dyn Schedule>> {
    let obj = spec
        .as_object()
        .ok_or_else(|| config(format!("schedule spec must be an object, got {spec}")))?;
    let ty = obj
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| config("schedule spec requires a string \"type\"".to_string()))?;
    let empty = Map::new();
    let params = match obj.get("params") {
        None | Some(Value::Null) => &empty,
        Some(Value::Object(m)) => m,
        Some(other) => {
            return Err(config(format!(
                "{ty}: \"params\" must be an object, got {other}"
            )))
        }
    };
    let p = Params { ty, map: params };

    let schedule: Box<dyn Schedule> = match ty {
        // --- atomic -----------------------------------------------------
        "FR" => Box::new(FR::new(p.req_u64("n")?)?),
        "CRF" => Box::new(crf()),
        "EXT" => Box::new(EXT::new()),
        "VR" => Box::new(VR::new(
            p.req_f64("mean")?,
            p.opt_usize("n_intervals")?.unwrap_or(12),
            p.opt_u64("seed")?,
        )?),
        "RR" => Box::new(RR::new(p.req_f64("probability")?, p.opt_u64("seed")?)?),
        "FI" => Box::new(build_fi(&p)?),
        "VI" => Box::new(build_vi(&p)?),
        "RI" => Box::new(build_ri(&p)?),
        "FT" => Box::new(FT::new(p.req_f64("interval")?)?),
        "VT" => Box::new(VT::new(
            p.req_f64("mean_interval")?,
            p.opt_usize("n_intervals")?.unwrap_or(12),
            p.opt_u64("seed")?,
        )?),
        "RT" => Box::new(RT::new(p.req_f64("mean_interval")?, p.opt_u64("seed")?)?),

        // --- wrappers ---------------------------------------------------
        "LimitedHold" => build_limited_hold(p.req_spec_value("inner")?, p.req_f64("hold")?)?,
        "Timeout" => Box::new(Timeout::new(
            p.req_schedule("inner")?,
            p.req_f64("duration")?,
            p.opt_bool("reset_on_response")?.unwrap_or(false),
        )?),
        "ResponseCost" => Box::new(ResponseCost::new(
            p.req_schedule("inner")?,
            p.req_f64("amount")?,
            p.opt_str("unit")?.unwrap_or("token").to_string(),
            p.opt_f64("initial_balance")?,
        )?),

        // --- differential -----------------------------------------------
        "DRO" => {
            let mode = match p.opt_str("dro_type")?.unwrap_or("resetting") {
                "resetting" => DroMode::Resetting,
                "momentary" => DroMode::Momentary,
                other => return Err(p.bad_value("dro_type", other)),
            };
            Box::new(DRO::new(p.req_f64("interval")?, mode)?)
        }
        "DRL" => Box::new(DRL::new(p.req_f64("interval")?)?),
        "DRH" => Box::new(DRH::new(
            p.req_u32("response_count")?,
            p.req_f64("time_window")?,
        )?),
        "Percentile" => {
            let target = match p.req_str("target")? {
                "irt" => PercentileTarget::Irt,
                other => return Err(p.bad_value("target", other)),
            };
            let direction = match p.opt_str("direction")?.unwrap_or("above") {
                "above" => PercentileDirection::Above,
                "below" => PercentileDirection::Below,
                other => return Err(p.bad_value("direction", other)),
            };
            let rank = u8::try_from(p.req_u64("rank")?)
                .map_err(|_| config(format!("{ty}: \"rank\" must be in 0..=100")))?;
            Box::new(Percentile::new(
                target,
                rank,
                p.req_usize("window")?,
                direction,
            )?)
        }

        // --- compound ---------------------------------------------------
        "Concurrent" => {
            let mut components: IndexMap<String, Box<dyn Schedule>> = IndexMap::default();
            for (key, cspec) in p.req_object("components")? {
                components.insert(key.clone(), nested(ty, "components", cspec)?);
            }
            Box::new(Concurrent::new(
                components,
                p.opt_f64("cod")?.unwrap_or(0.0),
                p.opt_u32("frco")?.unwrap_or(0),
            )?)
        }
        "Alternative" => Box::new(Alternative::new(
            p.req_schedule("first")?,
            p.req_schedule("second")?,
        )),
        "Conjunctive" => Box::new(Conjunctive::new(
            p.req_schedule("first")?,
            p.req_schedule("second")?,
        )?),
        "Multiple" => Box::new(Multiple::new(
            p.req_schedule_list("components")?,
            p.opt_str_vec("stimuli")?,
        )?),
        "Mixed" => Box::new(Mixed::new(p.req_schedule_list("components")?)?),
        "Chained" => Box::new(Chained::new(
            p.req_schedule_list("components")?,
            p.opt_str_vec("stimuli")?,
        )?),
        "Tandem" => Box::new(Tandem::new(p.req_schedule_list("components")?)?),
        "Interpolate" => Box::new(Interpolate::new(
            p.req_schedule("base")?,
            p.req_schedule("probe")?,
            p.req_f64("interval")?,
            p.req_f64("probe_duration")?,
            p.opt_f64("first_probe_at")?,
        )?),
        "Overlay" => Box::new(Overlay::new(
            p.req_schedule("base")?,
            p.req_schedule("punisher")?,
        )?),
        "SecondOrder" => Box::new(SecondOrder::new(
            p.req_schedule("overall")?,
            p.req_schedule("unit")?,
        )),

        // --- progressive and adjusting ----------------------------------
        "ProgressiveRatio" => {
            let step_fn = match p.req_str("step")? {
                "arithmetic" => arithmetic(
                    p.opt_u32("start")?.unwrap_or(1),
                    p.opt_u32("step_size")?.unwrap_or(1),
                )?,
                "geometric" => geometric(
                    p.opt_u32("start")?.unwrap_or(1),
                    p.opt_f64("ratio")?.unwrap_or(2.0),
                )?,
                "richardson_roberts" => richardson_roberts(),
                other => return Err(p.bad_value("step", other)),
            };
            Box::new(ProgressiveRatio::new(step_fn))
        }
        "InterlockingSchedule" => Box::new(InterlockingSchedule::new(
            p.req_u64("initial_ratio")?,
            p.req_f64("decay_time")?,
        )?),
        "AdjustingSchedule" => {
            let target = match p.req_str("target")? {
                "ratio" => AdjustingTarget::Ratio,
                "interval" => AdjustingTarget::Interval,
                other => return Err(p.bad_value("target", other)),
            };
            Box::new(AdjustingSchedule::new(
                target,
                p.req_f64("start")?,
                p.req_f64("step")?,
                p.opt_f64("minimum")?,
                p.opt_f64("maximum")?,
            )?)
        }

        // --- aversive ---------------------------------------------------
        "Sidman" => Box::new(Sidman::new(
            p.req_f64("shock_shock_interval")?,
            p.req_f64("response_shock_interval")?,
            p.opt_f64("shock_magnitude")?.unwrap_or(1.0),
        )?),
        "DiscriminatedAvoidance" => Box::new(DiscriminatedAvoidance::new(
            p.req_f64("warning_duration")?,
            p.req_f64("iti")?,
            p.opt_f64("shock_magnitude")?.unwrap_or(1.0),
        )?),
        "Escape" => Box::new(Escape::new(
            p.req_f64("trial_duration")?,
            p.req_f64("iti")?,
            p.opt_f64("shock_magnitude")?.unwrap_or(1.0),
        )?),

        // --- trial-based ------------------------------------------------
        "MatchingToSample" => Box::new(MatchingToSample::new(
            p.req_u32("n_comparisons")?,
            p.opt_f64("sample_duration")?.unwrap_or(0.0),
            p.opt_f64("choice_timeout")?.unwrap_or(5.0),
            p.opt_schedule("consequence")?,
            p.opt_schedule("incorrect")?,
            p.opt_f64("iti")?.unwrap_or(1.0),
            p.opt_u64("seed")?,
        )?
        .with_trial_types(
            p.opt_trial_types("trial_types")?,
            p.opt_trial_types("probe_trial_types")?,
        )?),
        "GoNoGo" => Box::new(GoNoGo::new(
            p.opt_f64("go_probability")?.unwrap_or(0.5),
            p.opt_f64("response_window")?.unwrap_or(5.0),
            p.opt_f64("iti")?.unwrap_or(5.0),
            p.opt_schedule("correct_go_schedule")?,
            p.opt_schedule("correct_nogo_schedule")?,
            p.opt_schedule("false_alarm")?,
            p.opt_u64("seed")?,
        )?),

        other => return Err(config(format!("unknown schedule type: {other:?}"))),
    };
    Ok(schedule)
}

// -----------------------------------------------------------------------------
// LimitedHold dispatch
// -----------------------------------------------------------------------------

/// `LimitedHold<S>` is generic over `S: ArmableSchedule`, so the inner
/// spec's type tag picks the concrete type here.
fn build_limited_hold(inner: &Value, hold: f64) -> Result<Box<dyn Schedule>> {
    let obj = inner.as_object().ok_or_else(|| {
        config(format!(
            "LimitedHold: \"inner\" must be an object, got {inner}"
        ))
    })?;
    let ty = obj
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| config("LimitedHold: \"inner\" requires a string \"type\"".to_string()))?;
    let empty = Map::new();
    let map = match obj.get("params") {
        None | Some(Value::Null) => &empty,
        Some(Value::Object(m)) => m,
        Some(other) => {
            return Err(config(format!(
                "{ty}: \"params\" must be an object, got {other}"
            )))
        }
    };
    let p = Params { ty, map };
    match ty {
        "FI" => Ok(Box::new(LimitedHold::new(build_fi(&p)?, hold)?)),
        "VI" => Ok(Box::new(LimitedHold::new(build_vi(&p)?, hold)?)),
        "RI" => Ok(Box::new(LimitedHold::new(build_ri(&p)?, hold)?)),
        other => Err(config(format!(
            "LimitedHold: \"inner\" must be FI, VI, or RI, got {other:?}"
        ))),
    }
}

fn build_fi(p: &Params<'_>) -> Result<FI> {
    FI::new(p.req_f64("interval")?)
}

fn build_vi(p: &Params<'_>) -> Result<VI> {
    VI::new(
        p.req_f64("mean_interval")?,
        p.opt_usize("n_intervals")?.unwrap_or(12),
        p.opt_u64("seed")?,
    )
}

fn build_ri(p: &Params<'_>) -> Result<RI> {
    RI::new(p.req_f64("mean_interval")?, p.opt_u64("seed")?)
}

// -----------------------------------------------------------------------------
// Parameter access
// -----------------------------------------------------------------------------

fn config(msg: String) -> ContingencyError {
    ContingencyError::Config(msg)
}

fn nested(ty: &str, key: &str, value: &Value) -> Result<Box<dyn Schedule>> {
    from_spec(value).map_err(|e| match e {
        ContingencyError::Config(msg) => config(format!("{ty}.{key}: {msg}")),
        other => other,
    })
}

/// A spec's `params` object, tagged with its `type` for error messages.
/// A key holding JSON `null` reads the same as an absent key.
struct Params<'a> {
    ty: &'a str,
    map: &'a Map<String, Value>,
}

impl<'a> Params<'a> {
    fn get(&self, key: &str) -> Option<&'a Value> {
        match self.map.get(key) {
            None | Some(Value::Null) => None,
            Some(v) => Some(v),
        }
    }

    fn missing(&self, key: &str) -> ContingencyError {
        config(format!("{}: missing required param {key:?}", self.ty))
    }

    fn wrong_type(&self, key: &str, expected: &str, got: &Value) -> ContingencyError {
        config(format!(
            "{}: param {key:?} must be {expected}, got {got}",
            self.ty
        ))
    }

    fn bad_value(&self, key: &str, got: &str) -> ContingencyError {
        config(format!(
            "{}: unsupported value {got:?} for param {key:?}",
            self.ty
        ))
    }

    fn opt_f64(&self, key: &str) -> Result<Option<f64>> {
        self.get(key)
            .map(|v| {
                v.as_f64()
                    .ok_or_else(|| self.wrong_type(key, "a number", v))
            })
            .transpose()
    }

    fn req_f64(&self, key: &str) -> Result<f64> {
        self.opt_f64(key)?.ok_or_else(|| self.missing(key))
    }

    fn opt_u64(&self, key: &str) -> Result<Option<u64>> {
        self.get(key)
            .map(|v| {
                v.as_u64()
                    .ok_or_else(|| self.wrong_type(key, "a non-negative integer", v))
            })
            .transpose()
    }

    fn req_u64(&self, key: &str) -> Result<u64> {
        self.opt_u64(key)?.ok_or_else(|| self.missing(key))
    }

    fn opt_u32(&self, key: &str) -> Result<Option<u32>> {
        self.opt_u64(key)?
            .map(|x| {
                u32::try_from(x)
                    .map_err(|_| config(format!("{}: param {key:?} out of range: {x}", self.ty)))
            })
            .transpose()
    }

    fn req_u32(&self, key: &str) -> Result<u32> {
        self.opt_u32(key)?.ok_or_else(|| self.missing(key))
    }

    fn opt_usize(&self, key: &str) -> Result<Option<usize>> {
        self.opt_u64(key)?
            .map(|x| {
                usize::try_from(x)
                    .map_err(|_| config(format!("{}: param {key:?} out of range: {x}", self.ty)))
            })
            .transpose()
    }

    fn req_usize(&self, key: &str) -> Result<usize> {
        self.opt_usize(key)?.ok_or_else(|| self.missing(key))
    }

    fn opt_bool(&self, key: &str) -> Result<Option<bool>> {
        self.get(key)
            .map(|v| {
                v.as_bool()
                    .ok_or_else(|| self.wrong_type(key, "a boolean", v))
            })
            .transpose()
    }

    fn opt_str(&self, key: &str) -> Result<Option<&'a str>> {
        self.get(key)
            .map(|v| {
                v.as_str()
                    .ok_or_else(|| self.wrong_type(key, "a string", v))
            })
            .transpose()
    }

    fn req_str(&self, key: &str) -> Result<&'a str> {
        self.opt_str(key)?.ok_or_else(|| self.missing(key))
    }

    fn opt_str_vec(&self, key: &str) -> Result<Option<Vec<String>>> {
        let Some(v) = self.get(key) else {
            return Ok(None);
        };
        let arr = v
            .as_array()
            .ok_or_else(|| self.wrong_type(key, "an array of strings", v))?;
        arr.iter()
            .map(|x| {
                x.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| self.wrong_type(key, "an array of strings", v))
            })
            .collect::<Result<Vec<_>>>()
            .map(Some)
    }

    /// An array of `{sample, correct, comparisons}` objects, as
    /// `MatchingToSample.trial_types` / `probe_trial_types` take.
    fn opt_trial_types(&self, key: &str) -> Result<Option<Vec<TrialType>>> {
        const EXPECTED: &str = "an array of {sample, correct, comparisons} objects";
        let Some(v) = self.get(key) else {
            return Ok(None);
        };
        let arr = v.as_array().ok_or_else(|| self.wrong_type(key, EXPECTED, v))?;
        arr.iter()
            .map(|t| {
                let obj = t.as_object().ok_or_else(|| self.wrong_type(key, EXPECTED, v))?;
                let text = |field: &str| {
                    obj.get(field)
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .ok_or_else(|| self.wrong_type(key, EXPECTED, v))
                };
                let comparisons = obj
                    .get("comparisons")
                    .and_then(Value::as_array)
                    .ok_or_else(|| self.wrong_type(key, EXPECTED, v))?
                    .iter()
                    .map(|c| {
                        c.as_str()
                            .map(str::to_string)
                            .ok_or_else(|| self.wrong_type(key, EXPECTED, v))
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok(TrialType::new(text("sample")?, text("correct")?, comparisons))
            })
            .collect::<Result<Vec<_>>>()
            .map(Some)
    }

    fn req_object(&self, key: &str) -> Result<&'a Map<String, Value>> {
        let v = self.get(key).ok_or_else(|| self.missing(key))?;
        v.as_object()
            .ok_or_else(|| self.wrong_type(key, "an object of schedule specs", v))
    }

    fn req_spec_value(&self, key: &str) -> Result<&'a Value> {
        self.get(key).ok_or_else(|| self.missing(key))
    }

    fn req_schedule(&self, key: &str) -> Result<Box<dyn Schedule>> {
        nested(self.ty, key, self.req_spec_value(key)?)
    }

    fn opt_schedule(&self, key: &str) -> Result<Option<Box<dyn Schedule>>> {
        self.get(key).map(|v| nested(self.ty, key, v)).transpose()
    }

    fn req_schedule_list(&self, key: &str) -> Result<Vec<Box<dyn Schedule>>> {
        let v = self.get(key).ok_or_else(|| self.missing(key))?;
        v.as_array()
            .ok_or_else(|| self.wrong_type(key, "an array of schedule specs", v))?
            .iter()
            .map(|c| nested(self.ty, key, c))
            .collect()
    }
}

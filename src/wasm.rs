//! JavaScript / TypeScript bindings for the contingency schedule engine.
//!
//! Compiled only for `wasm32-*` targets via `wasm-bindgen`. Build with:
//!
//! ```text
//! cargo build -p contingency --target wasm32-unknown-unknown --release
//! # or, via wasm-pack for npm packaging:
//! wasm-pack build --target web --release
//! ```
//!
//! On the JS side every schedule is wrapped in a single `Schedule` class
//! with static constructor methods (`Schedule.fr(5)`, `Schedule.fi(10.0)`,
//! `Schedule.alternative(a, b)`, …) together with the standard
//! `step(now, event?)` / `reset()` pair. The binding mirrors the PyO3
//! `PySchedule` factory-method pattern one-for-one.

#![allow(clippy::useless_conversion)]

use indexmap::IndexMap;
use wasm_bindgen::prelude::*;

use crate::{
    schedules::{self, LimitedHold, ProgressiveRatio},
    ContingencyError, Outcome, Reinforcer, ResponseEvent, Schedule,
};

// ---------------------------------------------------------------------------
// Error mapping
// ---------------------------------------------------------------------------

fn err_to_js(e: ContingencyError) -> JsValue {
    JsError::new(&e.to_string()).into()
}

// ---------------------------------------------------------------------------
// ResponseEvent
// ---------------------------------------------------------------------------

/// JS-facing wrapper for a single response event.
#[wasm_bindgen(js_name = "ResponseEvent")]
#[derive(Clone)]
pub struct JsResponseEvent {
    inner: ResponseEvent,
}

#[wasm_bindgen(js_class = "ResponseEvent")]
impl JsResponseEvent {
    /// Construct a response event. ``operandum`` defaults to ``"main"``.
    #[wasm_bindgen(constructor)]
    pub fn new(time: f64, operandum: Option<String>) -> Self {
        Self {
            inner: ResponseEvent {
                time,
                operandum: operandum.unwrap_or_else(|| "main".into()),
            },
        }
    }

    /// Monotonic timestamp of the response.
    #[wasm_bindgen(getter)]
    pub fn time(&self) -> f64 {
        self.inner.time
    }

    /// Identifier of the operandum (lever, key, button).
    #[wasm_bindgen(getter)]
    pub fn operandum(&self) -> String {
        self.inner.operandum.clone()
    }
}

// ---------------------------------------------------------------------------
// Reinforcer
// ---------------------------------------------------------------------------

/// JS-facing wrapper for a scheduled reinforcer.
#[wasm_bindgen(js_name = "Reinforcer")]
#[derive(Clone)]
pub struct JsReinforcer {
    inner: Reinforcer,
}

#[wasm_bindgen(js_class = "Reinforcer")]
impl JsReinforcer {
    /// Delivery time (monotonic).
    #[wasm_bindgen(getter)]
    pub fn time(&self) -> f64 {
        self.inner.time
    }

    /// Delivery magnitude. Negative magnitudes encode punishers.
    #[wasm_bindgen(getter)]
    pub fn magnitude(&self) -> f64 {
        self.inner.magnitude
    }

    /// Label — canonically ``"SR+"`` or ``"SR-"``.
    #[wasm_bindgen(getter)]
    pub fn label(&self) -> String {
        self.inner.label.clone()
    }
}

// ---------------------------------------------------------------------------
// Outcome
// ---------------------------------------------------------------------------

/// JS-facing wrapper for a schedule step result.
#[wasm_bindgen(js_name = "Outcome")]
pub struct JsOutcome {
    inner: Outcome,
}

#[wasm_bindgen(js_class = "Outcome")]
impl JsOutcome {
    /// Whether the step produced a reinforcement delivery.
    #[wasm_bindgen(getter)]
    pub fn reinforced(&self) -> bool {
        self.inner.reinforced
    }

    /// The scheduled reinforcer, or ``undefined`` when
    /// ``reinforced === false``.
    #[wasm_bindgen(getter)]
    pub fn reinforcer(&self) -> Option<JsReinforcer> {
        self.inner
            .reinforcer
            .as_ref()
            .map(|r| JsReinforcer { inner: r.clone() })
    }

    /// Backend-specific payload, returned as a plain JS object keyed by
    /// meta name. Empty object when no meta was attached.
    #[wasm_bindgen(getter)]
    pub fn meta(&self) -> Result<JsValue, JsValue> {
        serde_wasm_bindgen::to_value(&self.inner.meta)
            .map_err(|e| JsError::new(&e.to_string()).into())
    }
}

// ---------------------------------------------------------------------------
// Schedule — unified wrapper
// ---------------------------------------------------------------------------

/// JS-facing handle to any reinforcement schedule.
///
/// Construction is via the static factory methods (`Schedule.fr(5)`,
/// `Schedule.alternative(a, b)`, …). Compound schedules consume the
/// `Schedule` values handed to them, so a `Schedule` cannot be used after
/// it has been wrapped by a compound.
#[wasm_bindgen(js_name = "Schedule")]
pub struct JsSchedule {
    inner: Box<dyn Schedule>,
}

impl JsSchedule {
    fn from_box(inner: Box<dyn Schedule>) -> Self {
        Self { inner }
    }
}

#[wasm_bindgen(js_class = "Schedule")]
impl JsSchedule {
    // ------------------------------------------------------------------
    // Core API
    // ------------------------------------------------------------------

    /// Advance the schedule to ``now`` and optionally deliver a response.
    pub fn step(
        &mut self,
        now: f64,
        event: Option<JsResponseEvent>,
    ) -> Result<JsOutcome, JsValue> {
        let ev = event.as_ref().map(|e| e.inner.clone());
        let out = self.inner.step(now, ev.as_ref()).map_err(err_to_js)?;
        Ok(JsOutcome { inner: out })
    }

    /// Return the schedule to its post-construction state.
    pub fn reset(&mut self) {
        self.inner.reset();
    }

    // ------------------------------------------------------------------
    // Ratio family
    // ------------------------------------------------------------------

    /// Fixed Ratio.
    #[wasm_bindgen(js_name = "fr")]
    pub fn fr(n: u64) -> Result<JsSchedule, JsValue> {
        Ok(Self::from_box(Box::new(
            schedules::FR::new(n).map_err(err_to_js)?,
        )))
    }

    /// Continuous Reinforcement (FR 1).
    #[wasm_bindgen(js_name = "crf")]
    pub fn crf() -> JsSchedule {
        Self::from_box(Box::new(schedules::crf()))
    }

    /// Variable Ratio with Fleshler-Hoffman sequence.
    #[wasm_bindgen(js_name = "vr")]
    pub fn vr(mean: f64, n_intervals: usize, seed: Option<u64>) -> Result<JsSchedule, JsValue> {
        Ok(Self::from_box(Box::new(
            schedules::VR::new(mean, n_intervals, seed).map_err(err_to_js)?,
        )))
    }

    /// Random Ratio.
    #[wasm_bindgen(js_name = "rr")]
    pub fn rr(probability: f64, seed: Option<u64>) -> Result<JsSchedule, JsValue> {
        Ok(Self::from_box(Box::new(
            schedules::RR::new(probability, seed).map_err(err_to_js)?,
        )))
    }

    // ------------------------------------------------------------------
    // Interval family
    // ------------------------------------------------------------------

    /// Fixed Interval.
    #[wasm_bindgen(js_name = "fi")]
    pub fn fi(interval: f64) -> Result<JsSchedule, JsValue> {
        Ok(Self::from_box(Box::new(
            schedules::FI::new(interval).map_err(err_to_js)?,
        )))
    }

    /// Variable Interval with Fleshler-Hoffman sequence.
    #[wasm_bindgen(js_name = "vi")]
    pub fn vi(
        mean_interval: f64,
        n_intervals: usize,
        seed: Option<u64>,
    ) -> Result<JsSchedule, JsValue> {
        Ok(Self::from_box(Box::new(
            schedules::VI::new(mean_interval, n_intervals, seed).map_err(err_to_js)?,
        )))
    }

    /// Random Interval.
    #[wasm_bindgen(js_name = "ri")]
    pub fn ri(mean_interval: f64, seed: Option<u64>) -> Result<JsSchedule, JsValue> {
        Ok(Self::from_box(Box::new(
            schedules::RI::new(mean_interval, seed).map_err(err_to_js)?,
        )))
    }

    // ------------------------------------------------------------------
    // Time-based family
    // ------------------------------------------------------------------

    /// Fixed Time (non-contingent).
    #[wasm_bindgen(js_name = "ft")]
    pub fn ft(interval: f64) -> Result<JsSchedule, JsValue> {
        Ok(Self::from_box(Box::new(
            schedules::FT::new(interval).map_err(err_to_js)?,
        )))
    }

    /// Variable Time (non-contingent) with Fleshler-Hoffman sequence.
    #[wasm_bindgen(js_name = "vt")]
    pub fn vt(
        mean_interval: f64,
        n_intervals: usize,
        seed: Option<u64>,
    ) -> Result<JsSchedule, JsValue> {
        Ok(Self::from_box(Box::new(
            schedules::VT::new(mean_interval, n_intervals, seed).map_err(err_to_js)?,
        )))
    }

    /// Random Time (non-contingent).
    #[wasm_bindgen(js_name = "rt")]
    pub fn rt(mean_interval: f64, seed: Option<u64>) -> Result<JsSchedule, JsValue> {
        Ok(Self::from_box(Box::new(
            schedules::RT::new(mean_interval, seed).map_err(err_to_js)?,
        )))
    }

    /// Extinction.
    #[wasm_bindgen(js_name = "ext")]
    pub fn ext() -> JsSchedule {
        Self::from_box(Box::new(schedules::EXT::new()))
    }

    // ------------------------------------------------------------------
    // Differential family
    // ------------------------------------------------------------------

    /// Differential Reinforcement of Other behavior (resetting variant).
    #[wasm_bindgen(js_name = "droResetting")]
    pub fn dro_resetting(interval: f64) -> Result<JsSchedule, JsValue> {
        Ok(Self::from_box(Box::new(
            schedules::DRO::resetting(interval).map_err(err_to_js)?,
        )))
    }

    /// Differential Reinforcement of Other behavior (momentary variant).
    #[wasm_bindgen(js_name = "droMomentary")]
    pub fn dro_momentary(interval: f64) -> Result<JsSchedule, JsValue> {
        Ok(Self::from_box(Box::new(
            schedules::DRO::momentary(interval).map_err(err_to_js)?,
        )))
    }

    /// Differential Reinforcement of Low rates.
    #[wasm_bindgen(js_name = "drl")]
    pub fn drl(interval: f64) -> Result<JsSchedule, JsValue> {
        Ok(Self::from_box(Box::new(
            schedules::DRL::new(interval).map_err(err_to_js)?,
        )))
    }

    /// Differential Reinforcement of High rates.
    #[wasm_bindgen(js_name = "drh")]
    pub fn drh(response_count: u32, time_window: f64) -> Result<JsSchedule, JsValue> {
        Ok(Self::from_box(Box::new(
            schedules::DRH::new(response_count, time_window).map_err(err_to_js)?,
        )))
    }

    // ------------------------------------------------------------------
    // Progressive-ratio family
    // ------------------------------------------------------------------

    /// Progressive-ratio schedule with an arithmetic step function.
    #[wasm_bindgen(js_name = "prArithmetic")]
    pub fn pr_arithmetic(start: u32, step: u32) -> Result<JsSchedule, JsValue> {
        let f = schedules::arithmetic(start, step).map_err(err_to_js)?;
        Ok(Self::from_box(Box::new(ProgressiveRatio::new(f))))
    }

    /// Progressive-ratio schedule with a geometric step function.
    #[wasm_bindgen(js_name = "prGeometric")]
    pub fn pr_geometric(start: u32, ratio: f64) -> Result<JsSchedule, JsValue> {
        let f = schedules::geometric(start, ratio).map_err(err_to_js)?;
        Ok(Self::from_box(Box::new(ProgressiveRatio::new(f))))
    }

    /// Progressive-ratio schedule using the Richardson-Roberts (1996) series.
    #[wasm_bindgen(js_name = "prRichardsonRoberts")]
    pub fn pr_richardson_roberts() -> JsSchedule {
        Self::from_box(Box::new(ProgressiveRatio::new(
            schedules::richardson_roberts(),
        )))
    }

    // ------------------------------------------------------------------
    // Compound: Alternative (binary, consumes both)
    // ------------------------------------------------------------------

    /// Alternative (whichever-first) compound schedule. Consumes both
    /// arguments.
    #[wasm_bindgen(js_name = "alternative")]
    pub fn alternative(first: JsSchedule, second: JsSchedule) -> JsSchedule {
        Self::from_box(Box::new(schedules::Alternative::new(
            first.inner,
            second.inner,
        )))
    }

    // ------------------------------------------------------------------
    // Compound: sequence family (Multiple / Chained / Tandem)
    // ------------------------------------------------------------------

    /// Multiple compound schedule. Consumes each provided component.
    #[wasm_bindgen(js_name = "multiple")]
    pub fn multiple(
        components: Vec<JsSchedule>,
        stimuli: Option<Vec<String>>,
    ) -> Result<JsSchedule, JsValue> {
        let inner: Vec<Box<dyn Schedule>> = components.into_iter().map(|s| s.inner).collect();
        Ok(Self::from_box(Box::new(
            schedules::Multiple::new(inner, stimuli).map_err(err_to_js)?,
        )))
    }

    /// Chained compound schedule. Consumes each provided component.
    #[wasm_bindgen(js_name = "chained")]
    pub fn chained(
        components: Vec<JsSchedule>,
        stimuli: Option<Vec<String>>,
    ) -> Result<JsSchedule, JsValue> {
        let inner: Vec<Box<dyn Schedule>> = components.into_iter().map(|s| s.inner).collect();
        Ok(Self::from_box(Box::new(
            schedules::Chained::new(inner, stimuli).map_err(err_to_js)?,
        )))
    }

    /// Tandem compound schedule. Consumes each provided component.
    #[wasm_bindgen(js_name = "tandem")]
    pub fn tandem(components: Vec<JsSchedule>) -> Result<JsSchedule, JsValue> {
        let inner: Vec<Box<dyn Schedule>> = components.into_iter().map(|s| s.inner).collect();
        Ok(Self::from_box(Box::new(
            schedules::Tandem::new(inner).map_err(err_to_js)?,
        )))
    }

    // ------------------------------------------------------------------
    // Compound: Concurrent
    // ------------------------------------------------------------------

    /// Concurrent compound schedule keyed by operandum.
    ///
    /// ``operanda`` and ``components`` are parallel arrays of equal
    /// length; each schedule is consumed. Duplicate operandum keys are
    /// rejected.
    #[wasm_bindgen(js_name = "concurrent")]
    pub fn concurrent(
        operanda: Vec<String>,
        components: Vec<JsSchedule>,
        cod: f64,
        cor: u32,
    ) -> Result<JsSchedule, JsValue> {
        if operanda.len() != components.len() {
            return Err(JsError::new(
                "concurrent: operanda and components must have the same length",
            )
            .into());
        }
        let mut map: IndexMap<String, Box<dyn Schedule>> =
            IndexMap::with_capacity(components.len());
        for (key, c) in operanda.into_iter().zip(components) {
            if map.insert(key.clone(), c.inner).is_some() {
                return Err(JsError::new(&format!(
                    "concurrent: duplicate operandum key {key:?}"
                ))
                .into());
            }
        }
        Ok(Self::from_box(Box::new(
            schedules::Concurrent::new(map, cod, cor).map_err(err_to_js)?,
        )))
    }

    // ------------------------------------------------------------------
    // Limited-hold convenience factories
    // ------------------------------------------------------------------
    //
    // JS has no equivalent of the Python `ArmableSchedule` two-step
    // construction idiom. Expose one convenience factory per interval
    // family instead so the caller can build an FI / VI / RI wrapped by
    // LimitedHold in a single call.

    /// LimitedHold(FI).
    #[wasm_bindgen(js_name = "limitedHoldFi")]
    pub fn limited_hold_fi(interval: f64, hold: f64) -> Result<JsSchedule, JsValue> {
        let fi = schedules::FI::new(interval).map_err(err_to_js)?;
        let lh = LimitedHold::new(fi, hold).map_err(err_to_js)?;
        Ok(Self::from_box(Box::new(lh)))
    }

    /// LimitedHold(VI).
    #[wasm_bindgen(js_name = "limitedHoldVi")]
    pub fn limited_hold_vi(
        mean_interval: f64,
        n_intervals: usize,
        seed: Option<u64>,
        hold: f64,
    ) -> Result<JsSchedule, JsValue> {
        let vi = schedules::VI::new(mean_interval, n_intervals, seed).map_err(err_to_js)?;
        let lh = LimitedHold::new(vi, hold).map_err(err_to_js)?;
        Ok(Self::from_box(Box::new(lh)))
    }

    /// LimitedHold(RI).
    #[wasm_bindgen(js_name = "limitedHoldRi")]
    pub fn limited_hold_ri(
        mean_interval: f64,
        seed: Option<u64>,
        hold: f64,
    ) -> Result<JsSchedule, JsValue> {
        let ri = schedules::RI::new(mean_interval, seed).map_err(err_to_js)?;
        let lh = LimitedHold::new(ri, hold).map_err(err_to_js)?;
        Ok(Self::from_box(Box::new(lh)))
    }
}

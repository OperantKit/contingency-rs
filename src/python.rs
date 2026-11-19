//! Python bindings via PyO3.
//!
//! Compiled when the `python` feature is enabled. Build the extension
//! module with maturin or cargo:
//!
//! ```text
//! cargo build -p contingency --features python
//! ```
//!
//! Every schedule is wrapped in a single [`PySchedule`] class exposing
//! factory classmethods (``Schedule.fr(5)``, ``Schedule.fi(10.0)``,
//! ``Schedule.alternative(a, b)``, …) together with the standard
//! ``step(now, event=None)`` / ``reset()`` pair. Interval-family
//! schedules that can be wrapped by limited-hold live behind a parallel
//! [`PyArmableSchedule`] class.
//!
//! The bindings are deliberately thin: they mirror the Rust public API
//! almost one-for-one so that Python-side tests can cross-validate
//! Rust behaviour against the Python reference port.

// PyO3 0.22's `#[pymethods]` macro expansion emits `<_ as Into<PyErr>>`
// conversions that clippy's `useless_conversion` lint flags on every
// method returning `PyResult<T>`. The expansion lives inside the macro,
// so the false positives are silenced crate-module-wide here.
#![allow(clippy::useless_conversion)]

use std::collections::BTreeMap;

use pyo3::basic::CompareOp;
use pyo3::exceptions::{PyRuntimeError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict, PyFloat, PyInt, PyList, PyString, PyTuple, PyType};
use pyo3::wrap_pyfunction;

use indexmap::IndexMap;

use crate::{
    respondent,
    schedule::ArmableSchedule,
    schedules::{self, DroMode, LimitedHold, ProgressiveRatio, StepFn},
    spec,
    types::MetaValue,
    ContingencyError, Outcome, Reinforcer, ResponseEvent, Schedule,
};

/// Rich comparison for value objects: only `==` and `!=` are defined.
fn eq_only(op: CompareOp, equal: bool, py: Python<'_>) -> PyObject {
    match op {
        CompareOp::Eq => equal.into_py(py),
        CompareOp::Ne => (!equal).into_py(py),
        _ => py.NotImplemented(),
    }
}

// ---------------------------------------------------------------------------
// Error mapping
// ---------------------------------------------------------------------------

fn map_err(e: ContingencyError) -> PyErr {
    match e {
        ContingencyError::Config(msg) => PyValueError::new_err(msg),
        ContingencyError::State(msg) => PyRuntimeError::new_err(msg),
        ContingencyError::Hardware(msg) => PyRuntimeError::new_err(msg),
    }
}

// ---------------------------------------------------------------------------
// ResponseEvent
// ---------------------------------------------------------------------------

#[pyclass(name = "ResponseEvent")]
#[derive(Clone)]
struct PyResponseEvent {
    inner: ResponseEvent,
}

#[pymethods]
impl PyResponseEvent {
    #[new]
    #[pyo3(signature = (time, operandum = "main".to_string()))]
    fn new(time: f64, operandum: String) -> Self {
        Self {
            inner: ResponseEvent { time, operandum },
        }
    }

    #[getter]
    fn time(&self) -> f64 {
        self.inner.time
    }

    #[getter]
    fn operandum(&self) -> &str {
        &self.inner.operandum
    }

    fn __repr__(&self) -> String {
        format!(
            "ResponseEvent(time={}, operandum={:?})",
            self.inner.time, self.inner.operandum
        )
    }

    fn __richcmp__(&self, other: &Bound<'_, PyAny>, op: CompareOp, py: Python<'_>) -> PyObject {
        match other.extract::<PyRef<'_, PyResponseEvent>>() {
            Ok(o) => eq_only(op, self.inner == o.inner, py),
            Err(_) => py.NotImplemented(),
        }
    }
}

// ---------------------------------------------------------------------------
// Reinforcer
// ---------------------------------------------------------------------------

#[pyclass(name = "Reinforcer")]
#[derive(Clone)]
struct PyReinforcer {
    inner: Reinforcer,
}

#[pymethods]
impl PyReinforcer {
    /// Same signature and defaults as contingency-py's
    /// ``Reinforcer(time, magnitude=1.0, label="SR+")``.
    #[new]
    #[pyo3(signature = (time, magnitude = 1.0, label = "SR+".to_string()))]
    fn new(time: f64, magnitude: f64, label: String) -> Self {
        Self {
            inner: Reinforcer {
                time,
                magnitude,
                label,
            },
        }
    }

    fn __richcmp__(&self, other: &Bound<'_, PyAny>, op: CompareOp, py: Python<'_>) -> PyObject {
        match other.extract::<PyRef<'_, PyReinforcer>>() {
            Ok(o) => eq_only(op, self.inner == o.inner, py),
            Err(_) => py.NotImplemented(),
        }
    }

    #[getter]
    fn time(&self) -> f64 {
        self.inner.time
    }

    #[getter]
    fn magnitude(&self) -> f64 {
        self.inner.magnitude
    }

    #[getter]
    fn label(&self) -> &str {
        &self.inner.label
    }

    fn __repr__(&self) -> String {
        format!(
            "Reinforcer(time={}, magnitude={}, label={:?})",
            self.inner.time, self.inner.magnitude, self.inner.label
        )
    }
}

/// Convert a meta map into a Python ``dict``.
fn meta_to_py<'py>(
    py: Python<'py>,
    meta: &BTreeMap<String, MetaValue>,
) -> PyResult<Bound<'py, PyDict>> {
    let dict = PyDict::new_bound(py);
    for (key, value) in meta.iter() {
        match value {
            MetaValue::Bool(b) => dict.set_item(key, b)?,
            MetaValue::Int(i) => dict.set_item(key, i)?,
            MetaValue::Float(f) => dict.set_item(key, f)?,
            MetaValue::Str(s) => dict.set_item(key, s)?,
            MetaValue::StrList(v) => dict.set_item(key, v)?,
            MetaValue::FloatList(v) => dict.set_item(key, v)?,
        }
    }
    Ok(dict)
}

// ---------------------------------------------------------------------------
// Outcome
// ---------------------------------------------------------------------------

#[pyclass(name = "Outcome")]
struct PyOutcome {
    inner: Outcome,
}

/// Convert one Python meta value into a [`MetaValue`].
///
/// `bool` is checked before `int` because Python's `bool` is an `int`
/// subclass. Lists and tuples must hold only strings.
fn meta_value_from_py(key: &str, value: &Bound<'_, PyAny>) -> PyResult<MetaValue> {
    if value.is_instance_of::<PyBool>() {
        return Ok(MetaValue::Bool(value.extract()?));
    }
    if value.is_instance_of::<PyInt>() {
        return Ok(MetaValue::Int(value.extract()?));
    }
    if value.is_instance_of::<PyFloat>() {
        return Ok(MetaValue::Float(value.extract()?));
    }
    if value.is_instance_of::<PyString>() {
        return Ok(MetaValue::Str(value.extract()?));
    }
    if value.is_instance_of::<PyList>() || value.is_instance_of::<PyTuple>() {
        if let Ok(items) = value.extract::<Vec<String>>() {
            return Ok(MetaValue::StrList(items));
        }
    }
    Err(PyTypeError::new_err(format!(
        "meta[{key:?}] must be bool, int, float, str or a list of str, got {}",
        value.get_type().name()?
    )))
}

fn meta_from_py(meta: Option<&Bound<'_, PyDict>>) -> PyResult<BTreeMap<String, MetaValue>> {
    let mut out = BTreeMap::new();
    if let Some(dict) = meta {
        for (k, v) in dict.iter() {
            let key: String = k
                .extract()
                .map_err(|_| PyTypeError::new_err("meta keys must be str"))?;
            let value = meta_value_from_py(&key, &v)?;
            out.insert(key, value);
        }
    }
    Ok(out)
}

#[pymethods]
impl PyOutcome {
    /// Same signature, defaults and validation as contingency-py's
    /// ``Outcome(reinforced=False, reinforcer=None, meta={})``.
    #[new]
    #[pyo3(signature = (reinforced = false, reinforcer = None, meta = None))]
    fn new(
        reinforced: bool,
        reinforcer: Option<PyRef<'_, PyReinforcer>>,
        meta: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Self> {
        if reinforced && reinforcer.is_none() {
            return Err(PyValueError::new_err(
                "reinforced=True requires a Reinforcer",
            ));
        }
        if !reinforced && reinforcer.is_some() {
            return Err(PyValueError::new_err(
                "reinforcer must be None when reinforced=False",
            ));
        }
        Ok(Self {
            inner: Outcome {
                reinforced,
                reinforcer: reinforcer.map(|r| r.inner.clone()),
                meta: meta_from_py(meta)?,
            },
        })
    }

    fn __richcmp__(&self, other: &Bound<'_, PyAny>, op: CompareOp, py: Python<'_>) -> PyObject {
        match other.extract::<PyRef<'_, PyOutcome>>() {
            Ok(o) => eq_only(op, self.inner == o.inner, py),
            Err(_) => py.NotImplemented(),
        }
    }

    #[getter]
    fn reinforced(&self) -> bool {
        self.inner.reinforced
    }

    #[getter]
    fn reinforcer(&self) -> Option<PyReinforcer> {
        self.inner
            .reinforcer
            .as_ref()
            .map(|r| PyReinforcer { inner: r.clone() })
    }

    #[getter]
    fn meta<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        meta_to_py(py, &self.inner.meta)
    }

    fn __repr__(&self) -> String {
        let kind = if self.inner.reinforced {
            "reinforced"
        } else {
            "empty"
        };
        format!("Outcome({}, meta_len={})", kind, self.inner.meta.len())
    }
}

// ---------------------------------------------------------------------------
// PySchedule — the unified wrapper
// ---------------------------------------------------------------------------

/// Unified Python-facing handle to any reinforcement schedule.
///
/// Construction is via factory classmethods; the underlying Rust
/// schedule is type-erased behind `Box<dyn Schedule>`. Compound
/// schedules consume (move) the `PySchedule` values handed to them, so
/// a `PySchedule` cannot be used after it has been wrapped by a
/// compound.
#[pyclass(name = "Schedule", unsendable)]
pub struct PySchedule {
    inner: Box<dyn Schedule>,
}

impl PySchedule {
    fn from_box(inner: Box<dyn Schedule>) -> Self {
        Self { inner }
    }
}

#[pymethods]
impl PySchedule {
    // ------------------------------------------------------------------
    // Core API
    // ------------------------------------------------------------------

    /// Advance the schedule to ``now`` and optionally deliver a response.
    #[pyo3(signature = (now, event = None))]
    fn step(&mut self, now: f64, event: Option<&PyResponseEvent>) -> PyResult<PyOutcome> {
        let ev = event.map(|e| e.inner.clone());
        let out = self.inner.step(now, ev.as_ref()).map_err(map_err)?;
        Ok(PyOutcome { inner: out })
    }

    /// Return the schedule to its post-construction state.
    fn reset(&mut self) {
        self.inner.reset();
    }

    /// Report whether a response ``event`` at ``now`` would reinforce,
    /// without changing this schedule's state.
    ///
    /// Returns ``None`` for schedules that offer no such probe (only FR,
    /// VR, RR, FI, VI and RI do; see ``Schedule::probe_response``).
    fn probe_response(&self, now: f64, event: &PyResponseEvent) -> PyResult<Option<bool>> {
        match self.inner.probe_response(now, &event.inner) {
            None => Ok(None),
            Some(result) => result.map(Some).map_err(map_err),
        }
    }

    /// The observable state of the schedule after the last step, as a
    /// ``dict`` (empty for schedules without one; see ``Schedule::state``).
    fn state<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        meta_to_py(py, &self.inner.state())
    }

    /// Build any schedule from a spec (``{"type": ..., "params": ...}``).
    ///
    /// Same as the module-level :func:`from_spec`.
    #[classmethod]
    fn from_spec(_cls: &Bound<'_, PyType>, spec: &Bound<'_, PyAny>) -> PyResult<Self> {
        build_from_spec(spec)
    }

    // ------------------------------------------------------------------
    // Ratio family
    // ------------------------------------------------------------------

    #[classmethod]
    fn fr(_cls: &Bound<'_, PyType>, n: u64) -> PyResult<Self> {
        Ok(Self::from_box(Box::new(
            schedules::FR::new(n).map_err(map_err)?,
        )))
    }

    #[classmethod]
    fn crf(_cls: &Bound<'_, PyType>) -> PyResult<Self> {
        Ok(Self::from_box(Box::new(schedules::crf())))
    }

    #[classmethod]
    #[pyo3(signature = (mean, n_intervals = 12, seed = None))]
    fn vr(
        _cls: &Bound<'_, PyType>,
        mean: f64,
        n_intervals: usize,
        seed: Option<u64>,
    ) -> PyResult<Self> {
        Ok(Self::from_box(Box::new(
            schedules::VR::new(mean, n_intervals, seed).map_err(map_err)?,
        )))
    }

    #[classmethod]
    #[pyo3(signature = (probability, seed = None))]
    fn rr(_cls: &Bound<'_, PyType>, probability: f64, seed: Option<u64>) -> PyResult<Self> {
        Ok(Self::from_box(Box::new(
            schedules::RR::new(probability, seed).map_err(map_err)?,
        )))
    }

    // ------------------------------------------------------------------
    // Interval family
    // ------------------------------------------------------------------

    #[classmethod]
    fn fi(_cls: &Bound<'_, PyType>, interval: f64) -> PyResult<Self> {
        Ok(Self::from_box(Box::new(
            schedules::FI::new(interval).map_err(map_err)?,
        )))
    }

    #[classmethod]
    #[pyo3(signature = (mean_interval, n_intervals = 12, seed = None))]
    fn vi(
        _cls: &Bound<'_, PyType>,
        mean_interval: f64,
        n_intervals: usize,
        seed: Option<u64>,
    ) -> PyResult<Self> {
        Ok(Self::from_box(Box::new(
            schedules::VI::new(mean_interval, n_intervals, seed).map_err(map_err)?,
        )))
    }

    #[classmethod]
    #[pyo3(signature = (mean_interval, seed = None))]
    fn ri(_cls: &Bound<'_, PyType>, mean_interval: f64, seed: Option<u64>) -> PyResult<Self> {
        Ok(Self::from_box(Box::new(
            schedules::RI::new(mean_interval, seed).map_err(map_err)?,
        )))
    }

    /// Wrap an armable interval schedule (FI/VI/RI) in a LimitedHold.
    ///
    /// Consumes ``inner``.
    #[classmethod]
    fn limited_hold(
        _cls: &Bound<'_, PyType>,
        inner: &mut PyArmableSchedule,
        hold: f64,
    ) -> PyResult<Self> {
        let stolen = inner.take()?;
        let lh = LimitedHold::new(stolen, hold).map_err(map_err)?;
        Ok(Self::from_box(Box::new(lh)))
    }

    // ------------------------------------------------------------------
    // Time-based family
    // ------------------------------------------------------------------

    #[classmethod]
    fn ft(_cls: &Bound<'_, PyType>, interval: f64) -> PyResult<Self> {
        Ok(Self::from_box(Box::new(
            schedules::FT::new(interval).map_err(map_err)?,
        )))
    }

    #[classmethod]
    #[pyo3(signature = (mean_interval, n_intervals = 12, seed = None))]
    fn vt(
        _cls: &Bound<'_, PyType>,
        mean_interval: f64,
        n_intervals: usize,
        seed: Option<u64>,
    ) -> PyResult<Self> {
        Ok(Self::from_box(Box::new(
            schedules::VT::new(mean_interval, n_intervals, seed).map_err(map_err)?,
        )))
    }

    #[classmethod]
    #[pyo3(signature = (mean_interval, seed = None))]
    fn rt(_cls: &Bound<'_, PyType>, mean_interval: f64, seed: Option<u64>) -> PyResult<Self> {
        Ok(Self::from_box(Box::new(
            schedules::RT::new(mean_interval, seed).map_err(map_err)?,
        )))
    }

    #[classmethod]
    fn ext(_cls: &Bound<'_, PyType>) -> PyResult<Self> {
        Ok(Self::from_box(Box::new(schedules::EXT::new())))
    }

    // ------------------------------------------------------------------
    // Differential family
    // ------------------------------------------------------------------

    #[classmethod]
    fn dro_resetting(_cls: &Bound<'_, PyType>, interval: f64) -> PyResult<Self> {
        Ok(Self::from_box(Box::new(
            schedules::DRO::new(interval, DroMode::Resetting).map_err(map_err)?,
        )))
    }

    #[classmethod]
    fn dro_momentary(_cls: &Bound<'_, PyType>, interval: f64) -> PyResult<Self> {
        Ok(Self::from_box(Box::new(
            schedules::DRO::new(interval, DroMode::Momentary).map_err(map_err)?,
        )))
    }

    #[classmethod]
    fn drl(_cls: &Bound<'_, PyType>, interval: f64) -> PyResult<Self> {
        Ok(Self::from_box(Box::new(
            schedules::DRL::new(interval).map_err(map_err)?,
        )))
    }

    #[classmethod]
    fn drh(_cls: &Bound<'_, PyType>, response_count: u32, time_window: f64) -> PyResult<Self> {
        Ok(Self::from_box(Box::new(
            schedules::DRH::new(response_count, time_window).map_err(map_err)?,
        )))
    }

    // ------------------------------------------------------------------
    // Compound: sequence-family (Multiple / Chained / Tandem)
    // ------------------------------------------------------------------

    /// Multiple compound schedule. Consumes each provided component.
    #[classmethod]
    #[pyo3(signature = (components, stimuli = None))]
    fn multiple(
        _cls: &Bound<'_, PyType>,
        components: Vec<PyRefMut<'_, PySchedule>>,
        stimuli: Option<Vec<String>>,
    ) -> PyResult<Self> {
        let inner = take_components(components)?;
        Ok(Self::from_box(Box::new(
            schedules::Multiple::new(inner, stimuli).map_err(map_err)?,
        )))
    }

    /// Chained compound schedule. Consumes each provided component.
    #[classmethod]
    #[pyo3(signature = (components, stimuli = None))]
    fn chained(
        _cls: &Bound<'_, PyType>,
        components: Vec<PyRefMut<'_, PySchedule>>,
        stimuli: Option<Vec<String>>,
    ) -> PyResult<Self> {
        let inner = take_components(components)?;
        Ok(Self::from_box(Box::new(
            schedules::Chained::new(inner, stimuli).map_err(map_err)?,
        )))
    }

    /// Tandem compound schedule. Consumes each provided component.
    #[classmethod]
    fn tandem(
        _cls: &Bound<'_, PyType>,
        components: Vec<PyRefMut<'_, PySchedule>>,
    ) -> PyResult<Self> {
        let inner = take_components(components)?;
        Ok(Self::from_box(Box::new(
            schedules::Tandem::new(inner).map_err(map_err)?,
        )))
    }

    // ------------------------------------------------------------------
    // Compound: Concurrent
    // ------------------------------------------------------------------

    /// Concurrent compound schedule keyed by operandum.
    ///
    /// ``components`` is a list of ``(operandum, schedule)`` tuples; each
    /// schedule is consumed. Insertion order is preserved.
    #[classmethod]
    #[pyo3(signature = (components, cod = 0.0, frco = 0))]
    fn concurrent(
        _cls: &Bound<'_, PyType>,
        components: Vec<(String, PyRefMut<'_, PySchedule>)>,
        cod: f64,
        frco: u32,
    ) -> PyResult<Self> {
        let mut map: IndexMap<String, Box<dyn Schedule>> =
            IndexMap::with_capacity(components.len());
        for (key, mut entry) in components {
            let stolen = take_inner(&mut entry)?;
            if map.insert(key.clone(), stolen).is_some() {
                return Err(PyValueError::new_err(format!(
                    "duplicate operandum key {key:?} in concurrent components"
                )));
            }
        }
        Ok(Self::from_box(Box::new(
            schedules::Concurrent::new(map, cod, frco).map_err(map_err)?,
        )))
    }

    // ------------------------------------------------------------------
    // Compound: Alternative (binary)
    // ------------------------------------------------------------------

    /// Alternative (whichever-first) compound schedule. Consumes both
    /// arguments.
    #[classmethod]
    fn alternative(
        _cls: &Bound<'_, PyType>,
        first: &mut PySchedule,
        second: &mut PySchedule,
    ) -> PyResult<Self> {
        let a = take_inner_mut(first)?;
        let b = take_inner_mut(second)?;
        Ok(Self::from_box(Box::new(schedules::Alternative::new(a, b))))
    }
}

// ---------------------------------------------------------------------------
// PyArmableSchedule — narrow wrapper for LimitedHold's inner argument
// ---------------------------------------------------------------------------

/// Python-facing handle to an armable interval schedule (FI/VI/RI).
///
/// The only reason this class exists is to give ``Schedule.limited_hold``
/// a type-safe argument. An ``ArmableSchedule`` can itself be stepped
/// and reset — it is a real schedule, just one with additional
/// introspection that ``LimitedHold`` relies on.
#[pyclass(name = "ArmableSchedule", unsendable)]
pub struct PyArmableSchedule {
    inner: Option<Box<dyn ArmableSchedule>>,
}

impl PyArmableSchedule {
    fn from_box(inner: Box<dyn ArmableSchedule>) -> Self {
        Self { inner: Some(inner) }
    }

    /// Take the inner schedule, leaving the wrapper in an empty state.
    fn take(&mut self) -> PyResult<Box<dyn ArmableSchedule>> {
        self.inner.take().ok_or_else(|| {
            PyRuntimeError::new_err(
                "ArmableSchedule has already been consumed by a compound schedule",
            )
        })
    }

    fn with_inner_mut<R>(&mut self, f: impl FnOnce(&mut dyn ArmableSchedule) -> R) -> PyResult<R> {
        let boxed = self.inner.as_deref_mut().ok_or_else(|| {
            PyRuntimeError::new_err(
                "ArmableSchedule has already been consumed by a compound schedule",
            )
        })?;
        Ok(f(boxed))
    }
}

#[pymethods]
impl PyArmableSchedule {
    #[pyo3(signature = (now, event = None))]
    fn step(&mut self, now: f64, event: Option<&PyResponseEvent>) -> PyResult<PyOutcome> {
        let ev = event.map(|e| e.inner.clone());
        let inner_result = self.with_inner_mut(|s| s.step(now, ev.as_ref()))?;
        let out = inner_result.map_err(map_err)?;
        Ok(PyOutcome { inner: out })
    }

    fn reset(&mut self) -> PyResult<()> {
        self.with_inner_mut(|s| s.reset())
    }

    #[classmethod]
    fn fi(_cls: &Bound<'_, PyType>, interval: f64) -> PyResult<Self> {
        Ok(Self::from_box(Box::new(
            schedules::FI::new(interval).map_err(map_err)?,
        )))
    }

    #[classmethod]
    #[pyo3(signature = (mean_interval, n_intervals = 12, seed = None))]
    fn vi(
        _cls: &Bound<'_, PyType>,
        mean_interval: f64,
        n_intervals: usize,
        seed: Option<u64>,
    ) -> PyResult<Self> {
        Ok(Self::from_box(Box::new(
            schedules::VI::new(mean_interval, n_intervals, seed).map_err(map_err)?,
        )))
    }

    #[classmethod]
    #[pyo3(signature = (mean_interval, seed = None))]
    fn ri(_cls: &Bound<'_, PyType>, mean_interval: f64, seed: Option<u64>) -> PyResult<Self> {
        Ok(Self::from_box(Box::new(
            schedules::RI::new(mean_interval, seed).map_err(map_err)?,
        )))
    }
}

// ---------------------------------------------------------------------------
// Helpers: drain PyRefMut<PySchedule> into owned boxes
// ---------------------------------------------------------------------------

fn take_inner_mut(owner: &mut PySchedule) -> PyResult<Box<dyn Schedule>> {
    // Replace the inner with a harmless stub so the Python-side handle
    // can still be dropped safely; re-using it will produce consistent
    // "empty" outcomes until the user drops it.
    let stub: Box<dyn Schedule> = Box::new(schedules::EXT::new());
    Ok(std::mem::replace(&mut owner.inner, stub))
}

fn take_inner(owner: &mut PyRefMut<'_, PySchedule>) -> PyResult<Box<dyn Schedule>> {
    take_inner_mut(owner)
}

fn take_components(items: Vec<PyRefMut<'_, PySchedule>>) -> PyResult<Vec<Box<dyn Schedule>>> {
    let mut out: Vec<Box<dyn Schedule>> = Vec::with_capacity(items.len());
    for mut entry in items {
        out.push(take_inner(&mut entry)?);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Progressive-ratio factory functions
// ---------------------------------------------------------------------------

fn pr_from_step_fn(step_fn: Box<dyn StepFn>) -> PySchedule {
    PySchedule::from_box(Box::new(ProgressiveRatio::new(step_fn)))
}

/// Progressive-ratio schedule with an arithmetic step function.
#[pyfunction]
#[pyo3(signature = (start = 1, step = 1))]
fn pr_arithmetic(start: u32, step: u32) -> PyResult<PySchedule> {
    let fn_ = schedules::arithmetic(start, step).map_err(map_err)?;
    Ok(pr_from_step_fn(fn_))
}

/// Progressive-ratio schedule with a geometric step function.
#[pyfunction]
#[pyo3(signature = (start = 1, ratio = 2.0))]
fn pr_geometric(start: u32, ratio: f64) -> PyResult<PySchedule> {
    let fn_ = schedules::geometric(start, ratio).map_err(map_err)?;
    Ok(pr_from_step_fn(fn_))
}

/// Progressive-ratio schedule using the Richardson-Roberts (1996) series.
#[pyfunction]
fn pr_richardson_roberts() -> PyResult<PySchedule> {
    let fn_ = schedules::richardson_roberts();
    Ok(pr_from_step_fn(fn_))
}

// ---------------------------------------------------------------------------
// Spec entry point
// ---------------------------------------------------------------------------

/// Parse a spec given as a JSON string or as a JSON-compatible Python
/// value (dict / list / str / int / float / bool / None).
fn spec_value_from_py(spec: &Bound<'_, PyAny>) -> PyResult<serde_json::Value> {
    let text: String = if spec.is_instance_of::<PyString>() {
        spec.extract()?
    } else {
        let json = spec.py().import_bound("json")?;
        let kwargs = PyDict::new_bound(spec.py());
        kwargs.set_item("allow_nan", false)?;
        json.call_method("dumps", (spec,), Some(&kwargs))
            .map_err(|e| {
                PyValueError::new_err(format!("schedule spec is not JSON-serialisable: {e}"))
            })?
            .extract()?
    };
    serde_json::from_str(&text)
        .map_err(|e| PyValueError::new_err(format!("schedule spec is not valid JSON: {e}")))
}

fn build_from_spec(spec: &Bound<'_, PyAny>) -> PyResult<PySchedule> {
    let value = spec_value_from_py(spec)?;
    let schedule = spec::from_spec(&value).map_err(map_err)?;
    Ok(PySchedule::from_box(schedule))
}

/// Build any of the spec types (see ``SPEC_TYPES``) from a spec
/// ``{"type": ..., "params": ...}`` given as a dict or a JSON string.
///
/// Raises ``ValueError`` when the spec is malformed or names an unknown
/// type.
#[pyfunction]
#[pyo3(name = "from_spec")]
fn py_from_spec(spec: &Bound<'_, PyAny>) -> PyResult<PySchedule> {
    build_from_spec(spec)
}

// ---------------------------------------------------------------------------
// Respondent Tier A expansion
// ---------------------------------------------------------------------------

// PyO3 0.22's `create_exception!` expansion tests a `gil-refs` cfg that
// this crate does not declare; the lint is silenced for that expansion only.
#[allow(unexpected_cfgs)]
mod exceptions {
    use pyo3::exceptions::PyValueError;

    pyo3::create_exception!(
        contingency_core,
        UnresolvedError,
        PyValueError,
        "A value a respondent element needs is supplied neither by the \
         expression nor by the options; ``missing`` names it."
    );
}
use exceptions::UnresolvedError;

/// Expand one Tier A respondent expression into its trial list.
///
/// ``spec`` is a contingency-dsl respondent AST node and ``options`` the
/// values left to the program (``trials``, ``seed``, ``labels``), each
/// given as a dict or a JSON string. Returns a list of trial dicts with
/// the keys ``stage``, ``cs``, ``us``, ``context``, ``order``, ``time``
/// and ``iti_s``.
///
/// Raises ``UnresolvedError`` (a ``ValueError``) with ``missing`` set
/// when a needed value is absent, and ``ValueError`` when ``spec`` is not
/// a Tier A node.
#[pyfunction]
#[pyo3(signature = (spec, options = None))]
fn expand_respondent<'py>(
    py: Python<'py>,
    spec: &Bound<'py, PyAny>,
    options: Option<&Bound<'py, PyAny>>,
) -> PyResult<Bound<'py, PyAny>> {
    let spec = spec_value_from_py(spec)?;
    let options = match options {
        Some(o) => spec_value_from_py(o)?,
        None => serde_json::Value::Null,
    };
    let to_py = |e: respondent::ExpandError| match e {
        respondent::ExpandError::Unresolved(missing) => {
            let err = UnresolvedError::new_err(format!("unresolved: {missing}"));
            match err.value_bound(py).setattr("missing", missing) {
                Ok(()) => err,
                Err(set_err) => set_err,
            }
        }
        respondent::ExpandError::Invalid(msg) => PyValueError::new_err(msg),
    };
    let options = respondent::ExpandOptions::from_value(&options).map_err(to_py)?;
    let trials = respondent::expand(&spec, &options).map_err(to_py)?;
    let text = serde_json::to_string(&trials)
        .map_err(|e| PyRuntimeError::new_err(format!("trial list not serialisable: {e}")))?;
    py.import_bound("json")?.call_method1("loads", (text,))
}

// ---------------------------------------------------------------------------
// Module entry
// ---------------------------------------------------------------------------

/// Python extension module — exposed by maturin / PyO3 as
/// ``contingency_core``.
#[pymodule]
fn contingency_core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyResponseEvent>()?;
    m.add_class::<PyReinforcer>()?;
    m.add_class::<PyOutcome>()?;
    m.add_class::<PySchedule>()?;
    m.add_class::<PyArmableSchedule>()?;
    m.add_function(wrap_pyfunction!(pr_arithmetic, m)?)?;
    m.add_function(wrap_pyfunction!(pr_geometric, m)?)?;
    m.add_function(wrap_pyfunction!(pr_richardson_roberts, m)?)?;
    m.add_function(wrap_pyfunction!(py_from_spec, m)?)?;
    m.add_function(wrap_pyfunction!(expand_respondent, m)?)?;
    m.add(
        "UnresolvedError",
        m.py().get_type_bound::<UnresolvedError>(),
    )?;
    m.add("SPEC_TYPES", PyTuple::new_bound(m.py(), spec::SPEC_TYPES))?;
    Ok(())
}

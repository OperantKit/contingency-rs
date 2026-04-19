//! UniFFI bindings for the contingency schedule engine.
//!
//! Exposes the public schedule runtime to Swift, Kotlin, and Kotlin
//! Multiplatform consumers via the UniFFI proc-macro interface. Only
//! compiled when the `uniffi` feature is enabled.
//!
//! Architectural notes:
//!
//! - Every schedule — simple, differential, progressive — is exposed as
//!   a single opaque [`UniffiSchedule`] object. Construction is via
//!   `#[uniffi::constructor]`-annotated static factory methods mirroring
//!   the factory API used by the PyO3 bindings in `crate::python`.
//! - The inner `Box<dyn Schedule + Send>` is wrapped in `std::sync::Mutex`
//!   so the exported object satisfies `Send + Sync`, which UniFFI
//!   requires for objects that cross the foreign-language boundary.
//! - Plain-data structs (`UniffiResponseEvent`, `UniffiReinforcer`,
//!   `UniffiOutcome`) cross the boundary by value.
//!
//! # Compound schedules
//!
//! Compound schedules (`Concurrent`, `Alternative`, `Multiple`,
//! `Chained`, `Tandem`) are fully exposed. The crate-level
//! [`crate::Schedule`] trait requires `Send`, which propagates through
//! the compound types' `Box<dyn Schedule>` fields and lets them sit
//! inside the `Mutex<SendableSchedule>` that UniFFI requires.
//!
//! ## Component ownership transfer
//!
//! UniFFI passes `Arc<Self>` arguments by reference. Compound
//! constructors take ownership of their components via the
//! [`UniffiSchedule::take_inner`] helper, which swaps the component's
//! inner `Box<dyn Schedule>` with a harmless `EXT` stub and returns the
//! original. Foreign-language callers that retain a reference to a
//! component after passing it to a compound constructor will observe
//! the stub `EXT` behaviour on that reference — the compound owns the
//! real schedule. This matches the consumption semantics of the PyO3
//! and wasm bindings.

use std::sync::{Arc, Mutex};

use crate::{
    schedule::ArmableSchedule,
    schedules::{self, DroMode, LimitedHold, ProgressiveRatio},
    ContingencyError, Outcome, Reinforcer, ResponseEvent, Schedule,
};

// ---------------------------------------------------------------------------
// Schedule trait-object aliases
// ---------------------------------------------------------------------------
//
// UniFFI's `uniffi::Object` requires `Send + Sync`. The crate-level
// `Schedule` trait has `Send` as a supertrait so every `Box<dyn
// Schedule>` is automatically `Send` — the aliases below simply name
// the boxed form for readability.

type SendableSchedule = Box<dyn Schedule>;
type SendableArmable = Box<dyn ArmableSchedule>;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Error type crossing the UniFFI boundary.
///
/// Mirrors [`crate::ContingencyError`] with variant data flattened to a
/// single message string so UniFFI can emit idiomatic sealed classes on
/// Kotlin / nested enums on Swift.
#[derive(Debug, thiserror::Error, uniffi::Error)]
#[uniffi(flat_error)]
pub enum UniffiContingencyError {
    /// Construction-time parameter validation failure.
    #[error("{0}")]
    Config(String),
    /// Runtime state violation.
    #[error("{0}")]
    State(String),
    /// Hardware Abstraction Layer I/O failure.
    #[error("{0}")]
    Hardware(String),
}

impl From<ContingencyError> for UniffiContingencyError {
    fn from(e: ContingencyError) -> Self {
        match e {
            ContingencyError::Config(m) => Self::Config(m),
            ContingencyError::State(m) => Self::State(m),
            ContingencyError::Hardware(m) => Self::Hardware(m),
        }
    }
}

// ---------------------------------------------------------------------------
// POD records
// ---------------------------------------------------------------------------

/// A response event at `time` on the named `operandum`.
#[derive(uniffi::Record, Clone, Debug)]
pub struct UniffiResponseEvent {
    /// Monotonic timestamp.
    pub time: f64,
    /// Operandum identifier (lever / key / button). `"main"` is the
    /// conventional default.
    pub operandum: String,
}

impl From<ResponseEvent> for UniffiResponseEvent {
    fn from(e: ResponseEvent) -> Self {
        Self {
            time: e.time,
            operandum: e.operandum,
        }
    }
}

impl From<&UniffiResponseEvent> for ResponseEvent {
    fn from(e: &UniffiResponseEvent) -> Self {
        ResponseEvent {
            time: e.time,
            operandum: e.operandum.clone(),
        }
    }
}

/// A delivered reinforcer crossing the UniFFI boundary.
#[derive(uniffi::Record, Clone, Debug)]
pub struct UniffiReinforcer {
    /// Delivery time (monotonic).
    pub time: f64,
    /// Delivery magnitude. Negative encodes punisher.
    pub magnitude: f64,
    /// Canonical `"SR+"` / `"SR-"` label.
    pub label: String,
}

impl From<Reinforcer> for UniffiReinforcer {
    fn from(r: Reinforcer) -> Self {
        Self {
            time: r.time,
            magnitude: r.magnitude,
            label: r.label,
        }
    }
}

/// Result of a [`UniffiSchedule::step`] call.
///
/// The `meta` map of [`crate::Outcome`] is not re-exposed here — the
/// Kotlin/Swift surface is the thin operational view and does not
/// currently need introspection into backend-specific payloads.
#[derive(uniffi::Record, Clone, Debug)]
pub struct UniffiOutcome {
    /// Whether the step produced a reinforcement delivery.
    pub reinforced: bool,
    /// The scheduled reinforcer when `reinforced == true`.
    pub reinforcer: Option<UniffiReinforcer>,
}

impl From<Outcome> for UniffiOutcome {
    fn from(o: Outcome) -> Self {
        Self {
            reinforced: o.reinforced,
            reinforcer: o.reinforcer.map(Into::into),
        }
    }
}

// ---------------------------------------------------------------------------
// Opaque schedule object
// ---------------------------------------------------------------------------

/// Foreign-language handle to any reinforcement schedule.
///
/// Construct via the `#[uniffi::constructor]` factory methods
/// (`UniffiSchedule::fr(5)`, `UniffiSchedule::fi(5.0)`, ...). Calling
/// [`UniffiSchedule::step`] advances the schedule to `now` and
/// optionally registers a response event.
#[derive(uniffi::Object)]
pub struct UniffiSchedule {
    inner: Mutex<SendableSchedule>,
}

impl UniffiSchedule {
    fn wrap<S: Schedule + Send + 'static>(s: S) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(Box::new(s)),
        })
    }

    /// Take the component's inner schedule, replacing it with a
    /// harmless `EXT` stub. Used by compound constructors to transfer
    /// ownership from a caller-held `Arc<UniffiSchedule>` into a
    /// compound wrapper. The caller's handle remains valid but will
    /// subsequently behave as `EXT` (never reinforce).
    fn take_inner(s: &Arc<Self>) -> SendableSchedule {
        let mut guard = s.inner.lock().expect("UniffiSchedule mutex poisoned");
        std::mem::replace(&mut *guard, Box::new(schedules::EXT::new()))
    }
}

// ---------------------------------------------------------------------------
// Instance methods
// ---------------------------------------------------------------------------

#[uniffi::export]
impl UniffiSchedule {
    /// Advance the schedule to `now` and optionally deliver a response.
    pub fn step(
        &self,
        now: f64,
        event: Option<UniffiResponseEvent>,
    ) -> Result<UniffiOutcome, UniffiContingencyError> {
        let mut guard = self.inner.lock().expect("UniffiSchedule mutex poisoned");
        let ev: Option<ResponseEvent> = event.as_ref().map(Into::into);
        let out = guard.step(now, ev.as_ref())?;
        Ok(out.into())
    }

    /// Return the schedule to its post-construction state.
    pub fn reset(&self) {
        let mut guard = self.inner.lock().expect("UniffiSchedule mutex poisoned");
        guard.reset();
    }
}

// ---------------------------------------------------------------------------
// Simple schedule factories
// ---------------------------------------------------------------------------

#[uniffi::export]
impl UniffiSchedule {
    // ---- Ratio family --------------------------------------------------

    /// Fixed Ratio (FR n).
    #[uniffi::constructor]
    pub fn fr(n: u64) -> Result<Arc<Self>, UniffiContingencyError> {
        Ok(Self::wrap(schedules::FR::new(n)?))
    }

    /// Continuous Reinforcement (FR 1).
    #[uniffi::constructor]
    pub fn crf() -> Arc<Self> {
        Self::wrap(schedules::crf())
    }

    /// Variable Ratio with a Fleshler–Hoffman sequence.
    #[uniffi::constructor]
    pub fn vr(
        mean: f64,
        n_intervals: u32,
        seed: Option<u64>,
    ) -> Result<Arc<Self>, UniffiContingencyError> {
        Ok(Self::wrap(schedules::VR::new(
            mean,
            n_intervals as usize,
            seed,
        )?))
    }

    /// Random Ratio.
    #[uniffi::constructor]
    pub fn rr(
        probability: f64,
        seed: Option<u64>,
    ) -> Result<Arc<Self>, UniffiContingencyError> {
        Ok(Self::wrap(schedules::RR::new(probability, seed)?))
    }

    // ---- Interval family -----------------------------------------------

    /// Fixed Interval.
    #[uniffi::constructor]
    pub fn fi(interval: f64) -> Result<Arc<Self>, UniffiContingencyError> {
        Ok(Self::wrap(schedules::FI::new(interval)?))
    }

    /// Variable Interval with a Fleshler–Hoffman sequence.
    #[uniffi::constructor]
    pub fn vi(
        mean_interval: f64,
        n_intervals: u32,
        seed: Option<u64>,
    ) -> Result<Arc<Self>, UniffiContingencyError> {
        Ok(Self::wrap(schedules::VI::new(
            mean_interval,
            n_intervals as usize,
            seed,
        )?))
    }

    /// Random Interval.
    #[uniffi::constructor]
    pub fn ri(
        mean_interval: f64,
        seed: Option<u64>,
    ) -> Result<Arc<Self>, UniffiContingencyError> {
        Ok(Self::wrap(schedules::RI::new(mean_interval, seed)?))
    }

    // ---- LimitedHold wrappers ------------------------------------------
    //
    // Exposed as three concrete pre-composed helpers keyed by the
    // armable-inner family (FI / VI / RI). This avoids the need for a
    // second UniFFI object type and dovetails with the factory API
    // style used elsewhere in the surface.

    /// Fixed-Interval with Limited Hold.
    #[uniffi::constructor]
    pub fn limited_hold_fi(
        interval: f64,
        hold: f64,
    ) -> Result<Arc<Self>, UniffiContingencyError> {
        let inner: SendableArmable = Box::new(schedules::FI::new(interval)?);
        Ok(Self::wrap(LimitedHold::new(inner, hold)?))
    }

    /// Variable-Interval with Limited Hold.
    #[uniffi::constructor]
    pub fn limited_hold_vi(
        mean_interval: f64,
        n_intervals: u32,
        hold: f64,
        seed: Option<u64>,
    ) -> Result<Arc<Self>, UniffiContingencyError> {
        let inner: SendableArmable =
            Box::new(schedules::VI::new(mean_interval, n_intervals as usize, seed)?);
        Ok(Self::wrap(LimitedHold::new(inner, hold)?))
    }

    /// Random-Interval with Limited Hold.
    #[uniffi::constructor]
    pub fn limited_hold_ri(
        mean_interval: f64,
        hold: f64,
        seed: Option<u64>,
    ) -> Result<Arc<Self>, UniffiContingencyError> {
        let inner: SendableArmable = Box::new(schedules::RI::new(mean_interval, seed)?);
        Ok(Self::wrap(LimitedHold::new(inner, hold)?))
    }

    // ---- Time-based family ---------------------------------------------

    /// Fixed Time (non-contingent).
    #[uniffi::constructor]
    pub fn ft(interval: f64) -> Result<Arc<Self>, UniffiContingencyError> {
        Ok(Self::wrap(schedules::FT::new(interval)?))
    }

    /// Variable Time with a Fleshler–Hoffman sequence.
    #[uniffi::constructor]
    pub fn vt(
        mean_interval: f64,
        n_intervals: u32,
        seed: Option<u64>,
    ) -> Result<Arc<Self>, UniffiContingencyError> {
        Ok(Self::wrap(schedules::VT::new(
            mean_interval,
            n_intervals as usize,
            seed,
        )?))
    }

    /// Random Time.
    #[uniffi::constructor]
    pub fn rt(
        mean_interval: f64,
        seed: Option<u64>,
    ) -> Result<Arc<Self>, UniffiContingencyError> {
        Ok(Self::wrap(schedules::RT::new(mean_interval, seed)?))
    }

    /// Extinction.
    #[uniffi::constructor]
    pub fn ext() -> Arc<Self> {
        Self::wrap(schedules::EXT::new())
    }

    // ---- Differential family -------------------------------------------

    /// Differential Reinforcement of Other behavior — resetting variant.
    #[uniffi::constructor]
    pub fn dro_resetting(interval: f64) -> Result<Arc<Self>, UniffiContingencyError> {
        Ok(Self::wrap(schedules::DRO::new(
            interval,
            DroMode::Resetting,
        )?))
    }

    /// Differential Reinforcement of Other behavior — momentary variant.
    #[uniffi::constructor]
    pub fn dro_momentary(interval: f64) -> Result<Arc<Self>, UniffiContingencyError> {
        Ok(Self::wrap(schedules::DRO::new(
            interval,
            DroMode::Momentary,
        )?))
    }

    /// Differential Reinforcement of Low rates.
    #[uniffi::constructor]
    pub fn drl(interval: f64) -> Result<Arc<Self>, UniffiContingencyError> {
        Ok(Self::wrap(schedules::DRL::new(interval)?))
    }

    /// Differential Reinforcement of High rates.
    #[uniffi::constructor]
    pub fn drh(
        response_count: u32,
        time_window: f64,
    ) -> Result<Arc<Self>, UniffiContingencyError> {
        Ok(Self::wrap(schedules::DRH::new(response_count, time_window)?))
    }

    // ---- Progressive Ratio ---------------------------------------------

    /// Progressive Ratio with an arithmetic step function
    /// (`r_n = start + n * step`).
    #[uniffi::constructor]
    pub fn pr_arithmetic(
        start: u32,
        step: u32,
    ) -> Result<Arc<Self>, UniffiContingencyError> {
        let step_fn = schedules::arithmetic(start, step)?;
        Ok(Self::wrap(ProgressiveRatio::new(step_fn)))
    }

    /// Progressive Ratio with a geometric step function
    /// (`r_n = round(start * ratio.powi(n))`).
    #[uniffi::constructor]
    pub fn pr_geometric(
        start: u32,
        ratio: f64,
    ) -> Result<Arc<Self>, UniffiContingencyError> {
        let step_fn = schedules::geometric(start, ratio)?;
        Ok(Self::wrap(ProgressiveRatio::new(step_fn)))
    }

    /// Progressive Ratio using the Richardson-Roberts (1996) series.
    #[uniffi::constructor]
    pub fn pr_richardson_roberts() -> Arc<Self> {
        let step_fn = schedules::richardson_roberts();
        Self::wrap(ProgressiveRatio::new(step_fn))
    }

    // ---- Compound schedules -------------------------------------------
    //
    // Each compound constructor consumes its component arguments via
    // `take_inner` — after the call, foreign-language references to the
    // original components behave as `EXT` (never reinforce). See the
    // module-level "Component ownership transfer" note.

    /// Alternative composer: reinforces whichever of `first` / `second`
    /// fires first on any given step. Strictly binary — nest calls for
    /// three or more branches: `Alternative(Alternative(a, b), c)`.
    #[uniffi::constructor]
    pub fn alternative(first: Arc<Self>, second: Arc<Self>) -> Arc<Self> {
        let f = Self::take_inner(&first);
        let s = Self::take_inner(&second);
        Self::wrap(schedules::Alternative::new(f, s))
    }

    /// Multiple schedule: `components` alternate as the active
    /// component on reinforcement, each presented under the
    /// corresponding discriminative stimulus in `stimuli`. If
    /// `stimuli` is `None`, default names `comp_0`, `comp_1`, ... are
    /// used. Requires at least 2 components.
    #[uniffi::constructor]
    pub fn multiple(
        components: Vec<Arc<Self>>,
        stimuli: Option<Vec<String>>,
    ) -> Result<Arc<Self>, UniffiContingencyError> {
        let inner: Vec<SendableSchedule> = components.iter().map(Self::take_inner).collect();
        Ok(Self::wrap(schedules::Multiple::new(inner, stimuli)?))
    }

    /// Chained schedule: `components` form a chain in which only the
    /// terminal component delivers primary reinforcement; non-terminal
    /// "reinforcements" advance the active component and are reported
    /// as chain transitions on the returned `Outcome.meta`.
    #[uniffi::constructor]
    pub fn chained(
        components: Vec<Arc<Self>>,
        stimuli: Option<Vec<String>>,
    ) -> Result<Arc<Self>, UniffiContingencyError> {
        let inner: Vec<SendableSchedule> = components.iter().map(Self::take_inner).collect();
        Ok(Self::wrap(schedules::Chained::new(inner, stimuli)?))
    }

    /// Tandem schedule: like `Chained` but without distinctive stimuli.
    /// `meta["current_component"]` on each `Outcome` is reported as an
    /// integer link index rather than a string.
    #[uniffi::constructor]
    pub fn tandem(
        components: Vec<Arc<Self>>,
    ) -> Result<Arc<Self>, UniffiContingencyError> {
        let inner: Vec<SendableSchedule> = components.iter().map(Self::take_inner).collect();
        Ok(Self::wrap(schedules::Tandem::new(inner)?))
    }

    /// Concurrent schedule with Changeover Delay (COD) and optional
    /// Changeover Ratio (COR). `operanda` and `components` are
    /// parallel arrays and must have the same length. `cod` is the
    /// post-changeover lockout duration in the same time unit as
    /// `step`'s `now`; `cor` is the number of consecutive responses on
    /// the new operandum required to confirm a changeover (0 = every
    /// switch counts immediately).
    ///
    /// References: Catania, A. C. (1966); Herrnstein, R. J. (1961).
    #[uniffi::constructor]
    pub fn concurrent(
        operanda: Vec<String>,
        components: Vec<Arc<Self>>,
        cod: f64,
        cor: u32,
    ) -> Result<Arc<Self>, UniffiContingencyError> {
        if operanda.len() != components.len() {
            return Err(UniffiContingencyError::Config(
                "concurrent: operanda and components length mismatch".into(),
            ));
        }
        let mut map = indexmap::IndexMap::new();
        for (name, comp) in operanda.into_iter().zip(components.iter()) {
            map.insert(name, Self::take_inner(comp));
        }
        Ok(Self::wrap(schedules::Concurrent::new(map, cod, cor)?))
    }
}

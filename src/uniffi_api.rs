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
//! `Chained`, `Tandem`) are intentionally **not** exposed in this first
//! UniFFI surface. Their crate-level struct definitions store
//! `Box<dyn Schedule>` internally without a `Send` bound, which makes
//! the compound type itself `!Send` and therefore unable to sit inside
//! the `Mutex<...>` that `uniffi::Object` requires. The two viable
//! paths for adding compound support are tracked as follow-up work:
//!
//! 1. Introduce `+ Send` on the internal `Box<dyn Schedule>` fields of
//!    the compound schedules (or on the [`crate::Schedule`] trait
//!    itself). This is a non-breaking change for existing consumers
//!    because every current concrete schedule implementation is
//!    `Send`, but it must be coordinated across the wasm / FFI / PyO3
//!    bindings.
//! 2. Add a dedicated `CompoundBuilder` UniFFI object whose internal
//!    storage is Send-bounded from the start and which owns its
//!    component schedules outright (no shared `Arc` refs crossing the
//!    boundary twice). This avoids touching the existing compound
//!    struct definitions.
//!
//! Either approach is non-trivial and out of scope for this pass.

use std::sync::{Arc, Mutex};

use crate::{
    schedule::ArmableSchedule,
    schedules::{self, DroMode, LimitedHold, ProgressiveRatio},
    ContingencyError, Outcome, Reinforcer, ResponseEvent, Schedule,
};

// ---------------------------------------------------------------------------
// Send-bounded schedule trait alias
// ---------------------------------------------------------------------------
//
// UniFFI's `uniffi::Object` requires `Send + Sync`. The crate-level
// `Schedule` trait is intentionally unconstrained so that the wasm /
// PyO3 bindings can use non-`Send` schedule objects freely. All current
// concrete, non-compound schedule implementations happen to be `Send`,
// so we bound the trait object locally here without touching the trait
// itself.

type SendableSchedule = Box<dyn Schedule + Send>;
type SendableArmable = Box<dyn ArmableSchedule + Send>;

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
}

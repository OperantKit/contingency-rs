//! C ABI surface for the contingency schedule engine.
//!
//! Every schedule is exposed as an opaque pointer (`OpkSchedule`).
//! Constructors return owned pointers; callers must release them with
//! [`opk_schedule_free`]. Step/reset operations return `0` on success
//! and non-zero on error; error messages are retrieved with
//! [`opk_last_error_message`] from a thread-local slot.
//!
//! Ownership conventions:
//!
//! * Simple-schedule constructors (`opk_fr`, `opk_fi`, `opk_crf`, …)
//!   return a freshly heap-allocated handle owned by the caller.
//! * Compound-schedule constructors (`opk_alternative`, `opk_multiple`,
//!   `opk_chained`, `opk_tandem`, `opk_concurrent`, `opk_limited_hold_*`)
//!   **take ownership** of the handles passed in. After such a call the
//!   component handles MUST NOT be freed, stepped, or reset separately;
//!   calling [`opk_schedule_free`] on the compound releases the whole
//!   tree.
//! * Strings passed in (e.g. `event_operandum`, `concurrent` operandum
//!   keys) are borrowed only for the duration of the call and copied
//!   internally where needed. Strings returned via `OpkOutcome`
//!   (`reinforcer_label`) are borrowed from the schedule handle and
//!   remain valid until the next call to [`opk_schedule_step`] on that
//!   handle (or until the handle is freed).

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};

use indexmap::IndexMap;

use crate::{
    schedule::ArmableSchedule,
    schedules::{self, DroMode, LimitedHold, ProgressiveRatio},
    ContingencyError, Outcome, ResponseEvent, Schedule,
};

// -----------------------------------------------------------------------
// Thread-local error channel
// -----------------------------------------------------------------------

thread_local! {
    static LAST_ERROR: std::cell::RefCell<Option<CString>> =
        const { std::cell::RefCell::new(None) };
}

fn set_last_error(e: impl std::fmt::Display) {
    let s = CString::new(e.to_string()).unwrap_or_else(|_| {
        CString::new("error (contained interior NUL)").expect("static ASCII is valid")
    });
    LAST_ERROR.with(|cell| *cell.borrow_mut() = Some(s));
}

/// Return a pointer to the last error message for this thread, or NULL
/// if no error has been recorded. The returned pointer is valid until
/// the next FFI call that sets an error on this thread and must not be
/// freed by the caller.
#[no_mangle]
pub extern "C" fn opk_last_error_message() -> *const c_char {
    LAST_ERROR.with(|cell| {
        cell.borrow()
            .as_ref()
            .map(|s| s.as_ptr())
            .unwrap_or(std::ptr::null())
    })
}

/// Clear the thread-local error channel.
#[no_mangle]
pub extern "C" fn opk_clear_last_error() {
    LAST_ERROR.with(|cell| *cell.borrow_mut() = None);
}

// -----------------------------------------------------------------------
// Opaque handle
// -----------------------------------------------------------------------

/// Opaque handle to a heap-allocated schedule.
///
/// Construct with any `opk_*` schedule constructor, step with
/// [`opk_schedule_step`], and release with [`opk_schedule_free`].
pub struct OpkSchedule {
    inner: Box<dyn Schedule>,
    /// Last-step reinforcer label, kept alive so the raw pointer
    /// exposed in [`OpkOutcome::reinforcer_label`] stays valid until
    /// the next step.
    last_label: Option<CString>,
}

/// Opaque handle to an armable interval schedule (FI / VI / RI).
///
/// Only used as an intermediary for constructing limited-hold
/// compounds; do not step or reset directly. Consumed by
/// [`opk_limited_hold_fi`], [`opk_limited_hold_vi`],
/// [`opk_limited_hold_ri`] (which take ownership).
pub struct OpkArmableSchedule {
    inner: Box<dyn ArmableSchedule>,
}

fn box_schedule<S: Schedule + 'static>(s: S) -> *mut OpkSchedule {
    Box::into_raw(Box::new(OpkSchedule {
        inner: Box::new(s),
        last_label: None,
    }))
}

fn box_schedule_dyn(inner: Box<dyn Schedule>) -> *mut OpkSchedule {
    Box::into_raw(Box::new(OpkSchedule {
        inner,
        last_label: None,
    }))
}

fn box_armable<S: ArmableSchedule + 'static>(s: S) -> *mut OpkArmableSchedule {
    Box::into_raw(Box::new(OpkArmableSchedule { inner: Box::new(s) }))
}

/// Release a schedule handle. Safe to call with NULL.
///
/// # Safety
///
/// `handle` must be either NULL or a pointer previously returned by an
/// `opk_*` schedule constructor and not yet freed.
#[no_mangle]
pub unsafe extern "C" fn opk_schedule_free(handle: *mut OpkSchedule) {
    if !handle.is_null() {
        // SAFETY: caller's contract asserts `handle` was produced by
        // `Box::into_raw` via an `opk_*` constructor and has not been
        // freed yet.
        unsafe { drop(Box::from_raw(handle)) };
    }
}

/// Release an armable schedule handle. Safe to call with NULL.
///
/// # Safety
///
/// `handle` must be either NULL or a pointer previously returned by an
/// `opk_armable_*` constructor and not yet freed or consumed by a
/// limited-hold constructor.
#[no_mangle]
pub unsafe extern "C" fn opk_armable_schedule_free(handle: *mut OpkArmableSchedule) {
    if !handle.is_null() {
        // SAFETY: caller's contract asserts `handle` was produced by
        // `Box::into_raw` via an `opk_armable_*` constructor and has
        // not been freed/consumed yet.
        unsafe { drop(Box::from_raw(handle)) };
    }
}

// -----------------------------------------------------------------------
// Outcome POD
// -----------------------------------------------------------------------

/// Plain-old-data mirror of [`crate::Outcome`] for C consumers.
///
/// Only the `reinforced` flag is meaningful when no reinforcer was
/// delivered; the remaining fields are zero-initialised in that case.
/// `reinforcer_label`, when non-NULL, is borrowed from the schedule
/// handle and remains valid until the next [`opk_schedule_step`] call
/// on that handle or until the handle is freed.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct OpkOutcome {
    /// Whether the step produced a reinforcement delivery.
    pub reinforced: bool,
    /// Delivery time. Valid only when `reinforced` is `true`.
    pub reinforcer_time: f64,
    /// Delivery magnitude. Valid only when `reinforced` is `true`.
    pub reinforcer_magnitude: f64,
    /// NUL-terminated reinforcer label. NULL when `reinforced` is
    /// `false`. Otherwise valid until the next `opk_schedule_step` call
    /// on the same schedule handle. Caller must NOT free.
    pub reinforcer_label: *const c_char,
}

impl OpkOutcome {
    fn empty() -> Self {
        Self {
            reinforced: false,
            reinforcer_time: 0.0,
            reinforcer_magnitude: 0.0,
            reinforcer_label: std::ptr::null(),
        }
    }
}

// -----------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------

fn handle_error<T>(r: std::result::Result<T, ContingencyError>) -> Option<T> {
    match r {
        Ok(v) => Some(v),
        Err(e) => {
            set_last_error(e);
            None
        }
    }
}

/// SAFETY: caller guarantees that `ptr` is either NULL or a
/// NUL-terminated UTF-8 string valid for the duration of the borrow.
unsafe fn cstr_to_string(ptr: *const c_char) -> std::result::Result<String, &'static str> {
    if ptr.is_null() {
        return Err("null string pointer");
    }
    // SAFETY: caller contract asserts `ptr` is a valid NUL-terminated
    // C string for the duration of this call.
    let bytes = unsafe { CStr::from_ptr(ptr) }.to_bytes();
    std::str::from_utf8(bytes)
        .map(|s| s.to_owned())
        .map_err(|_| "invalid UTF-8 in string argument")
}

fn write_outcome(handle: &mut OpkSchedule, outcome: &Outcome, out: &mut OpkOutcome) {
    if let Some(r) = outcome.reinforcer.as_ref() {
        let label = CString::new(r.label.as_str()).unwrap_or_else(|_| {
            CString::new("<invalid label>").expect("static ASCII is valid")
        });
        let ptr = label.as_ptr();
        handle.last_label = Some(label);
        *out = OpkOutcome {
            reinforced: true,
            reinforcer_time: r.time,
            reinforcer_magnitude: r.magnitude,
            reinforcer_label: ptr,
        };
    } else {
        handle.last_label = None;
        *out = OpkOutcome::empty();
    }
}

// -----------------------------------------------------------------------
// Step / reset
// -----------------------------------------------------------------------

/// Step a schedule. Returns `0` on success, non-zero on error.
///
/// * `now` — monotonic clock time. Must be `>=` the previous step's
///   `now` within [`crate::TIME_TOL`].
/// * `has_event` — when `true`, a response event is supplied with
///   `event_time` and `event_operandum`. `event_operandum` must be a
///   valid NUL-terminated UTF-8 string for the duration of this call.
///   When `has_event` is `false`, `event_time` and `event_operandum`
///   are ignored.
/// * `out` — receives the outcome; must point to a writable
///   [`OpkOutcome`]. `reinforcer_label` in the written outcome borrows
///   memory from the schedule handle and remains valid until the next
///   step call on this handle.
///
/// On error, `*out` is set to an empty (non-reinforced) outcome.
/// Retrieve the error message via [`opk_last_error_message`].
///
/// # Safety
///
/// * `handle` must be a non-null pointer returned by an `opk_*`
///   constructor and not yet freed.
/// * `out` must be a non-null, suitably-aligned pointer to a writable
///   [`OpkOutcome`].
/// * When `has_event` is `true`, `event_operandum` must be a valid
///   NUL-terminated UTF-8 string valid for the duration of the call.
#[no_mangle]
pub unsafe extern "C" fn opk_schedule_step(
    handle: *mut OpkSchedule,
    now: f64,
    has_event: bool,
    event_time: f64,
    event_operandum: *const c_char,
    out: *mut OpkOutcome,
) -> c_int {
    if handle.is_null() {
        set_last_error("opk_schedule_step: null handle");
        return 1;
    }
    if out.is_null() {
        set_last_error("opk_schedule_step: null out");
        return 1;
    }

    // SAFETY: caller asserts `handle` is a live `OpkSchedule`; no
    // aliased references exist because the C caller owns the pointer
    // and passes it exclusively for this call.
    let wrapper = unsafe { &mut *handle };
    // SAFETY: caller asserts `out` is a valid writable `OpkOutcome`.
    let out_ref = unsafe { &mut *out };

    let event = if has_event {
        // SAFETY: caller asserts `event_operandum` is a valid
        // NUL-terminated UTF-8 string when `has_event` is true.
        let op = match unsafe { cstr_to_string(event_operandum) } {
            Ok(s) => s,
            Err(e) => {
                set_last_error(format!("opk_schedule_step: event_operandum: {e}"));
                *out_ref = OpkOutcome::empty();
                return 1;
            }
        };
        Some(ResponseEvent {
            time: event_time,
            operandum: op,
        })
    } else {
        None
    };

    match wrapper.inner.step(now, event.as_ref()) {
        Ok(outcome) => {
            write_outcome(wrapper, &outcome, out_ref);
            0
        }
        Err(e) => {
            set_last_error(e);
            *out_ref = OpkOutcome::empty();
            wrapper.last_label = None;
            1
        }
    }
}

/// Reset a schedule to its post-construction state.
///
/// # Safety
///
/// `handle` must be a non-null pointer returned by an `opk_*`
/// constructor and not yet freed.
#[no_mangle]
pub unsafe extern "C" fn opk_schedule_reset(handle: *mut OpkSchedule) -> c_int {
    if handle.is_null() {
        set_last_error("opk_schedule_reset: null handle");
        return 1;
    }
    // SAFETY: caller asserts `handle` is a live `OpkSchedule`.
    let wrapper = unsafe { &mut *handle };
    wrapper.inner.reset();
    wrapper.last_label = None;
    0
}

// -----------------------------------------------------------------------
// Ratio-family constructors
// -----------------------------------------------------------------------

/// Fixed-Ratio schedule with requirement `n`.
/// Returns NULL on failure; retrieve the error via
/// [`opk_last_error_message`].
#[no_mangle]
pub extern "C" fn opk_fr(n: u64) -> *mut OpkSchedule {
    match handle_error(schedules::FR::new(n)) {
        Some(s) => box_schedule(s),
        None => std::ptr::null_mut(),
    }
}

/// Continuous Reinforcement (FR 1).
#[no_mangle]
pub extern "C" fn opk_crf() -> *mut OpkSchedule {
    box_schedule(schedules::crf())
}

/// Variable-Ratio schedule (Fleshler-Hoffman progression).
///
/// * `mean` — mean ratio requirement.
/// * `n_intervals` — number of values in the pre-generated sequence.
/// * `has_seed` — when `true`, use `seed`; when `false`, sample a
///   non-deterministic seed.
#[no_mangle]
pub extern "C" fn opk_vr(
    mean: f64,
    n_intervals: usize,
    has_seed: bool,
    seed: u64,
) -> *mut OpkSchedule {
    let s = if has_seed { Some(seed) } else { None };
    match handle_error(schedules::VR::new(mean, n_intervals, s)) {
        Some(v) => box_schedule(v),
        None => std::ptr::null_mut(),
    }
}

/// Random-Ratio schedule (Bernoulli per response).
#[no_mangle]
pub extern "C" fn opk_rr(probability: f64, has_seed: bool, seed: u64) -> *mut OpkSchedule {
    let s = if has_seed { Some(seed) } else { None };
    match handle_error(schedules::RR::new(probability, s)) {
        Some(v) => box_schedule(v),
        None => std::ptr::null_mut(),
    }
}

// -----------------------------------------------------------------------
// Interval-family constructors
// -----------------------------------------------------------------------

/// Fixed-Interval schedule with `interval` time units.
#[no_mangle]
pub extern "C" fn opk_fi(interval: f64) -> *mut OpkSchedule {
    match handle_error(schedules::FI::new(interval)) {
        Some(s) => box_schedule(s),
        None => std::ptr::null_mut(),
    }
}

/// Variable-Interval schedule (Fleshler-Hoffman progression).
#[no_mangle]
pub extern "C" fn opk_vi(
    mean_interval: f64,
    n_intervals: usize,
    has_seed: bool,
    seed: u64,
) -> *mut OpkSchedule {
    let s = if has_seed { Some(seed) } else { None };
    match handle_error(schedules::VI::new(mean_interval, n_intervals, s)) {
        Some(v) => box_schedule(v),
        None => std::ptr::null_mut(),
    }
}

/// Random-Interval schedule (exponential).
#[no_mangle]
pub extern "C" fn opk_ri(
    mean_interval: f64,
    has_seed: bool,
    seed: u64,
) -> *mut OpkSchedule {
    let s = if has_seed { Some(seed) } else { None };
    match handle_error(schedules::RI::new(mean_interval, s)) {
        Some(v) => box_schedule(v),
        None => std::ptr::null_mut(),
    }
}

// -----------------------------------------------------------------------
// Armable interval constructors (for LimitedHold)
// -----------------------------------------------------------------------

/// FI armable-schedule handle (only for use with `opk_limited_hold_fi`).
#[no_mangle]
pub extern "C" fn opk_armable_fi(interval: f64) -> *mut OpkArmableSchedule {
    match handle_error(schedules::FI::new(interval)) {
        Some(s) => box_armable(s),
        None => std::ptr::null_mut(),
    }
}

/// VI armable-schedule handle.
#[no_mangle]
pub extern "C" fn opk_armable_vi(
    mean_interval: f64,
    n_intervals: usize,
    has_seed: bool,
    seed: u64,
) -> *mut OpkArmableSchedule {
    let s = if has_seed { Some(seed) } else { None };
    match handle_error(schedules::VI::new(mean_interval, n_intervals, s)) {
        Some(v) => box_armable(v),
        None => std::ptr::null_mut(),
    }
}

/// RI armable-schedule handle.
#[no_mangle]
pub extern "C" fn opk_armable_ri(
    mean_interval: f64,
    has_seed: bool,
    seed: u64,
) -> *mut OpkArmableSchedule {
    let s = if has_seed { Some(seed) } else { None };
    match handle_error(schedules::RI::new(mean_interval, s)) {
        Some(v) => box_armable(v),
        None => std::ptr::null_mut(),
    }
}

/// Wrap an FI schedule with a limited-hold window. Takes ownership of
/// `inner`; on success the inner handle must NOT be freed separately.
/// On failure, `inner` is freed by this function.
///
/// # Safety
///
/// `inner` must be a valid pointer previously returned by
/// [`opk_armable_fi`] and not yet freed or consumed.
#[no_mangle]
pub unsafe extern "C" fn opk_limited_hold_fi(
    inner: *mut OpkArmableSchedule,
    hold: f64,
) -> *mut OpkSchedule {
    limited_hold_consume(inner, hold)
}

/// Wrap a VI schedule with a limited-hold window.
///
/// # Safety
///
/// `inner` must be a valid pointer previously returned by
/// [`opk_armable_vi`] and not yet freed or consumed.
#[no_mangle]
pub unsafe extern "C" fn opk_limited_hold_vi(
    inner: *mut OpkArmableSchedule,
    hold: f64,
) -> *mut OpkSchedule {
    limited_hold_consume(inner, hold)
}

/// Wrap an RI schedule with a limited-hold window.
///
/// # Safety
///
/// `inner` must be a valid pointer previously returned by
/// [`opk_armable_ri`] and not yet freed or consumed.
#[no_mangle]
pub unsafe extern "C" fn opk_limited_hold_ri(
    inner: *mut OpkArmableSchedule,
    hold: f64,
) -> *mut OpkSchedule {
    limited_hold_consume(inner, hold)
}

/// Shared implementation for the three `opk_limited_hold_*` flavours.
///
/// # Safety
///
/// `inner` must be a valid `OpkArmableSchedule` pointer and not yet
/// freed or consumed.
unsafe fn limited_hold_consume(
    inner: *mut OpkArmableSchedule,
    hold: f64,
) -> *mut OpkSchedule {
    if inner.is_null() {
        set_last_error("opk_limited_hold_*: null inner handle");
        return std::ptr::null_mut();
    }
    // SAFETY: caller asserts `inner` was produced by an
    // `opk_armable_*` constructor and is not yet freed/consumed.
    let wrapper = unsafe { Box::from_raw(inner) };
    let stolen = wrapper.inner;
    match handle_error(LimitedHold::new(stolen, hold)) {
        Some(lh) => box_schedule_dyn(Box::new(lh)),
        None => std::ptr::null_mut(),
    }
}

// -----------------------------------------------------------------------
// Time-based family
// -----------------------------------------------------------------------

/// Fixed-Time schedule (non-contingent).
#[no_mangle]
pub extern "C" fn opk_ft(interval: f64) -> *mut OpkSchedule {
    match handle_error(schedules::FT::new(interval)) {
        Some(s) => box_schedule(s),
        None => std::ptr::null_mut(),
    }
}

/// Variable-Time schedule (Fleshler-Hoffman).
#[no_mangle]
pub extern "C" fn opk_vt(
    mean_interval: f64,
    n_intervals: usize,
    has_seed: bool,
    seed: u64,
) -> *mut OpkSchedule {
    let s = if has_seed { Some(seed) } else { None };
    match handle_error(schedules::VT::new(mean_interval, n_intervals, s)) {
        Some(v) => box_schedule(v),
        None => std::ptr::null_mut(),
    }
}

/// Random-Time schedule (exponential).
#[no_mangle]
pub extern "C" fn opk_rt(
    mean_interval: f64,
    has_seed: bool,
    seed: u64,
) -> *mut OpkSchedule {
    let s = if has_seed { Some(seed) } else { None };
    match handle_error(schedules::RT::new(mean_interval, s)) {
        Some(v) => box_schedule(v),
        None => std::ptr::null_mut(),
    }
}

/// Extinction schedule (never reinforces).
#[no_mangle]
pub extern "C" fn opk_ext() -> *mut OpkSchedule {
    box_schedule(schedules::EXT::new())
}

// -----------------------------------------------------------------------
// Differential-reinforcement family
// -----------------------------------------------------------------------

/// Differential Reinforcement of Other behaviour — resetting timer.
#[no_mangle]
pub extern "C" fn opk_dro_resetting(interval: f64) -> *mut OpkSchedule {
    match handle_error(schedules::DRO::new(interval, DroMode::Resetting)) {
        Some(s) => box_schedule(s),
        None => std::ptr::null_mut(),
    }
}

/// Differential Reinforcement of Other behaviour — momentary (whole-interval).
#[no_mangle]
pub extern "C" fn opk_dro_momentary(interval: f64) -> *mut OpkSchedule {
    match handle_error(schedules::DRO::new(interval, DroMode::Momentary)) {
        Some(s) => box_schedule(s),
        None => std::ptr::null_mut(),
    }
}

/// Differential Reinforcement of Low rates.
#[no_mangle]
pub extern "C" fn opk_drl(interval: f64) -> *mut OpkSchedule {
    match handle_error(schedules::DRL::new(interval)) {
        Some(s) => box_schedule(s),
        None => std::ptr::null_mut(),
    }
}

/// Differential Reinforcement of High rates.
#[no_mangle]
pub extern "C" fn opk_drh(response_count: u32, time_window: f64) -> *mut OpkSchedule {
    match handle_error(schedules::DRH::new(response_count, time_window)) {
        Some(s) => box_schedule(s),
        None => std::ptr::null_mut(),
    }
}

// -----------------------------------------------------------------------
// Progressive-ratio constructors
// -----------------------------------------------------------------------

/// Progressive-Ratio schedule with arithmetic step.
#[no_mangle]
pub extern "C" fn opk_pr_arithmetic(start: u32, step: u32) -> *mut OpkSchedule {
    match handle_error(schedules::arithmetic(start, step)) {
        Some(step_fn) => box_schedule(ProgressiveRatio::new(step_fn)),
        None => std::ptr::null_mut(),
    }
}

/// Progressive-Ratio schedule with geometric step.
#[no_mangle]
pub extern "C" fn opk_pr_geometric(start: u32, ratio: f64) -> *mut OpkSchedule {
    match handle_error(schedules::geometric(start, ratio)) {
        Some(step_fn) => box_schedule(ProgressiveRatio::new(step_fn)),
        None => std::ptr::null_mut(),
    }
}

/// Progressive-Ratio schedule using the Richardson-Roberts (1996) series.
#[no_mangle]
pub extern "C" fn opk_pr_richardson_roberts() -> *mut OpkSchedule {
    let step_fn = schedules::richardson_roberts();
    box_schedule(ProgressiveRatio::new(step_fn))
}

// -----------------------------------------------------------------------
// Compound: Alternative
// -----------------------------------------------------------------------

/// Construct Alternative(first, second). Takes ownership of both
/// handles — caller MUST NOT free them separately. Handles remain
/// valid for the lifetime of the returned Alternative handle;
/// `opk_schedule_free` on the returned handle drops all nested
/// schedules.
///
/// Returns NULL and sets the thread-local error when either handle is
/// NULL. In the NULL case any non-NULL handle passed in is freed
/// eagerly so the caller never double-frees.
///
/// # Safety
///
/// `first` and `second` must be valid handles previously created by an
/// `opk_*` constructor. After this call they must NOT be used again.
#[no_mangle]
pub unsafe extern "C" fn opk_alternative(
    first: *mut OpkSchedule,
    second: *mut OpkSchedule,
) -> *mut OpkSchedule {
    if first.is_null() || second.is_null() {
        set_last_error("opk_alternative: null component");
        // Free whichever is non-null so the caller does not leak or
        // double-free.
        if !first.is_null() {
            // SAFETY: non-null pointer previously produced by
            // `Box::into_raw` via a constructor.
            unsafe { drop(Box::from_raw(first)) };
        }
        if !second.is_null() {
            // SAFETY: non-null pointer previously produced by
            // `Box::into_raw` via a constructor.
            unsafe { drop(Box::from_raw(second)) };
        }
        return std::ptr::null_mut();
    }
    // SAFETY: caller's contract asserts both handles are live and
    // previously unused since construction.
    let f = unsafe { Box::from_raw(first) }.inner;
    // SAFETY: same as above for `second`.
    let s = unsafe { Box::from_raw(second) }.inner;
    box_schedule_dyn(Box::new(schedules::Alternative::new(f, s)))
}

// -----------------------------------------------------------------------
// Compound: Multiple / Chained / Tandem
// -----------------------------------------------------------------------

/// Shared helper: drain `len` component handles into owned boxes.
///
/// On any NULL-pointer failure every handle provided is freed to avoid
/// leaks.
///
/// # Safety
///
/// `components` must point to `len` valid handles, each previously
/// returned by an `opk_*` constructor and not yet consumed.
unsafe fn take_components(
    components: *const *mut OpkSchedule,
    len: usize,
) -> Option<Vec<Box<dyn Schedule>>> {
    if len > 0 && components.is_null() {
        set_last_error("null components pointer");
        return None;
    }
    let slice: &[*mut OpkSchedule] = if len == 0 {
        &[]
    } else {
        // SAFETY: caller asserts `components` is a valid array of
        // length `len`.
        unsafe { std::slice::from_raw_parts(components, len) }
    };
    // First pass: check for NULL entries without consuming anything.
    for &p in slice {
        if p.is_null() {
            set_last_error("null component handle");
            // Free all non-null entries eagerly so the caller doesn't
            // double-free or leak.
            for &q in slice {
                if !q.is_null() {
                    // SAFETY: `q` was produced by `Box::into_raw` via
                    // a constructor.
                    unsafe { drop(Box::from_raw(q)) };
                }
            }
            return None;
        }
    }
    let mut out: Vec<Box<dyn Schedule>> = Vec::with_capacity(len);
    for &p in slice {
        // SAFETY: verified non-null above; caller guarantees not yet
        // freed.
        let wrapper = unsafe { Box::from_raw(p) };
        out.push(wrapper.inner);
    }
    Some(out)
}

/// SAFETY: caller asserts `stimuli` is NULL or an array of `len` valid
/// NUL-terminated UTF-8 strings.
unsafe fn maybe_stimuli(
    stimuli: *const *const c_char,
    len: usize,
) -> std::result::Result<Option<Vec<String>>, &'static str> {
    if stimuli.is_null() {
        return Ok(None);
    }
    let slice: &[*const c_char] = if len == 0 {
        &[]
    } else {
        // SAFETY: caller asserts a valid array of length `len`.
        unsafe { std::slice::from_raw_parts(stimuli, len) }
    };
    let mut out = Vec::with_capacity(len);
    for &p in slice {
        // SAFETY: caller asserts each pointer is a valid NUL-terminated
        // UTF-8 string.
        out.push(unsafe { cstr_to_string(p)? });
    }
    Ok(Some(out))
}

/// Multiple compound schedule. Takes ownership of every component.
///
/// * `components` — array of `len` schedule handles.
/// * `stimuli` — optional array of `len` NUL-terminated UTF-8 stimulus
///   labels. Pass NULL to skip.
///
/// On failure, all component handles are freed; the returned pointer
/// is NULL.
///
/// # Safety
///
/// * `components` must point to `len` valid schedule handles.
/// * When `stimuli` is non-NULL, it must point to `len` valid
///   NUL-terminated UTF-8 strings.
#[no_mangle]
pub unsafe extern "C" fn opk_multiple(
    components: *const *mut OpkSchedule,
    len: usize,
    stimuli: *const *const c_char,
) -> *mut OpkSchedule {
    // SAFETY: caller contract propagated to `take_components`.
    let boxes = match unsafe { take_components(components, len) } {
        Some(v) => v,
        None => return std::ptr::null_mut(),
    };
    // SAFETY: caller contract propagated to `maybe_stimuli`.
    let stims = match unsafe { maybe_stimuli(stimuli, len) } {
        Ok(v) => v,
        Err(msg) => {
            set_last_error(format!("opk_multiple: stimuli: {msg}"));
            return std::ptr::null_mut();
        }
    };
    match handle_error(schedules::Multiple::new(boxes, stims)) {
        Some(s) => box_schedule(s),
        None => std::ptr::null_mut(),
    }
}

/// Chained compound schedule. Takes ownership of every component.
///
/// # Safety
///
/// Same contract as [`opk_multiple`].
#[no_mangle]
pub unsafe extern "C" fn opk_chained(
    components: *const *mut OpkSchedule,
    len: usize,
    stimuli: *const *const c_char,
) -> *mut OpkSchedule {
    // SAFETY: caller contract propagated.
    let boxes = match unsafe { take_components(components, len) } {
        Some(v) => v,
        None => return std::ptr::null_mut(),
    };
    // SAFETY: caller contract propagated.
    let stims = match unsafe { maybe_stimuli(stimuli, len) } {
        Ok(v) => v,
        Err(msg) => {
            set_last_error(format!("opk_chained: stimuli: {msg}"));
            return std::ptr::null_mut();
        }
    };
    match handle_error(schedules::Chained::new(boxes, stims)) {
        Some(s) => box_schedule(s),
        None => std::ptr::null_mut(),
    }
}

/// Tandem compound schedule. Takes ownership of every component.
///
/// # Safety
///
/// `components` must point to `len` valid schedule handles.
#[no_mangle]
pub unsafe extern "C" fn opk_tandem(
    components: *const *mut OpkSchedule,
    len: usize,
) -> *mut OpkSchedule {
    // SAFETY: caller contract propagated.
    let boxes = match unsafe { take_components(components, len) } {
        Some(v) => v,
        None => return std::ptr::null_mut(),
    };
    match handle_error(schedules::Tandem::new(boxes)) {
        Some(s) => box_schedule(s),
        None => std::ptr::null_mut(),
    }
}

// -----------------------------------------------------------------------
// Compound: Concurrent
// -----------------------------------------------------------------------

/// Concurrent compound schedule keyed by operandum name. Takes ownership
/// of every component.
///
/// * `operanda` — array of `len` NUL-terminated UTF-8 operandum keys.
///   Duplicates are rejected.
/// * `components` — array of `len` schedule handles, paired 1:1 with
///   `operanda`.
/// * `cod` — changeover delay.
/// * `cor` — changeover ratio.
///
/// On failure, every component handle is freed.
///
/// # Safety
///
/// * `operanda` must point to `len` valid NUL-terminated UTF-8 strings.
/// * `components` must point to `len` valid schedule handles.
#[no_mangle]
pub unsafe extern "C" fn opk_concurrent(
    operanda: *const *const c_char,
    components: *const *mut OpkSchedule,
    len: usize,
    cod: f64,
    cor: u32,
) -> *mut OpkSchedule {
    if len == 0 {
        set_last_error("opk_concurrent: requires at least 2 components, got 0");
        return std::ptr::null_mut();
    }
    if operanda.is_null() {
        set_last_error("opk_concurrent: null operanda pointer");
        // Free components we are about to refuse to own.
        if !components.is_null() {
            // SAFETY: caller asserts components points to `len` entries.
            let slice = unsafe { std::slice::from_raw_parts(components, len) };
            for &p in slice {
                if !p.is_null() {
                    // SAFETY: `p` produced by `Box::into_raw`.
                    unsafe { drop(Box::from_raw(p)) };
                }
            }
        }
        return std::ptr::null_mut();
    }
    // Take components (eagerly frees on error).
    // SAFETY: caller contract propagated.
    let boxes = match unsafe { take_components(components, len) } {
        Some(v) => v,
        None => return std::ptr::null_mut(),
    };
    // Parse operandum keys; on error, boxes drop normally and the
    // components' memory is released via their destructors.
    // SAFETY: caller asserts `operanda` is a valid `len`-length array.
    let op_slice = unsafe { std::slice::from_raw_parts(operanda, len) };
    let mut keys: Vec<String> = Vec::with_capacity(len);
    for &p in op_slice {
        // SAFETY: caller asserts each entry is a valid NUL-terminated
        // UTF-8 string.
        match unsafe { cstr_to_string(p) } {
            Ok(s) => keys.push(s),
            Err(e) => {
                set_last_error(format!("opk_concurrent: operandum: {e}"));
                drop(boxes);
                return std::ptr::null_mut();
            }
        }
    }

    let mut map: IndexMap<String, Box<dyn Schedule>> = IndexMap::with_capacity(len);
    for (k, b) in keys.into_iter().zip(boxes.into_iter()) {
        if map.insert(k.clone(), b).is_some() {
            set_last_error(format!(
                "opk_concurrent: duplicate operandum key {k:?}"
            ));
            return std::ptr::null_mut();
        }
    }
    match handle_error(schedules::Concurrent::new(map, cod, cor)) {
        Some(s) => box_schedule(s),
        None => std::ptr::null_mut(),
    }
}

// -----------------------------------------------------------------------
// Unit tests
// -----------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    unsafe fn new_outcome() -> OpkOutcome {
        OpkOutcome::empty()
    }

    #[test]
    fn null_free_is_noop() {
        // SAFETY: explicit NULL; API contracts permit this.
        unsafe { opk_schedule_free(std::ptr::null_mut()) };
    }

    #[test]
    fn last_error_roundtrip() {
        set_last_error("hello world");
        let ptr = opk_last_error_message();
        assert!(!ptr.is_null());
        // SAFETY: pointer comes from our own thread-local CString.
        let s = unsafe { CStr::from_ptr(ptr) };
        assert_eq!(s.to_str().unwrap(), "hello world");

        opk_clear_last_error();
        assert!(opk_last_error_message().is_null());
    }

    #[test]
    fn fr3_reinforces_on_third_response() {
        let h = opk_fr(3);
        assert!(!h.is_null());
        // SAFETY: `h` is a live handle; `out` is a local.
        let mut out = unsafe { new_outcome() };
        let main_cstr = CString::new("main").unwrap();

        for i in 1..=3_u32 {
            // SAFETY: valid handle, valid out, valid NUL-terminated operandum.
            let rc = unsafe {
                opk_schedule_step(
                    h,
                    f64::from(i),
                    true,
                    f64::from(i),
                    main_cstr.as_ptr(),
                    &mut out,
                )
            };
            assert_eq!(rc, 0);
            if i < 3 {
                assert!(!out.reinforced);
            } else {
                assert!(out.reinforced);
                assert_eq!(out.reinforcer_time, 3.0);
            }
        }
        // SAFETY: valid handle.
        unsafe { opk_schedule_free(h) };
    }
}

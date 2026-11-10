//! Smoke test for the C FFI surface.
//!
//! Calls the `extern "C"` functions directly to confirm the ABI is
//! sound and that ownership/free semantics behave as documented. The
//! test runs as a pure-Rust integration test — it does not require a
//! C compiler — but every call path mirrors what a C caller would do.

use std::ffi::{CStr, CString};

use contingency::ffi::*;

fn empty_outcome() -> OpkOutcome {
    OpkOutcome {
        reinforced: false,
        reinforcer_time: 0.0,
        reinforcer_magnitude: 0.0,
        reinforcer_label: std::ptr::null(),
    }
}

#[test]
fn fr3_reinforces_on_third_response_via_ffi() {
    let handle = opk_fr(3);
    assert!(!handle.is_null(), "opk_fr(3) should return a handle");

    let mut out = empty_outcome();
    let operandum = CString::new("main").unwrap();

    for i in 1..=3_u32 {
        // SAFETY: `handle` is a live handle from `opk_fr`; `out` is a
        // valid stack slot; `operandum` is a valid NUL-terminated
        // string owned by this frame.
        let rc = unsafe {
            opk_schedule_step(
                handle,
                f64::from(i),
                true,
                f64::from(i),
                operandum.as_ptr(),
                &mut out,
            )
        };
        assert_eq!(rc, 0, "step {i} should succeed");
        if i < 3 {
            assert!(!out.reinforced, "step {i} should not reinforce");
        } else {
            assert!(out.reinforced, "third response should reinforce");
            assert_eq!(out.reinforcer_time, 3.0);
            assert_eq!(out.reinforcer_magnitude, 1.0);
            assert!(!out.reinforcer_label.is_null());
            // SAFETY: label is owned by the schedule handle and valid
            // until the next step.
            let label = unsafe { CStr::from_ptr(out.reinforcer_label) };
            assert_eq!(label.to_str().unwrap(), "SR+");
        }
    }

    // Reset round-trips cleanly.
    // SAFETY: live handle.
    assert_eq!(unsafe { opk_schedule_reset(handle) }, 0);

    // SAFETY: live handle.
    unsafe { opk_schedule_free(handle) };
}

#[test]
fn error_channel_populates_on_invalid_fr() {
    opk_clear_last_error();
    let handle = opk_fr(0);
    assert!(handle.is_null(), "FR(0) must fail");
    let msg_ptr = opk_last_error_message();
    assert!(!msg_ptr.is_null(), "error message must be populated");
    // SAFETY: msg_ptr is the thread-local CString we just wrote.
    let msg = unsafe { CStr::from_ptr(msg_ptr) }.to_string_lossy();
    assert!(
        msg.contains("FR requires n >= 1"),
        "unexpected error message: {msg}"
    );
}

#[test]
fn ext_never_reinforces() {
    let handle = opk_ext();
    assert!(!handle.is_null());
    let mut out = empty_outcome();
    let operandum = CString::new("main").unwrap();
    for i in 1..=5_u32 {
        // SAFETY: live handle, valid slot and string.
        let rc = unsafe {
            opk_schedule_step(
                handle,
                f64::from(i),
                true,
                f64::from(i),
                operandum.as_ptr(),
                &mut out,
            )
        };
        assert_eq!(rc, 0);
        assert!(!out.reinforced);
        assert!(out.reinforcer_label.is_null());
    }
    // SAFETY: live handle.
    unsafe { opk_schedule_free(handle) };
}

#[test]
fn alternative_takes_ownership_of_components() {
    let first = opk_fr(1);
    let second = opk_fr(2);
    assert!(!first.is_null() && !second.is_null());
    // SAFETY: both handles valid and unused since construction.
    let compound = unsafe { opk_alternative(first, second) };
    assert!(!compound.is_null());
    // Now `first` and `second` are consumed; freeing only the compound
    // releases the whole tree.
    // SAFETY: live compound handle.
    unsafe { opk_schedule_free(compound) };
}

#[test]
fn limited_hold_fi_wraps_and_runs() {
    let inner = opk_armable_fi(10.0);
    assert!(!inner.is_null());
    // SAFETY: inner is valid and unused.
    let lh = unsafe { opk_limited_hold_fi(inner, 2.0) };
    assert!(!lh.is_null());
    let mut out = empty_outcome();
    // Advance past the FI interval but within the hold window and
    // emit a response: should reinforce.
    let op = CString::new("main").unwrap();
    // SAFETY: valid handle, slot, string.
    let rc = unsafe { opk_schedule_step(lh, 11.0, true, 11.0, op.as_ptr(), &mut out) };
    assert_eq!(rc, 0);
    assert!(out.reinforced);
    // SAFETY: live handle.
    unsafe { opk_schedule_free(lh) };
}

#[test]
fn progressive_ratio_arithmetic() {
    let pr = opk_pr_arithmetic(1, 1);
    assert!(!pr.is_null());
    // SAFETY: live handle.
    unsafe { opk_schedule_free(pr) };
}

#[test]
fn concurrent_round_trips() {
    let a = opk_fr(1);
    let b = opk_fr(1);
    assert!(!a.is_null() && !b.is_null());
    let left = CString::new("left").unwrap();
    let right = CString::new("right").unwrap();
    let ops: [*const std::os::raw::c_char; 2] = [left.as_ptr(), right.as_ptr()];
    let comps: [*mut OpkSchedule; 2] = [a, b];
    // SAFETY: parallel arrays of length 2; components and strings are
    // valid for the duration of the call.
    let handle = unsafe { opk_concurrent(ops.as_ptr(), comps.as_ptr(), 2, 0.0, 0) };
    assert!(!handle.is_null());
    // SAFETY: live handle.
    unsafe { opk_schedule_free(handle) };
}

// -----------------------------------------------------------------------
// opk_from_spec
// -----------------------------------------------------------------------

/// Step `handle` once with a response on `operandum` at `t`.
fn respond(handle: *mut OpkSchedule, t: f64, operandum: &str) -> (CIntRc, OpkOutcome) {
    let op = CString::new(operandum).unwrap();
    let mut out = empty_outcome();
    // SAFETY: `handle` is live; `op` and `out` outlive the call.
    let rc = unsafe { opk_schedule_step(handle, t, true, t, op.as_ptr(), &mut out) };
    (rc, out)
}

type CIntRc = std::os::raw::c_int;

#[test]
fn from_spec_builds_fr() {
    let spec = CString::new(r#"{"type": "FR", "params": {"n": 3}}"#).unwrap();
    // SAFETY: `spec` is a valid NUL-terminated string for the call.
    let handle = unsafe { opk_from_spec(spec.as_ptr()) };
    assert!(
        !handle.is_null(),
        "opk_from_spec(FR 3) should return a handle"
    );
    let fired: Vec<bool> = (1..=3)
        .map(|i| {
            let (rc, out) = respond(handle, f64::from(i), "main");
            assert_eq!(rc, 0);
            out.reinforced
        })
        .collect();
    assert_eq!(fired, vec![false, false, true]);
    // SAFETY: live handle.
    unsafe { opk_schedule_free(handle) };
}

#[test]
fn from_spec_builds_concurrent_vi_vi() {
    let spec = CString::new(
        r#"{"type": "Concurrent", "params": {
            "components": {
                "left": {"type": "VI", "params": {"mean_interval": 1.0, "seed": 1}},
                "right": {"type": "VI", "params": {"mean_interval": 1.0, "seed": 2}}
            },
            "cod": 0.0}}"#,
    )
    .unwrap();
    // SAFETY: `spec` is a valid NUL-terminated string for the call.
    let handle = unsafe { opk_from_spec(spec.as_ptr()) };
    assert!(
        !handle.is_null(),
        "opk_from_spec(Conc VI VI) should return a handle"
    );
    // Both intervals have long elapsed at t=100: the first response on
    // each operandum is reinforced; an immediate repeat on `left` is not.
    let (rc, a) = respond(handle, 100.0, "left");
    assert_eq!(rc, 0);
    assert!(a.reinforced, "left VI should be armed at t=100");
    let (rc, b) = respond(handle, 100.001, "left");
    assert_eq!(rc, 0);
    assert!(!b.reinforced, "left VI restarts after reinforcement");
    let (rc, c) = respond(handle, 100.002, "right");
    assert_eq!(rc, 0);
    assert!(c.reinforced, "right VI runs independently of left");
    // An operandum the spec did not name is a step error.
    let (rc, _) = respond(handle, 100.003, "middle");
    assert_ne!(rc, 0);
    // SAFETY: live handle.
    unsafe { opk_schedule_free(handle) };
}

#[test]
fn from_spec_reports_bad_json_and_bad_spec() {
    for bad in [
        "not json",
        r#"{"type": "NoSuchSchedule"}"#,
        r#"{"type": "FR", "params": {"n": 0}}"#,
    ] {
        opk_clear_last_error();
        let spec = CString::new(bad).unwrap();
        // SAFETY: valid NUL-terminated string.
        let handle = unsafe { opk_from_spec(spec.as_ptr()) };
        assert!(handle.is_null(), "{bad:?} should be rejected");
        let msg = opk_last_error_message();
        assert!(!msg.is_null(), "{bad:?} should set an error message");
    }
    // SAFETY: NULL is the documented error case.
    assert!(unsafe { opk_from_spec(std::ptr::null()) }.is_null());
}

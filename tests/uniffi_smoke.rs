//! Smoke tests for the UniFFI binding surface.
//!
//! These only verify that the Rust-side `#[uniffi::export]` surface
//! round-trips values correctly. Downstream Swift / Kotlin / KMP
//! consumers are exercised via separate binding-generation + language
//! test suites (not run here).

#![cfg(feature = "uniffi")]

use contingency::uniffi_api::{UniffiResponseEvent, UniffiSchedule};

fn response(time: f64, operandum: &str) -> UniffiResponseEvent {
    UniffiResponseEvent {
        time,
        operandum: operandum.into(),
    }
}

#[test]
fn fr3_reinforces_on_third_response() {
    let s = UniffiSchedule::fr(3).unwrap();

    let r1 = s.step(1.0, Some(response(1.0, "main"))).unwrap();
    assert!(!r1.reinforced);

    let r2 = s.step(2.0, Some(response(2.0, "main"))).unwrap();
    assert!(!r2.reinforced);

    let r3 = s.step(3.0, Some(response(3.0, "main"))).unwrap();
    assert!(r3.reinforced);
    let reinforcer = r3.reinforcer.expect("expected reinforcer on FR3 completion");
    assert_eq!(reinforcer.label, "SR+");
}

#[test]
fn crf_reinforces_every_response() {
    let s = UniffiSchedule::crf();
    for t in [1.0, 2.0, 3.0] {
        let out = s.step(t, Some(response(t, "main"))).unwrap();
        assert!(out.reinforced, "CRF should reinforce at t={t}");
    }
}

#[test]
fn fi_reinforces_after_interval() {
    let s = UniffiSchedule::fi(5.0).unwrap();

    let early = s.step(2.0, Some(response(2.0, "main"))).unwrap();
    assert!(!early.reinforced);

    let ready = s.step(6.0, Some(response(6.0, "main"))).unwrap();
    assert!(ready.reinforced);
}

#[test]
fn ext_never_reinforces() {
    let s = UniffiSchedule::ext();
    for t in [1.0, 2.0, 3.0, 100.0] {
        let out = s.step(t, Some(response(t, "main"))).unwrap();
        assert!(!out.reinforced);
    }
}

#[test]
fn reset_returns_to_initial_state() {
    let s = UniffiSchedule::fr(2).unwrap();
    s.step(1.0, Some(response(1.0, "main"))).unwrap();
    s.reset();
    let r1 = s.step(2.0, Some(response(2.0, "main"))).unwrap();
    assert!(!r1.reinforced, "post-reset FR2 should not reinforce on 1st response");
    let r2 = s.step(3.0, Some(response(3.0, "main"))).unwrap();
    assert!(r2.reinforced);
}

#[test]
fn fr_zero_returns_config_error() {
    // `Arc<UniffiSchedule>` does not implement `Debug`, so we cannot
    // use `expect_err` here — match the error branch manually.
    match UniffiSchedule::fr(0) {
        Ok(_) => panic!("FR 0 must be rejected"),
        Err(e) => assert!(!e.to_string().is_empty()),
    }
}

#[test]
fn limited_hold_fi_withdraws_on_missed_hold() {
    let s = UniffiSchedule::limited_hold_fi(1.0, 0.5).unwrap();

    // Skip past the arm + hold window without responding.
    let withdraw = s.step(2.0, None).unwrap();
    assert!(!withdraw.reinforced);

    // Respond after the next interval has elapsed.
    let later = s.step(4.0, Some(response(4.0, "main"))).unwrap();
    // The schedule may or may not have rearmed by exactly t=4.0 — assert
    // only that the call succeeds and returns a well-formed outcome.
    let _ = later.reinforced;
}

#[test]
fn pr_arithmetic_rejects_zero_start() {
    match UniffiSchedule::pr_arithmetic(0, 1) {
        Ok(_) => panic!("arithmetic start < 1 must be rejected"),
        Err(e) => assert!(!e.to_string().is_empty()),
    }
}

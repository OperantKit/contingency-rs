//! Integration tests for `contingency::hw::VirtualApparatus`.
//!
//! Ported from Python `tests/test_hw_virtual.py`.

use contingency::hw::{Apparatus, LogKind, LogPayload, VirtualApparatus};
use contingency::{ContingencyError, Reinforcer};

fn make() -> VirtualApparatus {
    VirtualApparatus::new(
        vec!["left".into(), "right".into()],
        vec!["food".into()],
        vec!["houselight".into()],
        "test".into(),
    )
    .expect("ok")
}

#[test]
fn info_reflects_construction() {
    let a = make();
    let info = a.info();
    assert_eq!(info.name, "test");
    assert_eq!(info.backend, "virtual");
    assert_eq!(info.operanda, vec!["left", "right"]);
    assert_eq!(info.reinforcers, vec!["food"]);
    assert_eq!(info.stimuli, vec!["houselight"]);
}

#[test]
fn initial_status_is_disconnected() {
    let a = make();
    assert!(!a.status().connected);
    assert!(a.status().last_error.is_none());
}

#[test]
fn connect_sets_connected_true_idempotent() {
    let mut a = make();
    a.connect().unwrap();
    assert!(a.status().connected);
    a.connect().unwrap();
    assert!(a.status().connected);
}

#[test]
fn poll_before_connect_is_hardware_error() {
    let mut a = make();
    let err = a.poll_responses(0.0).unwrap_err();
    assert!(matches!(err, ContingencyError::Hardware(_)));
}

#[test]
fn deliver_before_connect_is_hardware_error() {
    let mut a = make();
    let err = a
        .deliver_reinforcer(0.0, &Reinforcer::at(0.0), "food")
        .unwrap_err();
    assert!(matches!(err, ContingencyError::Hardware(_)));
}

#[test]
fn set_stimulus_before_connect_is_hardware_error() {
    let mut a = make();
    let err = a.set_stimulus(0.0, "houselight", true).unwrap_err();
    assert!(matches!(err, ContingencyError::Hardware(_)));
}

#[test]
fn press_enqueues_and_poll_drains() {
    let mut a = make();
    a.connect().unwrap();
    a.press("left", 1.0).unwrap();
    a.press("right", 2.0).unwrap();
    let drained = a.poll_responses(3.0).unwrap();
    assert_eq!(drained.len(), 2);
    assert_eq!(drained[0].operandum, "left");
    assert_eq!(drained[0].time, 1.0);
    assert_eq!(drained[1].operandum, "right");
    // Second poll returns empty.
    assert!(a.poll_responses(4.0).unwrap().is_empty());
}

#[test]
fn press_unknown_operandum_is_hardware_error() {
    let mut a = make();
    let err = a.press("up", 1.0).unwrap_err();
    assert!(matches!(err, ContingencyError::Hardware(_)));
}

#[test]
fn deliver_reinforcer_logs_event() {
    let mut a = make();
    a.connect().unwrap();
    let r = Reinforcer {
        time: 5.0,
        magnitude: 2.5,
        label: "SR+".into(),
    };
    a.deliver_reinforcer(5.0, &r, "food").unwrap();
    let log = a.event_log();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].kind, LogKind::Reinforcer);
    assert_eq!(log[0].time, 5.0);
    match &log[0].payload {
        LogPayload::Reinforcer {
            channel,
            magnitude,
            label,
        } => {
            assert_eq!(channel, "food");
            assert_eq!(*magnitude, 2.5);
            assert_eq!(label, "SR+");
        }
        _ => panic!("expected reinforcer payload"),
    }
}

#[test]
fn deliver_reinforcer_unknown_channel_errors() {
    let mut a = make();
    a.connect().unwrap();
    let err = a
        .deliver_reinforcer(0.0, &Reinforcer::at(0.0), "water")
        .unwrap_err();
    assert!(matches!(err, ContingencyError::Hardware(_)));
}

#[test]
fn set_stimulus_on_off_logs_event() {
    let mut a = make();
    a.connect().unwrap();
    a.set_stimulus(1.0, "houselight", true).unwrap();
    a.set_stimulus(2.0, "houselight", false).unwrap();
    let log = a.event_log();
    assert_eq!(log.len(), 2);
    assert_eq!(log[0].kind, LogKind::StimulusOn);
    assert_eq!(log[1].kind, LogKind::StimulusOff);
}

#[test]
fn set_stimulus_unknown_errors() {
    let mut a = make();
    a.connect().unwrap();
    let err = a.set_stimulus(0.0, "tone", true).unwrap_err();
    assert!(matches!(err, ContingencyError::Hardware(_)));
}

#[test]
fn disconnect_clears_queue() {
    let mut a = make();
    a.connect().unwrap();
    a.press("left", 1.0).unwrap();
    a.disconnect().unwrap();
    assert!(!a.status().connected);
    a.connect().unwrap();
    // Reconnect after disconnect: queue should be empty.
    assert!(a.poll_responses(2.0).unwrap().is_empty());
}

#[test]
fn event_log_is_ordered() {
    let mut a = make();
    a.connect().unwrap();
    a.press("left", 1.0).unwrap();
    a.deliver_reinforcer(2.0, &Reinforcer::at(2.0), "food").unwrap();
    a.set_stimulus(3.0, "houselight", true).unwrap();
    let log = a.event_log();
    assert_eq!(log.len(), 3);
    assert_eq!(log[0].kind, LogKind::Response);
    assert_eq!(log[1].kind, LogKind::Reinforcer);
    assert_eq!(log[2].kind, LogKind::StimulusOn);
}

#[test]
fn empty_operanda_errors() {
    let err = VirtualApparatus::new(
        Vec::new(),
        vec!["food".into()],
        Vec::new(),
        "x".into(),
    )
    .unwrap_err();
    assert!(matches!(err, ContingencyError::Hardware(_)));
}

#[test]
fn empty_reinforcers_errors() {
    let err = VirtualApparatus::new(
        vec!["main".into()],
        Vec::new(),
        Vec::new(),
        "x".into(),
    )
    .unwrap_err();
    assert!(matches!(err, ContingencyError::Hardware(_)));
}

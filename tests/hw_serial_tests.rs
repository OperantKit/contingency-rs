//! Integration tests for the serial-backend apparatus (feature `serial`).
//!
//! Uses a mock in-memory link so no real hardware is required.

#![cfg(feature = "serial")]

use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

use contingency::hw::serial_backend::SerialApparatus;
use contingency::hw::Apparatus;
use contingency::{ContingencyError, Reinforcer};

/// Mock full-duplex link: caller pushes inbound bytes via `push_inbound`
/// and inspects outbound bytes via `outbound`.
#[derive(Clone, Default)]
struct MockLink {
    inbound: Arc<Mutex<Vec<u8>>>,
    outbound: Arc<Mutex<Vec<u8>>>,
}

impl MockLink {
    fn new() -> Self {
        Self::default()
    }
    fn push_inbound(&self, bytes: &[u8]) {
        self.inbound.lock().unwrap().extend_from_slice(bytes);
    }
    fn outbound_drain(&self) -> Vec<u8> {
        std::mem::take(&mut *self.outbound.lock().unwrap())
    }
}

impl Read for MockLink {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut inbound = self.inbound.lock().unwrap();
        if inbound.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                "no data",
            ));
        }
        let n = buf.len().min(inbound.len());
        buf[..n].copy_from_slice(&inbound[..n]);
        inbound.drain(..n);
        Ok(n)
    }
}

impl Write for MockLink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.outbound.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn build(link: MockLink) -> SerialApparatus<MockLink> {
    SerialApparatus::with_link(
        link,
        vec!["left".into(), "right".into()],
        vec!["food".into()],
        vec!["cue".into()],
        "serial-test".into(),
    )
    .expect("construction ok")
}

#[test]
fn poll_before_connect_is_hardware_error() {
    let link = MockLink::new();
    let mut a = build(link);
    let err = a.poll_responses(0.0).unwrap_err();
    assert!(matches!(err, ContingencyError::Hardware(_)));
}

#[test]
fn deliver_before_connect_is_hardware_error() {
    let link = MockLink::new();
    let mut a = build(link);
    let err = a
        .deliver_reinforcer(0.0, &Reinforcer::at(0.0), "food")
        .unwrap_err();
    assert!(matches!(err, ContingencyError::Hardware(_)));
}

#[test]
fn set_stimulus_before_connect_is_hardware_error() {
    let link = MockLink::new();
    let mut a = build(link);
    let err = a.set_stimulus(0.0, "cue", true).unwrap_err();
    assert!(matches!(err, ContingencyError::Hardware(_)));
}

#[test]
fn inbound_response_line_parsed() {
    let link = MockLink::new();
    link.push_inbound(b"R left\n");
    let mut a = build(link.clone());
    a.connect().unwrap();
    let events = a.poll_responses(1.0).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].operandum, "left");
    assert_eq!(events[0].time, 1.0);
}

#[test]
fn multiple_lines_in_single_read() {
    let link = MockLink::new();
    link.push_inbound(b"R left\nR right\nR left\n");
    let mut a = build(link.clone());
    a.connect().unwrap();
    let events = a.poll_responses(5.0).unwrap();
    assert_eq!(events.len(), 3);
    assert_eq!(events[0].operandum, "left");
    assert_eq!(events[1].operandum, "right");
    assert_eq!(events[2].operandum, "left");
}

#[test]
fn partial_line_buffered_across_polls() {
    let link = MockLink::new();
    let mut a = build(link.clone());
    a.connect().unwrap();
    link.push_inbound(b"R le");
    let events = a.poll_responses(1.0).unwrap();
    assert!(events.is_empty());
    link.push_inbound(b"ft\n");
    let events = a.poll_responses(2.0).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].operandum, "left");
    assert_eq!(events[0].time, 2.0);
}

#[test]
fn malformed_lines_dropped() {
    let link = MockLink::new();
    link.push_inbound(b"garbage\nR\nR left extra\nX left\nR unknown\nR right\n");
    let mut a = build(link.clone());
    a.connect().unwrap();
    let events = a.poll_responses(0.0).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].operandum, "right");
}

#[test]
fn deliver_writes_wire_frame() {
    let link = MockLink::new();
    let mut a = build(link.clone());
    a.connect().unwrap();
    let r = Reinforcer {
        time: 0.0,
        magnitude: 1.5,
        label: "SR+".into(),
    };
    a.deliver_reinforcer(0.0, &r, "food").unwrap();
    let out = link.outbound_drain();
    assert_eq!(out, b"D food 1.5\n");
}

#[test]
fn set_stimulus_writes_wire_frame() {
    let link = MockLink::new();
    let mut a = build(link.clone());
    a.connect().unwrap();
    a.set_stimulus(0.0, "cue", true).unwrap();
    a.set_stimulus(0.0, "cue", false).unwrap();
    let out = link.outbound_drain();
    assert_eq!(out, b"S cue 1\nS cue 0\n");
}

#[test]
fn deliver_unknown_channel_errors() {
    let link = MockLink::new();
    let mut a = build(link);
    a.connect().unwrap();
    let err = a
        .deliver_reinforcer(0.0, &Reinforcer::at(0.0), "water")
        .unwrap_err();
    assert!(matches!(err, ContingencyError::Hardware(_)));
}

#[test]
fn set_stimulus_unknown_errors() {
    let link = MockLink::new();
    let mut a = build(link);
    a.connect().unwrap();
    let err = a.set_stimulus(0.0, "tone", true).unwrap_err();
    assert!(matches!(err, ContingencyError::Hardware(_)));
}

#[test]
fn disconnect_drops_link() {
    let link = MockLink::new();
    let mut a = build(link);
    a.connect().unwrap();
    a.disconnect().unwrap();
    assert!(!a.status().connected);
    // Reconnect fails because the link was dropped.
    let err = a.connect().unwrap_err();
    assert!(matches!(err, ContingencyError::Hardware(_)));
}

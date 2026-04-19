//! Serial-backed [`Apparatus`] backend (feature `serial`).
//!
//! # Wire protocol
//!
//! ASCII, line-oriented (`\n`-terminated). One frame per line.
//!
//! **Apparatus → host (unknown lines silently dropped)**
//!
//! * `R <operandum>` — the subject produced a response on the named
//!   operandum (for example `R left`).
//!
//! **Host → apparatus**
//!
//! * `D <channel> <magnitude>` — deliver a reinforcer on `channel` with
//!   the given magnitude.
//! * `S <stimulus> <0|1>` — set the stimulus state (`1` = on, `0` = off).
//!
//! No retries, no reconnection, no handshake.
//!
//! # Transport abstraction
//!
//! [`SerialApparatus`] is generic over a [`SerialLink`] — any type
//! that implements [`std::io::Read`] + [`std::io::Write`] + `Send`.
//! The [`open`] helper opens a real hardware port via the `serialport`
//! crate (feature-gated). Tests supply a mock link (e.g. a pair of
//! `Cursor<Vec<u8>>` or a ring buffer).

use std::io::{Read, Write};

use crate::errors::{ContingencyError, Result};
use crate::hw::not_connected;
use crate::hw::protocols::{Apparatus, ApparatusInfo, ApparatusStatus};
use crate::types::{Reinforcer, ResponseEvent};

/// Marker trait: any `Read + Write + Send` can act as a serial link.
pub trait SerialLink: Read + Write + Send {}
impl<T: Read + Write + Send> SerialLink for T {}

/// Line-oriented serial transport for operant-box hardware.
///
/// Generic over the link type `L`. Use [`open`] to obtain a real
/// `serialport`-backed apparatus, or construct directly with a mock
/// link for tests.
pub struct SerialApparatus<L: SerialLink> {
    info: ApparatusInfo,
    status: ApparatusStatus,
    link: Option<L>,
    read_buffer: Vec<u8>,
}

impl<L: SerialLink> SerialApparatus<L> {
    /// Construct with an explicit link (pre-opened). `connect()` is
    /// still required before any I/O.
    ///
    /// Returns a `Hardware` error if `operanda` or `reinforcers` is
    /// empty.
    pub fn with_link(
        link: L,
        operanda: Vec<String>,
        reinforcers: Vec<String>,
        stimuli: Vec<String>,
        name: String,
    ) -> Result<Self> {
        if operanda.is_empty() {
            return Err(ContingencyError::Hardware(
                "at least one operandum must be declared".into(),
            ));
        }
        if reinforcers.is_empty() {
            return Err(ContingencyError::Hardware(
                "at least one reinforcer channel must be declared".into(),
            ));
        }
        Ok(Self {
            info: ApparatusInfo {
                name,
                backend: "serial".into(),
                operanda,
                reinforcers,
                stimuli,
            },
            status: ApparatusStatus::default(),
            link: Some(link),
            read_buffer: Vec::new(),
        })
    }

    fn require_connected(&mut self) -> Result<&mut L> {
        if !self.status.connected {
            return Err(not_connected("SerialApparatus"));
        }
        self.link
            .as_mut()
            .ok_or_else(|| not_connected("SerialApparatus"))
    }

    fn parse_line(&self, raw: &[u8], now: f64) -> Option<ResponseEvent> {
        let text = std::str::from_utf8(raw).ok()?.trim();
        if text.is_empty() {
            return None;
        }
        let mut parts = text.split_whitespace();
        let tag = parts.next()?;
        let op = parts.next()?;
        if parts.next().is_some() {
            return None;
        }
        if tag != "R" {
            return None;
        }
        if !self.info.operanda.iter().any(|o| o == op) {
            return None;
        }
        Some(ResponseEvent {
            time: now,
            operandum: op.into(),
        })
    }
}

impl<L: SerialLink> Apparatus for SerialApparatus<L> {
    fn info(&self) -> ApparatusInfo {
        self.info.clone()
    }

    fn status(&self) -> ApparatusStatus {
        self.status.clone()
    }

    fn connect(&mut self) -> Result<()> {
        if self.link.is_none() {
            return Err(ContingencyError::Hardware(
                "serial link has been dropped; reopen via SerialApparatus::with_link".into(),
            ));
        }
        self.status = ApparatusStatus {
            connected: true,
            last_error: None,
        };
        self.read_buffer.clear();
        Ok(())
    }

    fn disconnect(&mut self) -> Result<()> {
        // Drop the link so real transports (e.g. serialport) close.
        self.link = None;
        self.status = ApparatusStatus {
            connected: false,
            last_error: self.status.last_error.clone(),
        };
        self.read_buffer.clear();
        Ok(())
    }

    fn poll_responses(&mut self, now: f64) -> Result<Vec<ResponseEvent>> {
        // Establish connection precondition.
        let _ = self.require_connected()?;

        // Read available bytes from the link into a local buffer.
        let mut chunk = [0u8; 4096];
        let n = {
            let link = self.require_connected()?;
            match link.read(&mut chunk) {
                Ok(n) => n,
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut
                    || e.kind() == std::io::ErrorKind::WouldBlock =>
                {
                    0
                }
                Err(e) => {
                    let msg = e.to_string();
                    self.status = ApparatusStatus {
                        connected: true,
                        last_error: Some(msg.clone()),
                    };
                    return Err(ContingencyError::Hardware(format!(
                        "serial read failed: {msg}"
                    )));
                }
            }
        };
        if n > 0 {
            self.read_buffer.extend_from_slice(&chunk[..n]);
        }

        let mut events = Vec::new();
        loop {
            let Some(pos) = self.read_buffer.iter().position(|&b| b == b'\n') else {
                break;
            };
            let line: Vec<u8> = self.read_buffer.drain(..=pos).take(pos).collect();
            if let Some(ev) = self.parse_line(&line, now) {
                events.push(ev);
            }
        }
        Ok(events)
    }

    fn deliver_reinforcer(
        &mut self,
        _now: f64,
        reinforcer: &Reinforcer,
        channel: &str,
    ) -> Result<()> {
        let _ = self.require_connected()?;
        if !self.info.reinforcers.iter().any(|c| c == channel) {
            return Err(ContingencyError::Hardware(format!(
                "unknown reinforcer channel: {channel:?}; available: {:?}",
                self.info.reinforcers
            )));
        }
        let payload = format!("D {channel} {}\n", reinforcer.magnitude);
        let link = self.require_connected()?;
        if let Err(e) = link.write_all(payload.as_bytes()).and_then(|_| link.flush()) {
            let msg = e.to_string();
            self.status = ApparatusStatus {
                connected: true,
                last_error: Some(msg.clone()),
            };
            return Err(ContingencyError::Hardware(format!(
                "serial write failed: {msg}"
            )));
        }
        Ok(())
    }

    fn set_stimulus(&mut self, _now: f64, stimulus: &str, on: bool) -> Result<()> {
        let _ = self.require_connected()?;
        if !self.info.stimuli.iter().any(|s| s == stimulus) {
            return Err(ContingencyError::Hardware(format!(
                "unknown stimulus: {stimulus:?}; available: {:?}",
                self.info.stimuli
            )));
        }
        let payload = format!("S {stimulus} {}\n", if on { 1 } else { 0 });
        let link = self.require_connected()?;
        if let Err(e) = link.write_all(payload.as_bytes()).and_then(|_| link.flush()) {
            let msg = e.to_string();
            self.status = ApparatusStatus {
                connected: true,
                last_error: Some(msg.clone()),
            };
            return Err(ContingencyError::Hardware(format!(
                "serial write failed: {msg}"
            )));
        }
        Ok(())
    }
}

/// Open a real hardware serial port via the `serialport` crate.
///
/// Only compiled when the `serial` feature is enabled.
pub fn open(
    port: &str,
    baudrate: u32,
    read_timeout: std::time::Duration,
    operanda: Vec<String>,
    reinforcers: Vec<String>,
    stimuli: Vec<String>,
    name: String,
) -> Result<SerialApparatus<Box<dyn SerialLink>>> {
    let sp = serialport::new(port, baudrate)
        .timeout(read_timeout)
        .open()
        .map_err(|e| {
            ContingencyError::Hardware(format!("failed to open serial port {port:?}: {e}"))
        })?;
    // `serialport::SerialPort` is `Read + Write + Send` via its trait
    // objects; wrap in a Box<dyn SerialLink>.
    let link: Box<dyn SerialLink> = Box::new(SerialPortAdapter(sp));
    SerialApparatus::with_link(link, operanda, reinforcers, stimuli, name)
}

/// Thin adapter so a `Box<dyn serialport::SerialPort>` satisfies
/// `Read + Write + Send`.
struct SerialPortAdapter(Box<dyn serialport::SerialPort>);

impl Read for SerialPortAdapter {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0.read(buf)
    }
}

impl Write for SerialPortAdapter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
    }
}

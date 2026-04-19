//! In-memory [`Apparatus`] backend for tests and offline simulation.
//!
//! The caller injects synthetic responses via [`VirtualApparatus::press`]
//! and drains them on the next
//! [`Apparatus::poll_responses`] call. Deliveries and stimulus toggles
//! append to the event log, which exposes a full, ordered trace of
//! every interaction for assertions.
//!
//! The backend is deliberately deterministic: no random IRT generator,
//! no clock, no latency. Those concerns belong to higher layers — e.g.
//! a subject simulator that pushes presses via
//! [`VirtualApparatus::press`].

use std::collections::VecDeque;

use crate::errors::{ContingencyError, Result};
use crate::hw::not_connected;
use crate::hw::protocols::{Apparatus, ApparatusInfo, ApparatusStatus};
use crate::types::{Reinforcer, ResponseEvent};

/// Category of a [`LoggedEvent`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogKind {
    /// A response was pressed.
    Response,
    /// A reinforcer was delivered on a channel.
    Reinforcer,
    /// A stimulus was turned on.
    StimulusOn,
    /// A stimulus was turned off.
    StimulusOff,
}

/// Payload for a [`LoggedEvent`]; tagged by [`LogKind`].
#[derive(Clone, Debug, PartialEq)]
pub enum LogPayload {
    /// Response event carrying the operandum name.
    Response {
        /// The operandum that was pressed.
        operandum: String,
    },
    /// Reinforcer delivery carrying channel, magnitude, label.
    Reinforcer {
        /// Reinforcer channel.
        channel: String,
        /// Delivery magnitude.
        magnitude: f64,
        /// Delivery label (e.g. `"SR+"`).
        label: String,
    },
    /// Stimulus on/off event carrying the stimulus name.
    Stimulus {
        /// Stimulus identifier.
        stimulus: String,
    },
}

/// A single entry in [`VirtualApparatus::event_log`].
#[derive(Clone, Debug, PartialEq)]
pub struct LoggedEvent {
    /// Monotonic timestamp of the event.
    pub time: f64,
    /// Event kind tag.
    pub kind: LogKind,
    /// Typed payload.
    pub payload: LogPayload,
}

/// In-memory operant-box simulator.
#[derive(Debug)]
pub struct VirtualApparatus {
    info: ApparatusInfo,
    status: ApparatusStatus,
    queue: VecDeque<ResponseEvent>,
    log: Vec<LoggedEvent>,
}

impl VirtualApparatus {
    /// Construct a new virtual apparatus.
    ///
    /// Returns a `Hardware` error if `operanda` or `reinforcers` is
    /// empty.
    pub fn new(
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
                backend: "virtual".into(),
                operanda,
                reinforcers,
                stimuli,
            },
            status: ApparatusStatus::default(),
            queue: VecDeque::new(),
            log: Vec::new(),
        })
    }

    /// Construct with sensible defaults (`operanda=["main"]`,
    /// `reinforcers=["food"]`, empty stimuli, `name="virtual"`).
    pub fn with_defaults() -> Self {
        Self::new(
            vec!["main".into()],
            vec!["food".into()],
            Vec::new(),
            "virtual".into(),
        )
        .expect("defaults are non-empty")
    }

    /// Enqueue a response event for the next
    /// [`Apparatus::poll_responses`]. Returns a `Hardware` error if
    /// `operandum` is unknown.
    pub fn press(&mut self, operandum: impl Into<String>, at: f64) -> Result<()> {
        let operandum = operandum.into();
        if !self.info.operanda.iter().any(|o| o == &operandum) {
            return Err(ContingencyError::Hardware(format!(
                "unknown operandum: {operandum:?}; available: {:?}",
                self.info.operanda
            )));
        }
        let event = ResponseEvent {
            time: at,
            operandum: operandum.clone(),
        };
        self.queue.push_back(event);
        self.log.push(LoggedEvent {
            time: at,
            kind: LogKind::Response,
            payload: LogPayload::Response { operandum },
        });
        Ok(())
    }

    /// Immutable snapshot of every delivery, stimulus, and response
    /// seen by this apparatus, in insertion order.
    pub fn event_log(&self) -> &[LoggedEvent] {
        &self.log
    }

    fn require_connected(&self) -> Result<()> {
        if !self.status.connected {
            return Err(not_connected("VirtualApparatus"));
        }
        Ok(())
    }
}

impl Apparatus for VirtualApparatus {
    fn info(&self) -> ApparatusInfo {
        self.info.clone()
    }

    fn status(&self) -> ApparatusStatus {
        self.status.clone()
    }

    fn connect(&mut self) -> Result<()> {
        self.status = ApparatusStatus {
            connected: true,
            last_error: None,
        };
        Ok(())
    }

    fn disconnect(&mut self) -> Result<()> {
        self.status = ApparatusStatus {
            connected: false,
            last_error: self.status.last_error.clone(),
        };
        self.queue.clear();
        Ok(())
    }

    fn poll_responses(&mut self, _now: f64) -> Result<Vec<ResponseEvent>> {
        self.require_connected()?;
        if self.queue.is_empty() {
            return Ok(Vec::new());
        }
        Ok(self.queue.drain(..).collect())
    }

    fn deliver_reinforcer(
        &mut self,
        now: f64,
        reinforcer: &Reinforcer,
        channel: &str,
    ) -> Result<()> {
        self.require_connected()?;
        if !self.info.reinforcers.iter().any(|c| c == channel) {
            return Err(ContingencyError::Hardware(format!(
                "unknown reinforcer channel: {channel:?}; available: {:?}",
                self.info.reinforcers
            )));
        }
        self.log.push(LoggedEvent {
            time: now,
            kind: LogKind::Reinforcer,
            payload: LogPayload::Reinforcer {
                channel: channel.into(),
                magnitude: reinforcer.magnitude,
                label: reinforcer.label.clone(),
            },
        });
        Ok(())
    }

    fn set_stimulus(&mut self, now: f64, stimulus: &str, on: bool) -> Result<()> {
        self.require_connected()?;
        if !self.info.stimuli.iter().any(|s| s == stimulus) {
            return Err(ContingencyError::Hardware(format!(
                "unknown stimulus: {stimulus:?}; available: {:?}",
                self.info.stimuli
            )));
        }
        self.log.push(LoggedEvent {
            time: now,
            kind: if on {
                LogKind::StimulusOn
            } else {
                LogKind::StimulusOff
            },
            payload: LogPayload::Stimulus {
                stimulus: stimulus.into(),
            },
        });
        Ok(())
    }
}

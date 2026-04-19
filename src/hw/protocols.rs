//! HAL contract: [`Apparatus`] trait plus descriptor value types.
//!
//! Mirrors `contingency.hw.protocols` on the Python side. An
//! `Apparatus` exposes three families of operations:
//!
//! * **lifecycle** — [`Apparatus::connect`] / [`Apparatus::disconnect`]
//! * **input** — [`Apparatus::poll_responses`] drains pending
//!   [`ResponseEvent`] values
//! * **output** — [`Apparatus::deliver_reinforcer`] fires a reinforcer,
//!   [`Apparatus::set_stimulus`] toggles a stimulus line
//!
//! The trait is synchronous and polling-based. Any async / event-loop
//! integration belongs in a higher-level session runner.

use serde::{Deserialize, Serialize};

use crate::errors::Result;
use crate::types::{Reinforcer, ResponseEvent};

/// Static description of an apparatus.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApparatusInfo {
    /// Human-readable identifier (appears in logs / dashboards).
    pub name: String,
    /// Backend tag — one of `"virtual"`, `"serial"`, `"hil_bridge"`.
    pub backend: String,
    /// Operandum identifiers this apparatus can emit response events for.
    pub operanda: Vec<String>,
    /// Reinforcer channel names accepted by
    /// [`Apparatus::deliver_reinforcer`].
    pub reinforcers: Vec<String>,
    /// Stimulus identifiers accepted by [`Apparatus::set_stimulus`].
    pub stimuli: Vec<String>,
}

/// Runtime connection state for an apparatus.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ApparatusStatus {
    /// `true` between successful `connect` and `disconnect` calls.
    pub connected: bool,
    /// Human-readable message for the most recent transport error,
    /// or `None` if none has occurred since the last reset.
    pub last_error: Option<String>,
}

/// Hardware-agnostic operant-box interface.
///
/// Backends must implement every method below. `now` arguments are
/// caller-supplied monotonic timestamps (unit-agnostic, same convention
/// as [`ResponseEvent`]). The HAL does not read a wall clock; the
/// caller chooses the time source.
pub trait Apparatus: Send {
    /// Static description of this apparatus.
    fn info(&self) -> ApparatusInfo;

    /// Current connection state and last error (if any).
    fn status(&self) -> ApparatusStatus;

    /// Open the transport and make the apparatus ready for I/O.
    fn connect(&mut self) -> Result<()>;

    /// Close the transport. Idempotent.
    fn disconnect(&mut self) -> Result<()>;

    /// Drain pending response events.
    ///
    /// Non-blocking. Returns an empty vector if no responses are
    /// currently buffered. Returns a `Hardware` error if the apparatus
    /// is not connected.
    fn poll_responses(&mut self, now: f64) -> Result<Vec<ResponseEvent>>;

    /// Fire a reinforcer on the named channel.
    fn deliver_reinforcer(
        &mut self,
        now: f64,
        reinforcer: &Reinforcer,
        channel: &str,
    ) -> Result<()>;

    /// Turn a stimulus (light, tone, cue) on or off.
    fn set_stimulus(&mut self, now: f64, stimulus: &str, on: bool) -> Result<()>;
}

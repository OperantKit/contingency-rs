//! Hardware Abstraction Layer for the contingency engine.
//!
//! A thin, synchronous I/O interface for reading response events from an
//! operant apparatus and delivering reinforcers / stimuli. The HAL is
//! deliberately decoupled from [`crate::schedule::Schedule`]: a session
//! runner is responsible for feeding [`Apparatus::poll_responses`]
//! output into `Schedule::step` and delivering any resulting
//! [`crate::types::Reinforcer`] payloads back to the apparatus.
//!
//! # Backends
//!
//! * [`VirtualApparatus`] — in-memory, fully deterministic; used by
//!   tests and offline simulation.
//! * [`serial_backend::SerialApparatus`] (feature `serial`) — line-oriented
//!   serial transport backed by the `serialport` crate.
//!
//! # HIL bridge
//!
//! There is intentionally no `HilBridgeApparatus` library type. The
//! HIL bridge is implemented by the `contingency-hil` binary
//! (see `src/bin/hil.rs`); consumers drive it via the JSONL TCP
//! protocol documented there. The Python-side `HilBridgeApparatus`
//! corresponds to the *client* of that binary.
//!
//! # Error semantics
//!
//! All backends raise
//! [`ContingencyError::Hardware`](crate::errors::ContingencyError::Hardware)
//! for "not connected" conditions, unknown channel names, malformed I/O,
//! and transport-level failures. This matches the Python package's
//! design decision to fold `NotConnectedError` into the `Hardware`
//! variant rather than introducing a new error kind.

pub mod protocols;
pub mod virtual_apparatus;

#[cfg(feature = "serial")]
pub mod serial_backend;

pub use protocols::{Apparatus, ApparatusInfo, ApparatusStatus};
pub use virtual_apparatus::{LoggedEvent, LogKind, LogPayload, VirtualApparatus};

use crate::errors::ContingencyError;

/// Helper: construct a `Hardware` error for the "not connected" case.
///
/// Matches the Python package's `NotConnectedError` message prefix.
#[inline]
pub(crate) fn not_connected(name: &str) -> ContingencyError {
    ContingencyError::Hardware(format!("{name} is not connected"))
}

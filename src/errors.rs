//! Error taxonomy for the contingency engine.
//!
//! Mirrors the Python `contingency.errors` hierarchy:
//! `ContingencyError { Config, State, Hardware }`.

use thiserror::Error;

/// Crate-level `Result` alias.
pub type Result<T> = std::result::Result<T, ContingencyError>;

/// Errors raised by schedule construction, stepping, or HAL I/O.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ContingencyError {
    /// Construction-time parameter validation failure.
    #[error("invalid schedule configuration: {0}")]
    Config(String),

    /// Runtime state violation — non-monotonic time, event/now
    /// mismatch, unknown operandum.
    #[error("inconsistent schedule state: {0}")]
    State(String),

    /// Hardware Abstraction Layer I/O failure.
    #[error("hardware error: {0}")]
    Hardware(String),
}

//! contingency — reinforcement schedule engine.
//!
//! Rust port of the `contingency-py` Python package. The Python
//! package's conformance corpus is the authoritative semantic
//! specification — see `../contingency-py/conformance/`.

#![deny(clippy::undocumented_unsafe_blocks)]
#![warn(missing_docs)]

pub mod constants;
pub mod errors;
pub mod helpers;
pub mod schedule;
pub mod schedules;
pub mod types;

#[cfg(feature = "python")]
pub mod python;

// `uniffi_api` module is a planned follow-up phase (KMP / Swift / Kotlin
// bindings). The `uniffi` feature flag is reserved in `Cargo.toml` so the
// scaffolding can land without a breaking interface change, but no module
// is wired up yet — enabling the feature currently compiles to a no-op.

pub use constants::TIME_TOL;
pub use errors::{ContingencyError, Result};
pub use schedule::{ArmableSchedule, Schedule};
pub use types::{Observation, Outcome, Reinforcer, ResponseEvent};

//! contingency — reinforcement schedule engine.
//!
//! Rust port of the `contingency-py` Python package. The Python
//! package's conformance corpus is the authoritative semantic
//! specification — see `../contingency-py/conformance/`.
//!
//! # `no_std`
//!
//! The schedule core (`schedules`, `helpers`, `schedule`, `types`,
//! `builder`, `errors`) builds without the standard library when the
//! default `std` feature is turned off; it then needs only `alloc`
//! (a global allocator supplied by the final binary). Without `std`,
//! every randomised schedule must be given a seed, because there is no
//! OS entropy to draw from. The HAL (`hw`), the C ABI (`ffi`) and the
//! language bindings require `std`.

#![cfg_attr(not(feature = "std"), no_std)]
#![deny(clippy::undocumented_unsafe_blocks)]
#![warn(missing_docs)]

extern crate alloc;

pub mod builder;
pub mod constants;
pub mod errors;
pub mod helpers;
#[cfg(feature = "std")]
pub mod hw;
mod math;
pub mod prelude;

#[cfg(all(feature = "std", feature = "c-runtime"))]
compile_error!("feature `c-runtime` is for `no_std` builds; build with --no-default-features");

#[cfg(all(not(feature = "std"), feature = "c-runtime"))]
mod c_runtime;
pub mod schedule;
pub mod schedules;
pub mod types;

pub use builder::ScheduleBuilder;

#[cfg(feature = "python")]
pub mod python;

#[cfg(all(feature = "std", not(target_arch = "wasm32")))]
pub mod ffi;

#[cfg(target_arch = "wasm32")]
pub mod wasm;

#[cfg(feature = "uniffi")]
pub mod uniffi_api;

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();

pub use constants::TIME_TOL;
pub use errors::{ContingencyError, Result};
pub use schedule::{ArmableSchedule, Schedule};
pub use types::{Observation, Outcome, Reinforcer, ResponseEvent};

/// Compiles and runs every `rust` code block in `README.md` as a doctest.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
pub struct ReadmeDoctests;

/// Compiles and runs every `rust` code block in `README.ja.md` as a doctest.
#[cfg(doctest)]
#[doc = include_str!("../README.ja.md")]
pub struct ReadmeJaDoctests;

//! Reinforcement schedule implementations.
//!
//! Mirrors `contingency.schedules` in the Python port. Schedule
//! families are added per file (ratio, interval, time_based,
//! concurrent, sequence, alternative, differential, progressive) and
//! re-exported from this module.

pub mod ratio;
pub use ratio::*;

pub mod interval;
pub use interval::*;

pub mod time_based;
pub use time_based::*;

pub mod alternative;
pub use alternative::*;

pub mod concurrent;
pub use concurrent::*;

pub mod sequence;
pub use sequence::*;

pub mod differential;
pub use differential::*;

pub mod progressive;
pub use progressive::*;

pub mod timeout;
pub use timeout::*;

pub mod response_cost;
pub use response_cost::*;

pub mod adjusting;
pub use adjusting::*;

pub mod interlocking;
pub use interlocking::*;

pub mod second_order;
pub use second_order::*;

pub mod percentile;
pub use percentile::*;

pub mod conjunctive;
pub use conjunctive::*;

pub mod mixed;
pub use mixed::*;

pub mod overlay;
pub use overlay::*;

pub mod interpolate;
pub use interpolate::*;

pub mod trial_based;
pub use trial_based::*;

pub mod aversive;
pub use aversive::*;

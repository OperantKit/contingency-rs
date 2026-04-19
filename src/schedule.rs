//! The `Schedule` trait — single abstraction every schedule implements.
//!
//! A schedule is a stateful object driven forward by `step`. Each step
//! advances an internal clock to `now` and optionally registers a
//! `ResponseEvent`. The schedule returns an `Outcome` describing
//! whether the step produced reinforcement.
//!
//! Mirrors `contingency.interfaces.Schedule` in Python.

use crate::{Outcome, ResponseEvent, Result};

/// A reinforcement schedule.
///
/// # Thread safety
///
/// The trait requires `Send` so schedules may be moved across threads
/// and held inside a `Mutex` behind a UniFFI `Arc`-shared object. All
/// concrete implementations in this crate satisfy `Send` naturally
/// (they hold only `f64` / integers / `SmallRng` / `Vec` / `Box<dyn
/// Schedule>` fields, none of which introduce `!Send` state).
pub trait Schedule: Send {
    /// Advance to `now` and optionally register a response.
    ///
    /// # Contract
    ///
    /// - `now` must be `>=` the previous step's `now` (within `TIME_TOL`).
    ///   Non-monotonic input returns `Err(ContingencyError::State(..))`.
    /// - If `event` is `Some`, `event.time` must equal `now` within
    ///   `TIME_TOL`.
    /// - The returned `Outcome` obeys the invariant that `reinforced`
    ///   iff `reinforcer.is_some()`.
    fn step(&mut self, now: f64, event: Option<&ResponseEvent>) -> Result<Outcome>;

    /// Return the schedule to its post-construction state.
    fn reset(&mut self);
}

/// Interval-family schedules wrappable by `LimitedHold` expose two
/// protected hooks. Kept as a separate trait (not part of `Schedule`)
/// so `LimitedHold` can wrap any implementation without leaking the
/// internals into the public `Schedule` surface.
pub trait ArmableSchedule: Schedule {
    /// Absolute monotonic time at which the currently armed interval
    /// elapses.
    fn arm_time(&self) -> f64;

    /// Resample the next interval anchored at `now`, *without*
    /// delivering a reinforcer. Used by `LimitedHold` to withdraw a
    /// missed opportunity.
    fn withdraw_and_rearm(&mut self, now: f64);
}

// -----------------------------------------------------------------------
// Blanket impls for boxed trait objects.
//
// These let `Box<dyn Schedule>` and `Box<dyn ArmableSchedule>` satisfy
// generic bounds like `S: Schedule` / `S: ArmableSchedule`. Without them
// a caller who type-erases a schedule (e.g. in the Python bindings) is
// unable to re-instantiate `LimitedHold<S>` over the erased value.
// -----------------------------------------------------------------------

impl<T: Schedule + ?Sized> Schedule for Box<T> {
    fn step(&mut self, now: f64, event: Option<&ResponseEvent>) -> Result<Outcome> {
        (**self).step(now, event)
    }

    fn reset(&mut self) {
        (**self).reset()
    }
}

impl<T: ArmableSchedule + ?Sized> ArmableSchedule for Box<T> {
    fn arm_time(&self) -> f64 {
        (**self).arm_time()
    }

    fn withdraw_and_rearm(&mut self, now: f64) {
        (**self).withdraw_and_rearm(now)
    }
}

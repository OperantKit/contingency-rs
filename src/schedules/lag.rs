//! Lag schedule — differential reinforcement of response variability
//! (Page & Neuringer, 1985).
//!
//! A *unit* is `length` consecutive responses, read as the sequence of
//! the operanda they were emitted on. Units do not overlap: every
//! `length`-th response closes a unit. At the end of a unit the schedule
//! reinforces iff the unit differs from each of the previous `n` units.
//! While fewer than `n` units have been completed, the unit is compared
//! with the ones there are (the first unit is always reinforced). Every
//! completed unit enters the comparison list, reinforced or not.
//! `Lag 0` reinforces every unit. Responses inside a unit are never
//! reinforced; ticks without a response do nothing.
//!
//! On a single operandum every unit is the same, so `Lag n` with
//! `n >= 1` reinforces only the first unit.
//!
//! # References
//!
//! Page, S., & Neuringer, A. (1985). Variability is an operant.
//! *Journal of Experimental Psychology: Animal Behavior Processes*,
//! 11(3), 429-452. <https://doi.org/10.1037/0097-7403.11.3.429>

#[allow(unused_imports)]
use crate::prelude::*;

use alloc::collections::{BTreeMap, VecDeque};

use crate::errors::ContingencyError;
use crate::helpers::checks::{check_event, check_time};
use crate::schedule::Schedule;
use crate::types::{MetaValue, Outcome, Reinforcer, ResponseEvent};
use crate::Result;

/// Largest `n` and `length` a [`Lag`] accepts (`u32::MAX`).
pub const LAG_MAX: usize = u32::MAX as usize;

/// Lag `n` schedule over units of `length` responses.
#[derive(Clone, Debug)]
pub struct Lag {
    n: usize,
    length: usize,
    current: Vec<String>,
    history: VecDeque<Vec<String>>,
    last_now: Option<f64>,
}

impl Lag {
    /// Construct a Lag schedule.
    ///
    /// # Errors
    ///
    /// [`ContingencyError::Config`] when `length < 1`, or when `n` or
    /// `length` exceeds [`LAG_MAX`].
    pub fn new(n: usize, length: usize) -> Result<Self> {
        if length < 1 {
            return Err(ContingencyError::Config(format!(
                "Lag length must be >= 1, got {length}"
            )));
        }
        if n > LAG_MAX || length > LAG_MAX {
            return Err(ContingencyError::Config(format!(
                "Lag n and length must be <= {LAG_MAX}, got n={n}, length={length}"
            )));
        }
        Ok(Self {
            n,
            length,
            current: Vec::new(),
            history: VecDeque::new(),
            last_now: None,
        })
    }

    /// Number of previous units the current unit must differ from.
    pub fn n(&self) -> usize {
        self.n
    }

    /// Number of responses in one unit.
    pub fn length(&self) -> usize {
        self.length
    }
}

impl Schedule for Lag {
    fn step(&mut self, now: f64, event: Option<&ResponseEvent>) -> Result<Outcome> {
        check_time(now, self.last_now)?;
        check_event(now, event)?;
        self.last_now = Some(now);

        let Some(ev) = event else {
            return Ok(Outcome::empty());
        };
        self.current.push(ev.operandum.clone());
        if self.current.len() < self.length {
            return Ok(Outcome::empty());
        }

        let unit = core::mem::take(&mut self.current);
        let met = !self.history.iter().any(|prev| *prev == unit);
        if self.n > 0 {
            if self.history.len() == self.n {
                self.history.pop_front();
            }
            self.history.push_back(unit);
        }
        if met {
            Ok(Outcome::reinforced(Reinforcer::at(now)))
        } else {
            Ok(Outcome::empty())
        }
    }

    fn state(&self) -> BTreeMap<String, MetaValue> {
        let mut state = BTreeMap::new();
        state.insert(
            "current_unit".to_string(),
            MetaValue::StrList(self.current.clone()),
        );
        state.insert(
            "units_compared".to_string(),
            MetaValue::Int(self.history.len() as i64),
        );
        state
    }

    fn reset(&mut self) {
        self.current.clear();
        self.history.clear();
        self.last_now = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_zero_length() {
        assert!(matches!(Lag::new(1, 0), Err(ContingencyError::Config(_))));
    }

    #[test]
    fn rejects_values_over_the_bound_without_allocating() {
        assert!(Lag::new(LAG_MAX, LAG_MAX).is_ok());
        assert!(matches!(
            Lag::new(LAG_MAX + 1, 1),
            Err(ContingencyError::Config(_))
        ));
        assert!(matches!(
            Lag::new(1, LAG_MAX + 1),
            Err(ContingencyError::Config(_))
        ));
    }

    #[test]
    fn history_is_bounded_by_n() {
        let mut s = Lag::new(2, 1).unwrap();
        for (i, op) in ["a", "b", "c", "d"].iter().enumerate() {
            let now = i as f64;
            s.step(now, Some(&ResponseEvent::on(*op, now))).unwrap();
        }
        assert_eq!(s.state().get("units_compared"), Some(&MetaValue::Int(2)));
        // "a" has left the list of the last two units (c, d).
        let o = s.step(9.0, Some(&ResponseEvent::on("a", 9.0))).unwrap();
        assert!(o.reinforced);
    }
}

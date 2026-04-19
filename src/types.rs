//! Core value objects.
//!
//! All types are immutable (`Clone + Debug + PartialEq`). Time is
//! unit-agnostic `f64` on a monotonic clock — unit is declared by the
//! caller (typically the DSL meta or `experiment-io`).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A response emitted by the subject at `time`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResponseEvent {
    /// Monotonic timestamp of the response.
    pub time: f64,
    /// Identifier of the operandum (lever, key, button).
    pub operandum: String,
}

impl ResponseEvent {
    /// Construct a response event, defaulting the operandum to `"main"`.
    pub fn new(time: f64) -> Self {
        Self {
            time,
            operandum: "main".into(),
        }
    }

    /// Construct a response event on a named operandum.
    pub fn on(operandum: impl Into<String>, time: f64) -> Self {
        Self {
            time,
            operandum: operandum.into(),
        }
    }
}

/// A discrete-time snapshot of the subject's cumulative state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct Observation {
    /// Time of the observation.
    pub time: f64,
    /// Cumulative response count at `time`.
    pub response_count: u64,
}

/// A scheduled reinforcement event.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Reinforcer {
    /// Delivery time (monotonic).
    pub time: f64,
    /// Delivery magnitude. Negative magnitudes encode punishers.
    pub magnitude: f64,
    /// Label — canonically `"SR+"`, `"SR-"` for punisher.
    pub label: String,
}

impl Reinforcer {
    /// Construct a default reinforcer (`magnitude = 1.0, label = "SR+"`).
    pub fn at(time: f64) -> Self {
        Self {
            time,
            magnitude: 1.0,
            label: "SR+".into(),
        }
    }
}

/// Optional arbitrary metadata value attached to an `Outcome`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MetaValue {
    /// Boolean flag.
    Bool(bool),
    /// Signed integer (covers counts and enum-like indices).
    Int(i64),
    /// Floating-point number.
    Float(f64),
    /// String value.
    Str(String),
}

/// Result returned by `Schedule::step`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct Outcome {
    /// Whether the step produced a reinforcement delivery.
    pub reinforced: bool,
    /// The scheduled reinforcer, or `None` when `reinforced == false`.
    pub reinforcer: Option<Reinforcer>,
    /// Backend-specific payload. Empty by default. Keyed-ordered map
    /// so serialisation is stable.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub meta: BTreeMap<String, MetaValue>,
}

impl Outcome {
    /// Non-reinforced outcome with empty meta.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Reinforced outcome wrapping the given reinforcer.
    pub fn reinforced(reinforcer: Reinforcer) -> Self {
        Self {
            reinforced: true,
            reinforcer: Some(reinforcer),
            meta: BTreeMap::new(),
        }
    }

    /// Attach a meta key/value; returns self for chaining.
    pub fn with_meta(mut self, key: impl Into<String>, value: MetaValue) -> Self {
        self.meta.insert(key.into(), value);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_event_default_operandum() {
        let e = ResponseEvent::new(1.0);
        assert_eq!(e.operandum, "main");
        assert_eq!(e.time, 1.0);
    }

    #[test]
    fn response_event_named_operandum() {
        let e = ResponseEvent::on("left", 2.5);
        assert_eq!(e.operandum, "left");
    }

    #[test]
    fn reinforcer_default_label_and_magnitude() {
        let r = Reinforcer::at(1.0);
        assert_eq!(r.label, "SR+");
        assert_eq!(r.magnitude, 1.0);
    }

    #[test]
    fn outcome_empty_is_not_reinforced() {
        let o = Outcome::empty();
        assert!(!o.reinforced);
        assert!(o.reinforcer.is_none());
        assert!(o.meta.is_empty());
    }

    #[test]
    fn outcome_reinforced_carries_reinforcer() {
        let r = Reinforcer::at(5.0);
        let o = Outcome::reinforced(r.clone());
        assert!(o.reinforced);
        assert_eq!(o.reinforcer.as_ref(), Some(&r));
    }

    #[test]
    fn outcome_with_meta_chains() {
        let o = Outcome::empty().with_meta("cod_suppressed", MetaValue::Bool(true));
        assert_eq!(o.meta.len(), 1);
    }
}

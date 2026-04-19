//! Overlay compound schedule — punishment overlay (Azrin & Holz, 1966).
//!
//! An Overlay runs two schedules in parallel on the same operandum: a
//! primary reinforcement schedule (`base`) and a punishment schedule
//! (`punisher`). Every `step` call forwards `(now, event)` to both.
//! Whatever `base` delivers is surfaced as a positive reinforcer;
//! whatever `punisher` delivers is re-labelled as a punisher (`SR-`)
//! with its magnitude negated.
//!
//! A single step may emit:
//! * nothing;
//! * a positive reinforcement (base only);
//! * a punishment (punisher only);
//! * a compound outcome (both fired same step) — meta carries
//!   `base_fired`, `punisher_fired`, `net_magnitude`. The surfaced
//!   `Reinforcer` magnitude equals `net = base - punisher` with label
//!   `SR+` when `net > 0`, `SR-` when `net < 0`, `SR0` when `0`.
//!
//! # References
//!
//! Azrin, N. H., & Holz, W. C. (1966). Punishment. In W. K. Honig
//! (Ed.), *Operant behavior: Areas of research and application*
//! (pp. 380-447). Appleton-Century-Crofts.

use crate::helpers::checks::{check_event, check_time};
use crate::schedule::Schedule;
use crate::types::{MetaValue, Outcome, Reinforcer, ResponseEvent};
use crate::Result;

/// Overlay (punishment overlay) compound schedule.
pub struct Overlay {
    base: Box<dyn Schedule>,
    punisher: Box<dyn Schedule>,
    last_now: Option<f64>,
}

impl Overlay {
    /// Construct an overlay of `base` and `punisher`.
    pub fn new(base: Box<dyn Schedule>, punisher: Box<dyn Schedule>) -> Result<Self> {
        Ok(Self {
            base,
            punisher,
            last_now: None,
        })
    }
}

impl Schedule for Overlay {
    fn step(&mut self, now: f64, event: Option<&ResponseEvent>) -> Result<Outcome> {
        check_time(now, self.last_now)?;
        check_event(now, event)?;
        self.last_now = Some(now);

        let base_outcome = self.base.step(now, event)?;
        let punisher_outcome = self.punisher.step(now, event)?;

        let base_fired = base_outcome.reinforced;
        let punisher_fired = punisher_outcome.reinforced;

        if !base_fired && !punisher_fired {
            return Ok(Outcome::empty());
        }

        if base_fired && !punisher_fired {
            let mut out = Outcome {
                reinforced: true,
                reinforcer: base_outcome.reinforcer,
                ..Outcome::default()
            };
            out.meta
                .insert("base_fired".to_string(), MetaValue::Bool(true));
            out.meta
                .insert("punisher_fired".to_string(), MetaValue::Bool(false));
            return Ok(out);
        }

        if punisher_fired && !base_fired {
            let p = punisher_outcome
                .reinforcer
                .expect("punisher reinforced but no reinforcer");
            let mut out = Outcome {
                reinforced: true,
                reinforcer: Some(Reinforcer {
                    time: p.time,
                    magnitude: -p.magnitude,
                    label: "SR-".to_string(),
                }),
                ..Outcome::default()
            };
            out.meta
                .insert("base_fired".to_string(), MetaValue::Bool(false));
            out.meta
                .insert("punisher_fired".to_string(), MetaValue::Bool(true));
            return Ok(out);
        }

        // Both fired — surface a compound outcome with net magnitude.
        let b = base_outcome
            .reinforcer
            .expect("base reinforced but no reinforcer");
        let p = punisher_outcome
            .reinforcer
            .expect("punisher reinforced but no reinforcer");
        let net = b.magnitude - p.magnitude;
        let label = if net > 0.0 {
            "SR+"
        } else if net < 0.0 {
            "SR-"
        } else {
            "SR0"
        };
        let mut out = Outcome {
            reinforced: true,
            reinforcer: Some(Reinforcer {
                time: b.time,
                magnitude: net,
                label: label.to_string(),
            }),
            ..Outcome::default()
        };
        out.meta
            .insert("base_fired".to_string(), MetaValue::Bool(true));
        out.meta
            .insert("punisher_fired".to_string(), MetaValue::Bool(true));
        out.meta
            .insert("net_magnitude".to_string(), MetaValue::Float(net));
        Ok(out)
    }

    fn reset(&mut self) {
        self.base.reset();
        self.punisher.reset();
        self.last_now = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::TIME_TOL;
    use crate::errors::ContingencyError;
    use crate::schedules::{EXT, FR};

    /// Minimal Schedule that fires exactly once at `fire_at`, with
    /// configurable magnitude and label.
    struct OneShotSchedule {
        fire_at: f64,
        magnitude: f64,
        label: String,
        fired: bool,
        last_now: Option<f64>,
    }

    impl OneShotSchedule {
        fn new(fire_at: f64, magnitude: f64, label: &str) -> Self {
            Self {
                fire_at,
                magnitude,
                label: label.to_string(),
                fired: false,
                last_now: None,
            }
        }
    }

    impl Schedule for OneShotSchedule {
        fn step(
            &mut self,
            now: f64,
            event: Option<&ResponseEvent>,
        ) -> Result<Outcome> {
            check_time(now, self.last_now)?;
            check_event(now, event)?;
            self.last_now = Some(now);
            if !self.fired && (now - self.fire_at).abs() <= TIME_TOL {
                self.fired = true;
                return Ok(Outcome::reinforced(Reinforcer {
                    time: now,
                    magnitude: self.magnitude,
                    label: self.label.clone(),
                }));
            }
            Ok(Outcome::empty())
        }
        fn reset(&mut self) {
            self.fired = false;
            self.last_now = None;
        }
    }

    fn ev(t: f64) -> ResponseEvent {
        ResponseEvent::new(t)
    }

    // --- Config / construction ----------------------------------------

    #[test]
    fn accepts_two_schedules() {
        let ov = Overlay::new(
            Box::new(FR::new(5).unwrap()),
            Box::new(FR::new(10).unwrap()),
        );
        assert!(ov.is_ok());
    }

    // --- Step semantics -----------------------------------------------

    #[test]
    fn neither_fires_returns_empty_outcome() {
        let mut ov = Overlay::new(
            Box::new(FR::new(5).unwrap()),
            Box::new(FR::new(5).unwrap()),
        )
        .unwrap();
        let out = ov.step(0.0, Some(&ev(0.0))).unwrap();
        assert!(!out.reinforced);
        assert!(out.reinforcer.is_none());
    }

    #[test]
    fn base_only_passes_through_positive_reinforcer() {
        let mut ov = Overlay::new(Box::new(FR::new(1).unwrap()), Box::new(EXT::new())).unwrap();
        let out = ov.step(0.0, Some(&ev(0.0))).unwrap();
        assert!(out.reinforced);
        let r = out.reinforcer.as_ref().unwrap();
        assert!((r.magnitude - 1.0).abs() < 1e-9);
        assert_eq!(r.label, "SR+");
        assert_eq!(out.meta.get("base_fired"), Some(&MetaValue::Bool(true)));
        assert_eq!(
            out.meta.get("punisher_fired"),
            Some(&MetaValue::Bool(false))
        );
    }

    #[test]
    fn punisher_only_emits_negative_labelled_sr_minus() {
        let mut ov = Overlay::new(Box::new(EXT::new()), Box::new(FR::new(1).unwrap())).unwrap();
        let out = ov.step(1.0, Some(&ev(1.0))).unwrap();
        assert!(out.reinforced);
        let r = out.reinforcer.as_ref().unwrap();
        assert!((r.magnitude - (-1.0)).abs() < 1e-9);
        assert_eq!(r.label, "SR-");
        assert_eq!(
            out.meta.get("punisher_fired"),
            Some(&MetaValue::Bool(true))
        );
        assert_eq!(out.meta.get("base_fired"), Some(&MetaValue::Bool(false)));
    }

    #[test]
    fn both_fire_compound_meta_and_positive_net() {
        let mut ov = Overlay::new(
            Box::new(FR::new(1).unwrap()),
            Box::new(OneShotSchedule::new(0.0, 0.3, "SR+")),
        )
        .unwrap();
        let out = ov.step(0.0, Some(&ev(0.0))).unwrap();
        assert!(out.reinforced);
        assert_eq!(out.meta.get("base_fired"), Some(&MetaValue::Bool(true)));
        assert_eq!(
            out.meta.get("punisher_fired"),
            Some(&MetaValue::Bool(true))
        );
        match out.meta.get("net_magnitude") {
            Some(MetaValue::Float(v)) => assert!((*v - 0.7).abs() < 1e-9),
            other => panic!("net_magnitude missing/wrong: {:?}", other),
        }
        let r = out.reinforcer.as_ref().unwrap();
        assert!((r.magnitude - 0.7).abs() < 1e-9);
        assert_eq!(r.label, "SR+");
    }

    #[test]
    fn both_fire_negative_net_labels_sr_minus() {
        let mut ov = Overlay::new(
            Box::new(FR::new(1).unwrap()),
            Box::new(OneShotSchedule::new(0.0, 5.0, "SR+")),
        )
        .unwrap();
        let out = ov.step(0.0, Some(&ev(0.0))).unwrap();
        assert!(out.reinforced);
        let r = out.reinforcer.as_ref().unwrap();
        assert!((r.magnitude - (-4.0)).abs() < 1e-9);
        assert_eq!(r.label, "SR-");
    }

    #[test]
    fn both_fire_zero_net_labels_sr0() {
        let mut ov = Overlay::new(
            Box::new(FR::new(1).unwrap()),
            Box::new(OneShotSchedule::new(0.0, 1.0, "SR+")),
        )
        .unwrap();
        let out = ov.step(0.0, Some(&ev(0.0))).unwrap();
        assert!(out.reinforced);
        let r = out.reinforcer.as_ref().unwrap();
        assert!(r.magnitude.abs() < 1e-9);
        assert_eq!(r.label, "SR0");
    }

    #[test]
    fn tick_without_event_returns_no_reinforcement_for_ratio() {
        let mut ov = Overlay::new(
            Box::new(FR::new(2).unwrap()),
            Box::new(FR::new(3).unwrap()),
        )
        .unwrap();
        let out = ov.step(5.0, None).unwrap();
        assert!(!out.reinforced);
    }

    #[test]
    fn non_monotonic_time_raises() {
        let mut ov = Overlay::new(
            Box::new(FR::new(1).unwrap()),
            Box::new(FR::new(2).unwrap()),
        )
        .unwrap();
        ov.step(1.0, Some(&ev(1.0))).unwrap();
        let err = ov.step(0.5, Some(&ev(0.5))).unwrap_err();
        assert!(matches!(err, ContingencyError::State(_)));
    }

    #[test]
    fn event_time_mismatch_raises() {
        let mut ov = Overlay::new(
            Box::new(FR::new(1).unwrap()),
            Box::new(FR::new(2).unwrap()),
        )
        .unwrap();
        let bad = ResponseEvent::new(0.9);
        let err = ov.step(1.0, Some(&bad)).unwrap_err();
        assert!(matches!(err, ContingencyError::State(_)));
    }

    #[test]
    fn reset_clears_both_components() {
        let mut ov = Overlay::new(
            Box::new(FR::new(2).unwrap()),
            Box::new(FR::new(2).unwrap()),
        )
        .unwrap();
        ov.step(0.0, Some(&ev(0.0))).unwrap();
        ov.reset();
        let out = ov.step(1.0, Some(&ev(1.0))).unwrap();
        assert!(!out.reinforced);
    }

    #[test]
    fn sequence_fr_base_only_pattern() {
        let mut ov =
            Overlay::new(Box::new(FR::new(3).unwrap()), Box::new(EXT::new())).unwrap();
        for t in [1.0, 2.0] {
            assert!(!ov.step(t, Some(&ev(t))).unwrap().reinforced);
        }
        let out = ov.step(3.0, Some(&ev(3.0))).unwrap();
        assert!(out.reinforced);
        assert_eq!(out.reinforcer.as_ref().unwrap().label, "SR+");
    }

    #[test]
    fn sequence_fr_punisher_only_pattern() {
        let mut ov =
            Overlay::new(Box::new(EXT::new()), Box::new(FR::new(3).unwrap())).unwrap();
        for t in [1.0, 2.0] {
            assert!(!ov.step(t, Some(&ev(t))).unwrap().reinforced);
        }
        let out = ov.step(3.0, Some(&ev(3.0))).unwrap();
        assert!(out.reinforced);
        assert_eq!(out.reinforcer.as_ref().unwrap().label, "SR-");
    }

    #[test]
    fn repeated_cycles_alternate_correctly() {
        let mut ov = Overlay::new(
            Box::new(FR::new(2).unwrap()),
            Box::new(FR::new(3).unwrap()),
        )
        .unwrap();
        ov.step(1.0, Some(&ev(1.0))).unwrap();
        let out = ov.step(2.0, Some(&ev(2.0))).unwrap();
        assert!(out.reinforced);
        assert_eq!(out.meta.get("base_fired"), Some(&MetaValue::Bool(true)));
        let out = ov.step(3.0, Some(&ev(3.0))).unwrap();
        assert!(out.reinforced);
        assert_eq!(
            out.meta.get("punisher_fired"),
            Some(&MetaValue::Bool(true))
        );
        assert_eq!(out.meta.get("base_fired"), Some(&MetaValue::Bool(false)));
    }
}

//! Alternative compound schedule — whichever-first of two components.
//!
//! An `Alternative` schedule presents two component schedules
//! concurrently on a single operandum. Both components run in parallel;
//! reinforcement is delivered as soon as **either** component's
//! criterion is met. When a reinforcer is delivered, **both**
//! components are reset so that the next cycle starts cleanly.
//!
//! The canonical example combines a response-based schedule with a
//! time-based schedule, e.g. `Alternative::new(Box::new(FR::new(10)?),
//! Box::new(FT::new(60.0)?))` — the subject is reinforced after the
//! 10th response *or* after 60 seconds, whichever comes first.
//!
//! # Binary shape
//!
//! `Alternative` is strictly binary. To compose three or more
//! components, nest Alternatives:
//! `Alternative::new(Box::new(Alternative::new(a, b)), c)`.
//! Keeping the shape binary simplifies reasoning about tie-breaking and
//! avoids an ambiguous ordering over arbitrary-arity components.
//!
//! # Event forwarding
//!
//! On every [`Schedule::step`] call, the provided `now` and `event` are
//! forwarded to **both** components unchanged. This is the semantics
//! required by the definition: a response that satisfies `FR(10)` does
//! not *affect* `FT(60)`'s decision (`FT` is response-independent), but
//! `FT` must still advance its clock to `now` and validate that
//! `event.time == now`. Both components therefore maintain their own
//! internal state independently; the `Alternative` wrapper only gates
//! which outcome is surfaced.
//!
//! # Tie-breaking
//!
//! If both components would reinforce on the same step — a rare but
//! possible situation — the first component wins deterministically. The
//! second component is still reset along with the first.
//!
//! # Reset semantics
//!
//! After a win, [`Schedule::reset`] is invoked on **both** components so
//! that each returns to its post-construction state. An external
//! [`Schedule::reset`] call clears the same state plus the Alternative's
//! own monotonic-time bookkeeping.
//!
//! # References
//!
//! Ferster, C. B., & Skinner, B. F. (1957). *Schedules of
//! reinforcement*. Appleton-Century-Crofts.

use crate::helpers::checks::{check_event, check_time};
use crate::schedule::Schedule;
use crate::types::{MetaValue, Outcome, ResponseEvent};
use crate::Result;

/// Alternative compound schedule of two components (whichever-first).
///
/// Both components are stepped on every call with the same `now` and
/// `event`. If either returns a reinforced [`Outcome`], both components
/// are reset and the reinforced outcome is propagated with
/// `meta["alternative_winner"]` set to `"first"` or `"second"`.
/// If both would reinforce simultaneously, the first component wins;
/// both are still reset.
///
/// # References
///
/// Ferster, C. B., & Skinner, B. F. (1957). *Schedules of
/// reinforcement*. Appleton-Century-Crofts.
pub struct Alternative {
    first: Box<dyn Schedule>,
    second: Box<dyn Schedule>,
    last_now: Option<f64>,
}

impl Alternative {
    /// Construct an Alternative of two component schedules.
    ///
    /// `first` wins on simultaneous satisfaction.
    pub fn new(first: Box<dyn Schedule>, second: Box<dyn Schedule>) -> Self {
        Self {
            first,
            second,
            last_now: None,
        }
    }
}

impl Schedule for Alternative {
    fn step(&mut self, now: f64, event: Option<&ResponseEvent>) -> Result<Outcome> {
        check_time(now, self.last_now)?;
        check_event(now, event)?;
        self.last_now = Some(now);

        // Forward the same (now, event) to both components so each
        // maintains its own state independently.
        let first_outcome = self.first.step(now, event)?;
        let second_outcome = self.second.step(now, event)?;

        if first_outcome.reinforced {
            self.first.reset();
            self.second.reset();
            let mut out = Outcome {
                reinforced: true,
                reinforcer: first_outcome.reinforcer,
                ..Outcome::default()
            };
            out.meta.insert(
                "alternative_winner".to_string(),
                MetaValue::Str("first".to_string()),
            );
            return Ok(out);
        }
        if second_outcome.reinforced {
            self.first.reset();
            self.second.reset();
            let mut out = Outcome {
                reinforced: true,
                reinforcer: second_outcome.reinforcer,
                ..Outcome::default()
            };
            out.meta.insert(
                "alternative_winner".to_string(),
                MetaValue::Str("second".to_string()),
            );
            return Ok(out);
        }
        Ok(Outcome::empty())
    }

    fn reset(&mut self) {
        self.first.reset();
        self.second.reset();
        self.last_now = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errors::ContingencyError;
    use crate::schedules::{FR, FT};

    fn respond(s: &mut Alternative, now: f64) -> Outcome {
        let ev = ResponseEvent::new(now);
        s.step(now, Some(&ev)).expect("step should succeed")
    }

    fn winner(o: &Outcome) -> Option<&str> {
        match o.meta.get("alternative_winner") {
            Some(MetaValue::Str(s)) => Some(s.as_str()),
            _ => None,
        }
    }

    // --- FR + FT: whichever-first ---------------------------------------

    #[test]
    fn fr_wins_on_third_response_then_ft_wins_after_reset() {
        // Alt(FR(3), FT(5.0)): response-win on 3rd response, then after
        // reset, a 5-s silent window should trigger FT.
        let mut alt = Alternative::new(
            Box::new(FR::new(3).unwrap()),
            Box::new(FT::new(5.0).unwrap()),
        );

        // FR counts responses; FT anchors on first step and waits 5 s.
        let o1 = respond(&mut alt, 1.0);
        assert!(!o1.reinforced);
        let o2 = respond(&mut alt, 2.0);
        assert!(!o2.reinforced);
        let o3 = respond(&mut alt, 3.0);
        assert!(o3.reinforced);
        assert_eq!(winner(&o3), Some("first"));
        assert_eq!(o3.reinforcer.as_ref().unwrap().time, 3.0);

        // After the FR win, both components were reset. A plain step at
        // t=3.0 re-anchors FT; no response there so FR stays at 0.
        let o_anchor = alt.step(3.0, None).unwrap();
        assert!(!o_anchor.reinforced);

        // No response for 5 s → FT fires at t=8.0.
        let o_time = alt.step(8.0, None).unwrap();
        assert!(o_time.reinforced);
        assert_eq!(winner(&o_time), Some("second"));
    }

    // --- FR(10) + FT(1.0): FR can beat FT when responses are dense --------

    #[test]
    fn fr10_beats_ft1_with_20hz_responding() {
        // 10 responses in 0.5 s (20 Hz) — FR(10) fires on the 10th
        // before FT(1.0) ever gets the chance.
        let mut alt = Alternative::new(
            Box::new(FR::new(10).unwrap()),
            Box::new(FT::new(1.0).unwrap()),
        );

        let mut fired_at: Option<f64> = None;
        for i in 1..=20 {
            let t = i as f64 * 0.05; // 20 Hz
            let o = respond(&mut alt, t);
            if o.reinforced {
                assert_eq!(winner(&o), Some("first"));
                fired_at = Some(t);
                break;
            }
        }
        assert!(fired_at.is_some());
        let t = fired_at.unwrap();
        assert!((t - 0.5).abs() < 1e-9, "expected first fire at t=0.5, got {t}");
    }

    #[test]
    fn ft1_beats_fr10_with_2hz_responding() {
        // Alt(FR(10), FT(1.0)) stepped every 0.5 s with a response —
        // at t=1.0 only 3 responses have occurred, but FT has elapsed.
        let mut alt = Alternative::new(
            Box::new(FR::new(10).unwrap()),
            Box::new(FT::new(1.0).unwrap()),
        );

        let ev0 = ResponseEvent::new(0.0);
        let o0 = alt.step(0.0, Some(&ev0)).unwrap();
        assert!(!o0.reinforced);

        let ev_half = ResponseEvent::new(0.5);
        let o_half = alt.step(0.5, Some(&ev_half)).unwrap();
        assert!(!o_half.reinforced);

        let ev1 = ResponseEvent::new(1.0);
        let o_win = alt.step(1.0, Some(&ev1)).unwrap();
        assert!(o_win.reinforced);
        assert_eq!(winner(&o_win), Some("second"));
    }

    // --- Tie-breaking ---------------------------------------------------

    #[test]
    fn tie_break_first_wins_on_simultaneous_satisfaction() {
        // FR(1) fires on any response; FT(0.001) fires on any step past
        // its interval. Anchor FT at t=0, then respond at t=0.002:
        // both would fire but first (FR) must win.
        let mut alt = Alternative::new(
            Box::new(FR::new(1).unwrap()),
            Box::new(FT::new(0.001).unwrap()),
        );

        let o_anchor = alt.step(0.0, None).unwrap();
        assert!(!o_anchor.reinforced);

        let o = respond(&mut alt, 0.002);
        assert!(o.reinforced);
        assert_eq!(winner(&o), Some("first"));
    }

    #[test]
    fn tie_break_resets_both_components() {
        // After a tie-break win on "first", the second component must
        // also have been reset. We detect this by observing that FT's
        // anchor is cleared: a subsequent plain step does not fire
        // until a full FT interval elapses from the re-anchor.
        let mut alt = Alternative::new(
            Box::new(FR::new(1).unwrap()),
            Box::new(FT::new(0.001).unwrap()),
        );
        alt.step(0.0, None).unwrap();
        // Tie at t=0.002: first wins.
        let o = respond(&mut alt, 0.002);
        assert!(o.reinforced);
        assert_eq!(winner(&o), Some("first"));

        // FT was reset. A plain step at t=0.002 re-anchors; a plain step
        // at t=0.003 (elapsed 0.001 ≥ interval) should fire on FT as
        // "second" — confirming FT was reset and re-anchored, rather
        // than still holding its pre-win anchor.
        let o_anchor = alt.step(0.002, None).unwrap();
        assert!(!o_anchor.reinforced);
        let o_next = alt.step(0.003, None).unwrap();
        assert!(o_next.reinforced);
        assert_eq!(winner(&o_next), Some("second"));
    }

    // --- Reset ---------------------------------------------------------

    #[test]
    fn reset_clears_both_components_and_bookkeeping() {
        let mut alt = Alternative::new(
            Box::new(FR::new(3).unwrap()),
            Box::new(FT::new(5.0).unwrap()),
        );

        alt.step(0.0, None).unwrap();
        respond(&mut alt, 1.0);
        respond(&mut alt, 2.0);

        alt.reset();

        // After reset, last_now is cleared so we can step at an earlier
        // absolute time without a monotonicity error.
        let o_anchor = alt.step(10.0, None).unwrap();
        assert!(!o_anchor.reinforced);

        // FR count was cleared: need 3 *new* responses to fire.
        respond(&mut alt, 11.0);
        respond(&mut alt, 12.0);
        let o_win = respond(&mut alt, 13.0);
        assert!(o_win.reinforced);
        assert_eq!(winner(&o_win), Some("first"));
    }

    #[test]
    fn win_resets_fr_component_counter() {
        // FR(3) + long FT. After the first FR3 win, a second cycle must
        // also require exactly 3 more responses.
        let mut alt = Alternative::new(
            Box::new(FR::new(3).unwrap()),
            Box::new(FT::new(100.0).unwrap()),
        );

        respond(&mut alt, 1.0);
        respond(&mut alt, 2.0);
        let o_win1 = respond(&mut alt, 3.0);
        assert!(o_win1.reinforced);
        assert_eq!(winner(&o_win1), Some("first"));

        // Second cycle.
        let o4 = respond(&mut alt, 4.0);
        assert!(!o4.reinforced);
        let o5 = respond(&mut alt, 5.0);
        assert!(!o5.reinforced);
        let o6 = respond(&mut alt, 6.0);
        assert!(o6.reinforced);
        assert_eq!(winner(&o6), Some("first"));
    }

    // --- Winner meta ---------------------------------------------------

    #[test]
    fn winner_meta_first_set_correctly() {
        let mut alt = Alternative::new(
            Box::new(FR::new(1).unwrap()),
            Box::new(FT::new(100.0).unwrap()),
        );
        let o = respond(&mut alt, 0.5);
        assert!(o.reinforced);
        assert_eq!(winner(&o), Some("first"));
    }

    #[test]
    fn winner_meta_second_set_correctly() {
        let mut alt = Alternative::new(
            Box::new(FR::new(100).unwrap()),
            Box::new(FT::new(1.0).unwrap()),
        );
        let o_anchor = alt.step(0.0, None).unwrap();
        assert!(!o_anchor.reinforced);
        let o = alt.step(1.0, None).unwrap();
        assert!(o.reinforced);
        assert_eq!(winner(&o), Some("second"));
    }

    #[test]
    fn non_reinforced_outcome_has_no_winner_meta() {
        let mut alt = Alternative::new(
            Box::new(FR::new(3).unwrap()),
            Box::new(FT::new(5.0).unwrap()),
        );
        let o = alt.step(0.5, None).unwrap();
        assert!(!o.reinforced);
        assert!(!o.meta.contains_key("alternative_winner"));
    }

    // --- Event forwarded to both components ---------------------------

    #[test]
    fn event_forwarded_to_both_components() {
        // Second component is FR — if the event were dropped, FR would
        // never count responses and would never fire.
        let mut alt = Alternative::new(
            Box::new(FT::new(100.0).unwrap()),
            Box::new(FR::new(3).unwrap()),
        );

        respond(&mut alt, 1.0);
        respond(&mut alt, 2.0);
        let o = respond(&mut alt, 3.0);
        assert!(o.reinforced);
        assert_eq!(winner(&o), Some("second"));
    }

    // --- Nested alternatives (3+ components) --------------------------

    #[test]
    fn nested_alternatives_chain_three_components() {
        // Alt(Alt(FR3, FT100), FR5): first matchable is FR3.
        let inner = Alternative::new(
            Box::new(FR::new(3).unwrap()),
            Box::new(FT::new(100.0).unwrap()),
        );
        let mut outer = Alternative::new(Box::new(inner), Box::new(FR::new(5).unwrap()));

        respond(&mut outer, 1.0);
        respond(&mut outer, 2.0);
        let o = respond(&mut outer, 3.0);
        assert!(o.reinforced);
        // Inner alternative wins → outer reports "first".
        assert_eq!(winner(&o), Some("first"));
    }

    // --- State errors --------------------------------------------------

    #[test]
    fn rejects_non_monotonic_now() {
        let mut alt = Alternative::new(
            Box::new(FR::new(3).unwrap()),
            Box::new(FT::new(5.0).unwrap()),
        );
        alt.step(1.0, None).unwrap();
        let err = alt.step(0.5, None).unwrap_err();
        assert!(matches!(err, ContingencyError::State(_)));
    }

    #[test]
    fn rejects_event_time_not_equal_now() {
        let mut alt = Alternative::new(
            Box::new(FR::new(3).unwrap()),
            Box::new(FT::new(5.0).unwrap()),
        );
        let ev = ResponseEvent::new(2.0);
        let err = alt.step(1.0, Some(&ev)).unwrap_err();
        assert!(matches!(err, ContingencyError::State(_)));
    }
}

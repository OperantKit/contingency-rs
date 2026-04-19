//! Property-based tests using `proptest` for invariant checks that
//! complement seeded determinism tests and the conformance replay.

use indexmap::IndexMap;
use proptest::prelude::*;

use contingency::{helpers, schedules, ResponseEvent, Schedule};

// ---------------------------------------------------------------------------
// Fleshler-Hoffman mean preservation
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn fh_intervals_mean_preserved(
        v in 1.0_f64..1000.0,
        n in 1_usize..50,
        seed: u64,
    ) {
        let out = helpers::fleshler_hoffman::generate_intervals(v, n, Some(seed));
        prop_assert_eq!(out.len(), n);
        let sum: f64 = out.iter().copied().sum();
        // Implementation rescales to hit the target sum exactly.
        prop_assert!((sum - v * n as f64).abs() < 1e-9);
        // All intervals are positive and finite.
        for &x in &out {
            prop_assert!(x.is_finite());
            prop_assert!(x > 0.0);
        }
    }

    #[test]
    fn fh_ratios_mean_preserved(
        v in 2_u64..200,
        n in 2_usize..30,
        seed: u64,
    ) {
        let out = helpers::fleshler_hoffman::generate_ratios(v as f64, n, Some(seed));
        prop_assert_eq!(out.len(), n);
        let sum: u64 = out.iter().copied().sum();
        prop_assert_eq!(sum, v * n as u64);
        // All ratios >= 1.
        for &r in &out {
            prop_assert!(r >= 1);
        }
    }
}

// ---------------------------------------------------------------------------
// RR probability convergence (Monte Carlo)
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn rr_probability_converges_within_tolerance(
        p in 0.05_f64..0.95,
        seed: u64,
    ) {
        let mut rr = schedules::RR::new(p, Some(seed)).unwrap();
        let trials = 5000_u64;
        let mut hits = 0_u64;
        for i in 1..=trials {
            let t = i as f64;
            let out = rr.step(t, Some(&ResponseEvent::new(t))).unwrap();
            if out.reinforced {
                hits += 1;
            }
        }
        let observed = hits as f64 / trials as f64;
        // Binomial std err: sqrt(p(1-p)/n). Accept 6 sigma to keep
        // false failure rate negligible across proptest's case volume.
        let sigma = (p * (1.0 - p) / trials as f64).sqrt();
        prop_assert!((observed - p).abs() < 6.0 * sigma + 0.01);
    }
}

// ---------------------------------------------------------------------------
// DRL within-interval invariant
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn drl_within_interval_never_reinforces(
        interval in 0.1_f64..100.0,
        irt_frac in 0.01_f64..0.9,
    ) {
        let irt = interval * irt_frac;
        // The tolerance band TIME_TOL means we also need irt to be
        // meaningfully smaller than the interval.
        prop_assume!(interval - irt > 1e-6);
        let mut drl = schedules::DRL::new(interval).unwrap();
        let r1 = drl.step(1.0, Some(&ResponseEvent::new(1.0))).unwrap();
        prop_assert!(r1.reinforced); // first response always reinforces
        let r2 = drl
            .step(1.0 + irt, Some(&ResponseEvent::new(1.0 + irt)))
            .unwrap();
        prop_assert!(!r2.reinforced);
    }

    #[test]
    fn drl_after_interval_reinforces(
        interval in 0.1_f64..100.0,
        extra in 0.01_f64..5.0,
    ) {
        let mut drl = schedules::DRL::new(interval).unwrap();
        let _ = drl.step(1.0, Some(&ResponseEvent::new(1.0))).unwrap();
        let t2 = 1.0 + interval + extra;
        let r2 = drl.step(t2, Some(&ResponseEvent::new(t2))).unwrap();
        prop_assert!(r2.reinforced);
    }
}

// ---------------------------------------------------------------------------
// FR exact-count firing
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn fr_fires_on_exactly_n_responses(n in 1_u64..200) {
        let mut fr = schedules::FR::new(n).unwrap();
        for i in 1..n {
            let t = i as f64;
            prop_assert!(!fr.step(t, Some(&ResponseEvent::new(t))).unwrap().reinforced);
        }
        let t = n as f64;
        prop_assert!(fr.step(t, Some(&ResponseEvent::new(t))).unwrap().reinforced);
    }
}

// ---------------------------------------------------------------------------
// Monotonic time invariant
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn non_monotonic_time_always_errors(
        start in 1.0_f64..100.0,
        back_delta in 0.001_f64..10.0,
    ) {
        let mut s = schedules::FR::new(1).unwrap();
        s.step(start, Some(&ResponseEvent::new(start))).unwrap();
        prop_assert!(s.step(start - back_delta, None).is_err());
    }
}

// ---------------------------------------------------------------------------
// Seeded determinism
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn vr_seeded_is_deterministic(mean in 2.0_f64..100.0, seed: u64) {
        let mut a = schedules::VR::new(mean, 12, Some(seed)).unwrap();
        let mut b = schedules::VR::new(mean, 12, Some(seed)).unwrap();
        for i in 1..=50 {
            let t = i as f64;
            let ev = ResponseEvent::new(t);
            let ra = a.step(t, Some(&ev)).unwrap().reinforced;
            let rb = b.step(t, Some(&ev)).unwrap().reinforced;
            prop_assert_eq!(ra, rb);
        }
    }

    #[test]
    fn vi_seeded_is_deterministic(mean in 0.5_f64..50.0, seed: u64) {
        let mut a = schedules::VI::new(mean, 12, Some(seed)).unwrap();
        let mut b = schedules::VI::new(mean, 12, Some(seed)).unwrap();
        for i in 1..=100 {
            let t = i as f64 * 0.5;
            let ev = ResponseEvent::new(t);
            let ra = a.step(t, Some(&ev)).unwrap().reinforced;
            let rb = b.step(t, Some(&ev)).unwrap().reinforced;
            prop_assert_eq!(ra, rb);
        }
    }

    #[test]
    fn rr_seeded_is_deterministic(p in 0.05_f64..0.95, seed: u64) {
        let mut a = schedules::RR::new(p, Some(seed)).unwrap();
        let mut b = schedules::RR::new(p, Some(seed)).unwrap();
        for i in 1..=200 {
            let t = i as f64;
            let ev = ResponseEvent::new(t);
            let ra = a.step(t, Some(&ev)).unwrap().reinforced;
            let rb = b.step(t, Some(&ev)).unwrap().reinforced;
            prop_assert_eq!(ra, rb);
        }
    }
}

// ---------------------------------------------------------------------------
// LimitedHold missed-window property
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn limited_hold_fi_misses_after_hold(
        interval in 1.0_f64..50.0,
        hold in 0.1_f64..5.0,
        overshoot in 0.1_f64..10.0,
    ) {
        let inner = schedules::FI::new(interval).unwrap();
        let mut lh = schedules::LimitedHold::new(inner, hold).unwrap();
        // Step once well before the interval so the clock anchors.
        let t_pre = interval * 0.1;
        let _ = lh.step(t_pre, Some(&ResponseEvent::new(t_pre))).unwrap();
        // Response after (interval + hold + overshoot) is outside the
        // hold window: no reinforcement regardless of arming.
        let t = interval + hold + overshoot;
        let out = lh.step(t, Some(&ResponseEvent::new(t))).unwrap();
        prop_assert!(!out.reinforced);
    }
}

// ---------------------------------------------------------------------------
// DRH rate-window invariant: under the threshold never reinforces
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn drh_below_threshold_never_reinforces(
        response_count in 3_u32..15,
        window in 1.0_f64..10.0,
        count in 1_u32..3,
    ) {
        // `count` strictly below `response_count` — by definition cannot
        // meet the rate criterion.
        prop_assume!(count < response_count);
        let mut drh = schedules::DRH::new(response_count, window).unwrap();
        for i in 1..=count {
            let t = window * 0.1 * i as f64;
            let out = drh.step(t, Some(&ResponseEvent::new(t))).unwrap();
            prop_assert!(!out.reinforced);
        }
    }
}

// ---------------------------------------------------------------------------
// Concurrent: sum of outcomes is at most 1 (single-choice invariant)
// ---------------------------------------------------------------------------

proptest! {
    #[test]
    fn concurrent_fr_does_not_double_reinforce(
        a_n in 1_u64..10,
        b_n in 1_u64..10,
        seed: u64,
    ) {
        let mut map: IndexMap<String, Box<dyn Schedule>> = IndexMap::new();
        map.insert("left".into(), Box::new(schedules::FR::new(a_n).unwrap()));
        map.insert("right".into(), Box::new(schedules::FR::new(b_n).unwrap()));
        let mut conc = schedules::Concurrent::new(map, 0.0, 0).unwrap();
        // Drive deterministic alternating responses using seed to pick
        // the operandum key. No COD, so every response should count.
        let mut rng_state = seed;
        for i in 1..=50 {
            let t = i as f64;
            rng_state = rng_state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let op = if (rng_state >> 33) & 1 == 0 { "left" } else { "right" };
            let ev = ResponseEvent { time: t, operandum: op.to_string() };
            let out = conc.step(t, Some(&ev)).unwrap();
            // At most one reinforcer per step — concurrent never
            // produces two simultaneously.
            if out.reinforced {
                prop_assert!(out.reinforcer.is_some());
            }
        }
    }
}

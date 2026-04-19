//! Fleshler & Hoffman (1962) variable-schedule generators.
//!
//! # References
//!
//! - Fleshler, M., & Hoffman, H. S. (1962). A progression for generating
//!   variable-interval schedules. *Journal of the Experimental Analysis of
//!   Behavior*, 5(4), 529-530. <https://doi.org/10.1901/jeab.1962.5-529>
//! - Hantula, D. A. (1991). A simple BASIC program to generate values for
//!   variable-interval schedules of reinforcement. *Journal of Applied
//!   Behavior Analysis*, 24(4), 799-801.
//!   <https://doi.org/10.1901/jaba.1991.24-799>
//!
//! # Determinism
//!
//! RNG is `SmallRng` seeded from an optional `u64`. The shuffle uses
//! `slice::shuffle`. The Python port uses Python's `random.Random`
//! (Mersenne Twister); bit-equivalent output across ports is NOT
//! guaranteed for stochastic schedules. Conformance fixtures treat
//! those as trajectory templates rather than exact replays.

use rand::rngs::SmallRng;
use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};
#[allow(unused_imports)]
use rand::prelude::*;

fn make_rng(seed: Option<u64>) -> SmallRng {
    match seed {
        Some(s) => SmallRng::seed_from_u64(s),
        None => SmallRng::from_entropy(),
    }
}

/// Raw Fleshler-Hoffman progression (unshuffled).
pub fn progression(v: f64, n: usize) -> Vec<f64> {
    if n == 0 {
        return Vec::new();
    }
    let dn = n as f64;
    let mut out = Vec::with_capacity(n);
    for m in 1..=n {
        let dm = m as f64;
        let value = if m == n {
            v * (1.0 + dn.ln())
        } else {
            let s1 = (1.0 + dn.ln()) + (dn - dm) * (dn - dm).ln();
            let s2 = (dn - dm + 1.0) * (dn - dm + 1.0).ln();
            v * (s1 - s2)
        };
        out.push(value);
    }
    out
}

/// Generate a Variable-Interval (VI) schedule with mean `v`.
///
/// Preserves the floating-point mean exactly (within rounding) by
/// adjusting element 0 before shuffling, mirroring the Python impl's
/// remnant correction.
pub fn generate_intervals(v: f64, n: usize, seed: Option<u64>) -> Vec<f64> {
    if n == 0 {
        return Vec::new();
    }
    let mut raw = progression(v, n);
    let target = v * n as f64;
    let actual: f64 = raw.iter().copied().sum();
    raw[0] += target - actual;
    let mut rng = make_rng(seed);
    raw.shuffle(&mut rng);
    raw
}

/// Generate a Variable-Ratio (VR) schedule with mean `v`.
///
/// Each value is rounded to a positive integer (`>= 1`) and the final
/// element absorbs any surplus so that the integer mean stays exactly
/// `v`. If the surplus would be non-positive, walk the list from the
/// tail decrementing entries `>= 2` until balance is restored —
/// matches the Python fallback.
pub fn generate_ratios(v: f64, n: usize, seed: Option<u64>) -> Vec<u64> {
    if n == 0 {
        return Vec::new();
    }
    let raw = progression(v, n);
    let mut rd: Vec<i64> = raw.iter().map(|x| x.round().max(1.0) as i64).collect();
    let target_total = (v * n as f64).round() as i64;
    let head_sum: i64 = rd[..n - 1].iter().sum();
    let mut surplus = target_total - head_sum;
    if surplus >= 1 {
        rd[n - 1] = surplus;
    } else {
        rd[n - 1] = 1;
        surplus -= 1;
        for i in (0..n).rev() {
            if rd[i] >= 2 {
                rd[i] -= 1;
                surplus += 1;
                if surplus >= 0 {
                    break;
                }
            }
        }
    }
    let mut out: Vec<u64> = rd.into_iter().map(|x| x as u64).collect();
    let mut rng = make_rng(seed);
    out.shuffle(&mut rng);
    out
}

/// Hantula (1991) BASIC-program variant of the FH progression.
///
/// Same arithmetic as `generate_intervals`; difference is in the
/// random-placement loop, which Hantula expressed as a GOTO retry.
pub fn generate_intervals_hantula1991(v: f64, n: usize, seed: Option<u64>) -> Vec<u64> {
    if n == 0 {
        return Vec::new();
    }
    let mut rng = make_rng(seed);
    let mut rd: Vec<u64> = vec![0; n];
    let raw = progression(v, n);
    for (_m, raw_val) in raw.iter().enumerate().take(n) {
        let value = raw_val.round() as i64;
        let value = if value == 0 { 1 } else { value as u64 };
        loop {
            let order = rng.gen_range(0..n);
            if rd[order] == 0 {
                rd[order] = value;
                break;
            }
        }
    }
    rd
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progression_empty_for_zero() {
        assert_eq!(progression(30.0, 0), Vec::<f64>::new());
    }

    #[test]
    fn progression_last_element_formula() {
        let n = 12;
        let v = 30.0;
        let raw = progression(v, n);
        let expected = v * (1.0 + (n as f64).ln());
        assert!((raw[n - 1] - expected).abs() < 1e-9);
    }

    #[test]
    fn generate_intervals_mean_preserved() {
        let v = 30.0;
        let n = 12;
        let out = generate_intervals(v, n, Some(0));
        let sum: f64 = out.iter().copied().sum();
        assert!((sum - v * n as f64).abs() < 1e-9);
    }

    #[test]
    fn generate_intervals_deterministic_under_seed() {
        let a = generate_intervals(30.0, 12, Some(42));
        let b = generate_intervals(30.0, 12, Some(42));
        assert_eq!(a, b);
    }

    #[test]
    fn generate_intervals_seed_differs() {
        let a = generate_intervals(30.0, 12, Some(1));
        let b = generate_intervals(30.0, 12, Some(2));
        assert_ne!(a, b);
    }

    #[test]
    fn generate_ratios_mean_preserved() {
        let v = 30.0;
        let n = 12;
        let out = generate_ratios(v, n, Some(0));
        let sum: u64 = out.iter().copied().sum();
        assert_eq!(sum, (v * n as f64).round() as u64);
    }

    #[test]
    fn generate_ratios_positive() {
        for x in generate_ratios(30.0, 12, Some(0)) {
            assert!(x >= 1);
        }
    }

    #[test]
    fn hantula_fills_all_slots() {
        let out = generate_intervals_hantula1991(30.0, 12, Some(0));
        assert_eq!(out.len(), 12);
        assert!(out.iter().all(|&x| x > 0));
    }

    #[test]
    fn hantula_mean_within_rounding_tolerance() {
        let v = 30.0;
        let n = 12;
        let out = generate_intervals_hantula1991(v, n, Some(0));
        let sum: u64 = out.iter().copied().sum();
        let deviation = (sum as f64 - v * n as f64).abs();
        assert!(deviation <= n as f64 / 2.0);
    }

    #[test]
    fn empty_cases() {
        assert!(generate_intervals(30.0, 0, Some(0)).is_empty());
        assert!(generate_ratios(30.0, 0, Some(0)).is_empty());
        assert!(generate_intervals_hantula1991(30.0, 0, Some(0)).is_empty());
    }
}

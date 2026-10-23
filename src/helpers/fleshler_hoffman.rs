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
//! RNG is [`PyRandom`], a port of Python's `random.Random`, seeded from
//! an optional `u64` the same way `random.Random(seed)` is. The shuffle
//! is `random.shuffle` and the Hantula placement is `randrange(0, n)`,
//! so a given seed yields the same sequence as the Python port. The
//! remaining arithmetic follows the Python port too: the VI remnant
//! correction sums with [`fsum`] (correctly rounded, like `math.fsum`)
//! and integer rounding is round-half-to-even (like Python `round`).

use super::py_random::PyRandom;

fn make_rng(seed: Option<u64>) -> PyRandom {
    PyRandom::from_optional_seed(seed)
}

/// Correctly rounded sum of `values`, matching Python's `math.fsum`
/// for finite inputs.
///
/// Shewchuk's algorithm: keep a list of non-overlapping partial sums
/// whose exact total equals the running sum, then add them from the
/// largest down and fix the last bit for half-way cases, as CPython's
/// `math_fsum` does. A plain left-to-right sum can differ from
/// `math.fsum` in the last ulp, which would shift element 0 of a VI
/// sequence away from the Python value.
fn fsum(values: &[f64]) -> f64 {
    let mut partials: Vec<f64> = Vec::new();
    for &value in values {
        let mut x = value;
        let mut kept = 0;
        for j in 0..partials.len() {
            let mut y = partials[j];
            if x.abs() < y.abs() {
                std::mem::swap(&mut x, &mut y);
            }
            let hi = x + y;
            let lo = y - (hi - x);
            if lo != 0.0 {
                partials[kept] = lo;
                kept += 1;
            }
            x = hi;
        }
        partials.truncate(kept);
        partials.push(x);
    }
    let mut n = partials.len();
    if n == 0 {
        return 0.0;
    }
    n -= 1;
    let mut hi = partials[n];
    let mut lo = 0.0;
    while n > 0 {
        let x = hi;
        n -= 1;
        let y = partials[n];
        hi = x + y;
        let yr = hi - x;
        lo = y - yr;
        if lo != 0.0 {
            break;
        }
    }
    if n > 0 && ((lo < 0.0 && partials[n - 1] < 0.0) || (lo > 0.0 && partials[n - 1] > 0.0)) {
        let y = lo * 2.0;
        let x = hi + y;
        let yr = x - hi;
        if y == yr {
            hi = x;
        }
    }
    hi
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
/// remnant correction (`raw[0] += v * n - math.fsum(raw)`).
pub fn generate_intervals(v: f64, n: usize, seed: Option<u64>) -> Vec<f64> {
    if n == 0 {
        return Vec::new();
    }
    let mut raw = progression(v, n);
    let target = v * n as f64;
    let actual = fsum(&raw);
    raw[0] += target - actual;
    make_rng(seed).shuffle(&mut raw);
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
    let mut rd: Vec<i64> = raw
        .iter()
        .map(|x| x.round_ties_even().max(1.0) as i64)
        .collect();
    let target_total = (v * n as f64).round_ties_even() as i64;
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
    make_rng(seed).shuffle(&mut out);
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
        let value = raw_val.round_ties_even() as i64;
        let value = if value == 0 { 1 } else { value as u64 };
        loop {
            let order = rng.randbelow(n as u64) as usize;
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

    // Expected values below were produced by contingency-py's
    // `helpers.fleshler_hoffman` on CPython 3.11 with the same arguments.

    #[test]
    fn fsum_is_correctly_rounded() {
        assert_eq!(fsum(&[0.1; 10]), 1.0);
        assert_eq!(
            fsum(&[1e100, 1.0, -1e100, 1e-100, 1e50, -1.0, -1e50]),
            1e-100
        );
        assert_eq!(fsum(&[]), 0.0);
    }

    #[test]
    fn generate_intervals_matches_python_where_fsum_differs_from_naive_sum() {
        // For (v, n) = (0.7, 15) a left-to-right sum of the progression
        // differs from `math.fsum` in the last bits, so element 0's
        // remnant correction (shuffled to index 2 here) pins `fsum`.
        let expected: [u64; 15] = [
            0x4004c3dc5b4fb27c,
            0x3feb03e7a005fec1,
            0x3f98715726694999,
            0x3ffa00f03756a2d1,
            0x3fd978b75e122eb9,
            0x3fd47bc524e2c380,
            0x3fc7d5d2e180f599,
            0x3fe2c10bc44cc7e9,
            0x3fdf1612b980e406,
            0x3ff0566987e7af8c,
            0x3fcffdf11f175ecc,
            0x3fb2ebed5d400c66,
            0x3ff424ae677cd3ee,
            0x3fe68140ff5bcca9,
            0x3fc05c2379802833,
        ];
        let got: Vec<u64> = generate_intervals(0.7, 15, Some(1))
            .iter()
            .map(|x| x.to_bits())
            .collect();
        assert_eq!(got, expected);
    }

    #[test]
    fn generate_ratios_matches_python() {
        assert_eq!(
            generate_ratios(7.5, 9, Some(5)),
            vec![2, 4, 1, 1, 24, 14, 10, 7, 5]
        );
        assert_eq!(generate_ratios(2.5, 4, Some(0)), vec![3, 1, 1, 5]);
        assert_eq!(generate_ratios(5.5, 4, Some(0)), vec![6, 1, 3, 12]);
    }

    #[test]
    fn generate_intervals_hantula1991_matches_python() {
        assert_eq!(
            generate_intervals_hantula1991(10.0, 12, Some(3)),
            vec![12, 10, 3, 1, 16, 5, 35, 6, 2, 1, 8, 21]
        );
    }
}

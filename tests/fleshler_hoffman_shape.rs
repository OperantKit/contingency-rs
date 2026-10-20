//! Shape of the Fleshler-Hoffman distribution, and agreement with the
//! Python port.
//!
//! Fleshler, M., & Hoffman, H. S. (1962). A progression for generating
//! variable-interval schedules. Journal of the Experimental Analysis of
//! Behavior, 5(4), 529-530. https://doi.org/10.1901/jeab.1962.5-529
//!
//! Equation (2), with the constant [-ln(1 - p)]^-1 replaced by the VI mean T:
//!
//!     t_n = T * [1 + ln N + (N - n) ln(N - n) - (N - n + 1) ln(N - n + 1)]
//!     t_N = T * (1 + ln N)                      (limit, n = N)
//!
//! t_n is the mean of the n-th of N equal-area classes of the exponential
//! distribution with mean T, so class n spans
//!
//!     [-T ln(1 - (n - 1)/N), -T ln(1 - n/N)].
//!
//! Hantula, D. A. (1991). A simple BASIC program to generate values for
//! variable-interval schedules of reinforcement. Journal of Applied Behavior
//! Analysis, 24(4), 799-801. https://doi.org/10.1901/jaba.1991.24-799
//!
//! Hantula restates the same equation and places each t_n at a random
//! unused slot. The BASIC listing stores t_n unrounded; the Swift original
//! (and this port) rounds each slot to an integer.
//!
//! Generators only shuffle the progression, so the multiset of values is
//! deterministic and is compared as a sorted list.

use proptest::prelude::*;

use contingency::helpers::fleshler_hoffman::{
    generate_intervals, generate_intervals_hantula1991, generate_ratios, progression,
};

/// Equation (2) of Fleshler & Hoffman (1962), coded separately from the module.
///
/// The float operations happen in the same order as `progression`, so the
/// two agree bit for bit; the Hantula rounding check relies on that.
fn closed_form(t: f64, n_terms: usize) -> Vec<f64> {
    let big_n = n_terms as f64;
    (1..=n_terms)
        .map(|n| {
            if n == n_terms {
                return t * (1.0 + big_n.ln());
            }
            let k = big_n - n as f64;
            t * (1.0 + big_n.ln() + k * k.ln() - (k + 1.0) * (k + 1.0).ln())
        })
        .collect()
}

fn sorted_f64(mut xs: Vec<f64>) -> Vec<f64> {
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    xs
}

fn sorted_u64(mut xs: Vec<u64>) -> Vec<u64> {
    xs.sort_unstable();
    xs
}

fn assert_close(actual: &[f64], expected: &[f64], rel: f64, abs: f64) {
    assert_eq!(actual.len(), expected.len());
    for (i, (a, e)) in actual.iter().zip(expected).enumerate() {
        let tol = abs.max(rel * e.abs());
        assert!(
            (a - e).abs() <= tol,
            "term {i}: {a} vs {e} (|diff| {} > {tol})",
            (a - e).abs()
        );
    }
}

// Values printed in Fleshler & Hoffman (1962, p. 530) for VI 30 s, N = 12.
// The printed values deviate from Equation (2) by up to 0.0023 s with no
// consistent sign (term 3: printed 7.020, equation 7.0223), which is larger
// than 3-decimal rounding; the tolerance below allows for that.
const PUBLISHED_VI30_N12: [f64; 12] = [
    1.287, 4.017, 7.020, 10.364, 14.123, 18.423, 23.447, 29.486, 37.067, 47.261, 62.958, 104.547,
];
const PUBLISHED_ABS_TOL: f64 = 0.003;

#[test]
fn vi30_n12_matches_published_progression() {
    for seed in [0_u64, 1, 42, 12345] {
        let out = sorted_f64(generate_intervals(30.0, 12, Some(seed)));
        assert_close(&out, &PUBLISHED_VI30_N12, 0.0, PUBLISHED_ABS_TOL);
    }
}

#[test]
fn closed_form_matches_published_progression() {
    assert_close(
        &closed_form(30.0, 12),
        &PUBLISHED_VI30_N12,
        0.0,
        PUBLISHED_ABS_TOL,
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn intervals_are_the_closed_form_set(
        v in 0.5_f64..600.0,
        n in 1_usize..=200,
        seed: u64,
    ) {
        // generate_intervals folds the float residual of sum(t_n) - v*n into
        // one term to keep the mean exact, so allow an error proportional to v*n.
        let out = sorted_f64(generate_intervals(v, n, Some(seed)));
        assert_close(&out, &sorted_f64(closed_form(v, n)), 1e-12, 1e-12 * v * n as f64);
    }

    #[test]
    fn hantula1991_is_the_rounded_closed_form_set(
        v in 1_u32..=300,
        n in 1_usize..=50,
        seed: u64,
    ) {
        let out = sorted_u64(generate_intervals_hantula1991(v as f64, n, Some(seed)));
        let expected = sorted_u64(
            closed_form(v as f64, n).iter().map(|x| (x.round() as u64).max(1)).collect(),
        );
        prop_assert_eq!(out, expected);
    }

    #[test]
    fn ratios_track_the_closed_form_set(
        v in 2_u32..=300,
        n in 2_usize..=50,
        seed: u64,
    ) {
        // VR rounds each of t_1..t_{N-1} and replaces t_N with whatever keeps
        // the integer mean exact (the Swift remnant rule). When small v and
        // large n make that surplus < 1, the Swift fallback decrements terms
        // instead; its mean is covered by the property tests in properties.rs.
        let cf = closed_form(v as f64, n);
        let head: Vec<i64> = cf[..n - 1].iter().map(|x| (x.round() as i64).max(1)).collect();
        let surplus = (v as i64) * (n as i64) - head.iter().sum::<i64>();
        prop_assume!(surplus >= 1);
        let mut expected: Vec<u64> = head.iter().map(|&x| x as u64).collect();
        expected.push(surplus as u64);
        let out = sorted_u64(generate_ratios(v as f64, n, Some(seed)));
        prop_assert_eq!(out, sorted_u64(expected));
    }
}

/// Conditional probability of reinforcement in successive equal-width bins.
///
/// For bin k = (k*width, (k+1)*width], the hazard is the number of intervals
/// ending in the bin divided by the number of intervals still running at its
/// start. Bins are evaluated while at least `min_at_risk` intervals remain.
fn hazards(values: &[f64], width: f64, min_at_risk: usize) -> Vec<f64> {
    let mut out = Vec::new();
    for k in 0.. {
        let (lo, hi) = (k as f64 * width, (k + 1) as f64 * width);
        let at_risk = values.iter().filter(|&&x| x > lo).count();
        if at_risk < min_at_risk {
            break;
        }
        let ending = values.iter().filter(|&&x| lo < x && x <= hi).count();
        out.push(ending as f64 / at_risk as f64);
    }
    out
}

#[test]
fn each_term_lies_in_its_equal_area_class() {
    let (v, n) = (30.0_f64, 200_usize);
    for (i, &t) in progression(v, n).iter().enumerate() {
        let i = i + 1;
        let lower = -v * (1.0 - (i - 1) as f64 / n as f64).ln();
        let upper = if i == n {
            f64::INFINITY
        } else {
            -v * (1.0 - i as f64 / n as f64).ln()
        };
        assert!(
            lower < t && t < upper,
            "term {i}: {t} not in ({lower}, {upper})"
        );
    }
}

#[test]
fn hazard_is_flat_across_equal_bins() {
    // Exponential with mean T: P(rf in a bin of width w | not yet) =
    // 1 - exp(-w/T), independent of the time since the last rf. The hazard
    // does not depend on order, so the seed does not matter.
    // Observed max deviation: 0.0054 / 0.0011 / 0.0025 for w = 5 / 15 / 30.
    let (v, n) = (30.0_f64, 1000_usize);
    let values = generate_intervals(v, n, Some(0));
    for width in [5.0_f64, 15.0, 30.0] {
        let expected = 1.0 - (-width / v).exp();
        let hs = hazards(&values, width, n / 10);
        assert!(hs.len() >= 3);
        let max_dev = hs.iter().map(|h| (h - expected).abs()).fold(0.0, f64::max);
        assert!(max_dev < 0.007, "width {width}: max deviation {max_dev}");
    }
}

#[test]
fn hazard_check_rejects_arithmetic_progression() {
    // Fleshler & Hoffman's counterexample: an arithmetic progression with the
    // same mean makes P(rf) rise with time since rf.
    let (v, n, width) = (30.0_f64, 1000_usize, 15.0_f64);
    let arithmetic: Vec<f64> = (0..n)
        .map(|i| 2.0 * v * (i as f64 + 0.5) / n as f64)
        .collect();
    let expected = 1.0 - (-width / v).exp();
    let hs = hazards(&arithmetic, width, n / 10);
    assert!(hs.windows(2).all(|w| w[0] <= w[1]));
    assert!(hs.iter().map(|h| (h - expected).abs()).fold(0.0, f64::max) > 0.1);
}

// ---------------------------------------------------------------------------
// Agreement with contingency-py
//
// Sorted outputs of contingency-py (`_fh_progression`, and the generators
// with seed=0), written with Python's shortest round-trip repr. Both ports
// must produce these sets for the same (v, n); the published values and the
// closed form above are the shared expectation they are derived from.
// ---------------------------------------------------------------------------

const PY_PROGRESSION: &[(f64, usize, &[f64])] = &[
    (
        30.0,
        12,
        &[
            1.2862455934221373,
            4.017287368391607,
            7.022307476205469,
            10.362533616021388,
            14.122360792095243,
            18.422772653074144,
            23.446181897705127,
            29.486835962911844,
            37.066982139383,
            47.26092434710685,
            62.9583686600433,
            104.54719949364001,
        ],
    ),
    (
        60.0,
        20,
        &[
            1.5256443981922985,
            4.684998691355418,
            8.020048822722288,
            11.551498826088675,
            15.303944055038556,
            19.306912298067402,
            23.596278356421152,
            28.21622544060105,
            33.2220286128036,
            38.68411216274254,
            44.69415237837026,
            51.3746046580021,
            58.89425901014981,
            67.49508273210772,
            77.54190122136968,
            89.62320935178312,
            104.78350170472544,
            125.17138612017312,
            156.56627474604602,
            239.74393641323945,
        ],
    ),
    (
        5.5,
        7,
        &[
            0.41302756570048427,
            1.3339859272161645,
            2.4414391725040527,
            3.8311326381904416,
            5.700022042939813,
            8.577886833644826,
            16.202505819804223,
        ],
    ),
    (1.0, 1, &[1.0]),
    (
        200.0,
        3,
        &[37.813956756734335, 142.46358550964388, 419.722457733622],
    ),
];

// (v, n, sorted generate_intervals_hantula1991, sorted generate_ratios)
const PY_INTEGER_SETS: &[(u32, usize, &[u64], &[u64])] = &[
    (
        30,
        12,
        &[1, 4, 7, 10, 14, 18, 23, 29, 37, 47, 63, 105],
        &[1, 4, 7, 10, 14, 18, 23, 29, 37, 47, 63, 107],
    ),
    (
        60,
        20,
        &[
            2, 5, 8, 12, 15, 19, 24, 28, 33, 39, 45, 51, 59, 67, 78, 90, 105, 125, 157, 240,
        ],
        &[
            2, 5, 8, 12, 15, 19, 24, 28, 33, 39, 45, 51, 59, 67, 78, 90, 105, 125, 157, 238,
        ],
    ),
    (
        10,
        40,
        &[
            1, 1, 1, 1, 1, 1, 2, 2, 2, 3, 3, 3, 4, 4, 5, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 11, 12,
            12, 13, 14, 15, 17, 18, 20, 22, 24, 28, 33, 47,
        ],
        &[
            1, 1, 1, 1, 1, 1, 2, 2, 2, 3, 3, 3, 4, 4, 5, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 11, 12,
            12, 13, 14, 15, 17, 18, 20, 22, 24, 28, 33, 47,
        ],
    ),
    (5, 7, &[1, 1, 2, 3, 5, 8, 15], &[1, 1, 2, 3, 5, 8, 15]),
    (200, 3, &[38, 142, 420], &[38, 142, 420]),
];

#[test]
fn progression_matches_python_port() {
    for &(v, n, expected) in PY_PROGRESSION {
        let out = sorted_f64(progression(v, n));
        assert_close(&out, expected, 1e-15, 0.0);
        assert_close(&sorted_f64(closed_form(v, n)), expected, 1e-15, 0.0);
    }
}

#[test]
fn intervals_match_python_port_as_sets() {
    for &(v, n, expected) in PY_PROGRESSION {
        for seed in [0_u64, 7, 99] {
            let out = sorted_f64(generate_intervals(v, n, Some(seed)));
            assert_close(&out, expected, 1e-12, 1e-12 * v * n as f64);
        }
    }
}

#[test]
fn integer_generators_match_python_port_as_sets() {
    for &(v, n, hantula, ratios) in PY_INTEGER_SETS {
        for seed in [0_u64, 7, 99] {
            assert_eq!(
                sorted_u64(generate_intervals_hantula1991(v as f64, n, Some(seed))),
                hantula,
                "hantula v={v} n={n}"
            );
            assert_eq!(
                sorted_u64(generate_ratios(v as f64, n, Some(seed))),
                ratios,
                "ratios v={v} n={n}"
            );
        }
    }
}

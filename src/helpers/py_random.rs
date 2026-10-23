//! Python-compatible pseudo-random number generator.
//!
//! A port of CPython's `random.Random`: the MT19937 core from
//! `Modules/_randommodule.c` plus the `Lib/random.py` methods that
//! `contingency-py` calls. Seeding with the same non-negative integer
//! yields the same draws, bit for bit, as `random.Random(seed)`.
//!
//! # References
//!
//! - Matsumoto, M., & Nishimura, T. (1998). Mersenne twister: A
//!   623-dimensionally equidistributed uniform pseudo-random number
//!   generator. *ACM Transactions on Modeling and Computer Simulation*,
//!   8(1), 3-30. <https://doi.org/10.1145/272991.272995>
//! - CPython `Modules/_randommodule.c` and `Lib/random.py`
//!   (<https://github.com/python/cpython>).

const N: usize = 624;
const M: usize = 397;
const MATRIX_A: u32 = 0x9908_b0df;
const UPPER_MASK: u32 = 0x8000_0000;
const LOWER_MASK: u32 = 0x7fff_ffff;

/// MT19937 state with CPython's seeding and sampling methods.
///
/// `getstate` / `setstate` are not provided; `Clone` takes a snapshot
/// that replays the same draws.
#[derive(Clone)]
pub struct PyRandom {
    state: [u32; N],
    index: usize,
}

impl std::fmt::Debug for PyRandom {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PyRandom")
            .field("state[0..4]", &&self.state[..4])
            .field("index", &self.index)
            .finish()
    }
}

impl PyRandom {
    /// Equivalent to `random.Random(seed)` for a non-negative integer seed.
    ///
    /// The seed is split into 32-bit little-endian words (one word below
    /// 2^32, two otherwise) and passed to `init_by_array`.
    pub fn from_seed(seed: u64) -> Self {
        let lo = seed as u32;
        let hi = (seed >> 32) as u32;
        if hi == 0 {
            Self::from_key(&[lo])
        } else {
            Self::from_key(&[lo, hi])
        }
    }

    /// Equivalent to `random.Random()` (seeded from OS entropy).
    ///
    /// Falls back to the system clock when OS entropy is unavailable.
    pub fn from_entropy() -> Self {
        let mut bytes = [0u8; N * 4];
        let key: Vec<u32> = if getrandom::getrandom(&mut bytes).is_ok() {
            bytes
                .chunks_exact(4)
                .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect()
        } else {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            vec![
                nanos as u32,
                (nanos >> 32) as u32,
                (nanos >> 64) as u32,
                std::process::id(),
            ]
        };
        Self::from_key(&key)
    }

    fn from_key(key: &[u32]) -> Self {
        let mut rng = PyRandom {
            state: [0; N],
            index: N,
        };
        rng.init_by_array(key);
        rng
    }

    fn init_genrand(&mut self, s: u32) {
        let mt = &mut self.state;
        mt[0] = s;
        for i in 1..N {
            mt[i] = 1_812_433_253u32
                .wrapping_mul(mt[i - 1] ^ (mt[i - 1] >> 30))
                .wrapping_add(i as u32);
        }
        self.index = N;
    }

    fn init_by_array(&mut self, key: &[u32]) {
        self.init_genrand(19_650_218);
        let mt = &mut self.state;
        let mut i = 1usize;
        let mut j = 0usize;
        for _ in 0..N.max(key.len()) {
            mt[i] = (mt[i] ^ (mt[i - 1] ^ (mt[i - 1] >> 30)).wrapping_mul(1_664_525))
                .wrapping_add(key[j])
                .wrapping_add(j as u32);
            i += 1;
            j += 1;
            if i >= N {
                mt[0] = mt[N - 1];
                i = 1;
            }
            if j >= key.len() {
                j = 0;
            }
        }
        for _ in 0..N - 1 {
            mt[i] = (mt[i] ^ (mt[i - 1] ^ (mt[i - 1] >> 30)).wrapping_mul(1_566_083_941))
                .wrapping_sub(i as u32);
            i += 1;
            if i >= N {
                mt[0] = mt[N - 1];
                i = 1;
            }
        }
        mt[0] = 0x8000_0000;
    }

    /// `genrand_uint32`: the next 32-bit output on `[0, 0xffffffff]`.
    pub fn genrand_u32(&mut self) -> u32 {
        const MAG01: [u32; 2] = [0, MATRIX_A];
        if self.index >= N {
            let mt = &mut self.state;
            for kk in 0..N - M {
                let y = (mt[kk] & UPPER_MASK) | (mt[kk + 1] & LOWER_MASK);
                mt[kk] = mt[kk + M] ^ (y >> 1) ^ MAG01[(y & 1) as usize];
            }
            for kk in N - M..N - 1 {
                let y = (mt[kk] & UPPER_MASK) | (mt[kk + 1] & LOWER_MASK);
                mt[kk] = mt[kk + M - N] ^ (y >> 1) ^ MAG01[(y & 1) as usize];
            }
            let y = (mt[N - 1] & UPPER_MASK) | (mt[0] & LOWER_MASK);
            mt[N - 1] = mt[M - 1] ^ (y >> 1) ^ MAG01[(y & 1) as usize];
            self.index = 0;
        }
        let mut y = self.state[self.index];
        self.index += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^= y >> 18;
        y
    }

    /// `random()`: a float on `[0, 1)` with 53-bit resolution
    /// (`genrand_res53`).
    pub fn random(&mut self) -> f64 {
        let a = self.genrand_u32() >> 5;
        let b = self.genrand_u32() >> 6;
        (a as f64 * 67_108_864.0 + b as f64) * (1.0 / 9_007_199_254_740_992.0)
    }

    /// `getrandbits(k)` for `k <= 64`. Words are drawn from the least
    /// significant end.
    pub fn getrandbits(&mut self, k: u32) -> u64 {
        assert!(k <= 64, "getrandbits supports k <= 64, got {k}");
        if k == 0 {
            return 0;
        }
        if k <= 32 {
            return (self.genrand_u32() >> (32 - k)) as u64;
        }
        let lo = self.genrand_u32() as u64;
        let hi = (self.genrand_u32() >> (64 - k)) as u64;
        lo | (hi << 32)
    }

    /// `_randbelow_with_getrandbits(n)`: an integer on `[0, n)`, `n > 0`.
    pub fn randbelow(&mut self, n: u64) -> u64 {
        assert!(n > 0, "randbelow requires n > 0");
        let k = 64 - n.leading_zeros();
        let mut r = self.getrandbits(k);
        while r >= n {
            r = self.getrandbits(k);
        }
        r
    }

    /// `randrange(start, stop)` with unit step, `start < stop`.
    pub fn randrange(&mut self, start: u64, stop: u64) -> u64 {
        assert!(start < stop, "empty range for randrange({start}, {stop})");
        start + self.randbelow(stop - start)
    }

    /// `expovariate(lambd)`: `-log(1.0 - random()) / lambd`.
    ///
    /// Callers pass `1.0 / mean`; the division form is kept so the last
    /// bit matches Python.
    pub fn expovariate(&mut self, lambd: f64) -> f64 {
        -(1.0 - self.random()).ln() / lambd
    }

    /// `shuffle(x)`: in-place Fisher-Yates from the back, drawing
    /// `randbelow(i + 1)`.
    pub fn shuffle<T>(&mut self, x: &mut [T]) {
        for i in (1..x.len()).rev() {
            let j = self.randbelow(i as u64 + 1) as usize;
            x.swap(i, j);
        }
    }
}

#[cfg(test)]
// Expected floats are CPython's `"%.17g"` output, kept digit for digit.
#[allow(clippy::excessive_precision)]
mod tests {
    use super::*;

    // Expected values come from CPython 3.11 (`random.Random(seed)`).

    fn assert_bits(got: &[f64], want: &[f64]) {
        assert_eq!(got.len(), want.len());
        for (i, (g, w)) in got.iter().zip(want).enumerate() {
            assert_eq!(g.to_bits(), w.to_bits(), "index {i}: got {g:e}, want {w:e}");
        }
    }

    #[test]
    fn state_after_seed_matches_cpython() {
        let rng = PyRandom::from_seed(42);
        assert_eq!(
            rng.state[..4],
            [2147483648, 3564348608, 1266698288, 4212342371]
        );
        assert_eq!(rng.index, 624);
    }

    #[test]
    fn random_seed_42() {
        let mut rng = PyRandom::from_seed(42);
        let got: Vec<f64> = (0..10).map(|_| rng.random()).collect();
        assert_bits(
            &got,
            &[
                0.63942679845788375,
                0.025010755222666936,
                0.27502931836911926,
                0.22321073814882275,
                0.7364712141640124,
                0.67669948742291131,
                0.89217956770484541,
                0.086938832629416152,
                0.42192181968527043,
                0.029797219438070344,
            ],
        );
    }

    #[test]
    fn getrandbits_32_seed_42() {
        let mut rng = PyRandom::from_seed(42);
        let got: Vec<u64> = (0..10).map(|_| rng.getrandbits(32)).collect();
        assert_eq!(
            got,
            [
                2746317213, 478163327, 107420369, 3184935163, 1181241943, 1051802512, 958682846,
                599310825, 3163119785, 440213415
            ]
        );
    }

    #[test]
    fn randbelow_100_seed_42() {
        let mut rng = PyRandom::from_seed(42);
        let got: Vec<u64> = (0..10).map(|_| rng.randbelow(100)).collect();
        assert_eq!(got, [81, 14, 3, 94, 35, 31, 28, 17, 94, 13]);
    }

    #[test]
    fn expovariate_1_seed_42() {
        let mut rng = PyRandom::from_seed(42);
        let got: Vec<f64> = (0..10).map(|_| rng.expovariate(1.0)).collect();
        assert_bits(
            &got,
            &[
                1.020060287274801,
                0.025328839042738891,
                0.32162406407496558,
                0.25258618567011354,
                1.3335926728080829,
                1.1291730087985927,
                2.2272880995513806,
                0.090952404606566145,
                0.54804615939649093,
                0.030250177207358588,
            ],
        );
    }

    #[test]
    fn shuffle_range_10_seed_42() {
        let mut rng = PyRandom::from_seed(42);
        let mut x: Vec<u32> = (0..10).collect();
        rng.shuffle(&mut x);
        assert_eq!(x, [7, 3, 2, 8, 5, 6, 9, 4, 0, 1]);
    }

    #[test]
    fn fixture_sub_seeds() {
        // VR (seed 42) and VT (seed 3) draw randrange(0, 2**31 - 1).
        assert_eq!(
            PyRandom::from_seed(42).randrange(0, 2_147_483_647),
            1373158606
        );
        assert_eq!(
            PyRandom::from_seed(3).randrange(0, 2_147_483_647),
            511025150
        );
        // VI (seed 7) draws getrandbits(64): low word first.
        let mut rng = PyRandom::from_seed(7);
        let sub = rng.getrandbits(64);
        assert_eq!(sub, 17485029721327973432);
        let mut halves = PyRandom::from_seed(7);
        assert_eq!(halves.getrandbits(32), 1390851128);
        assert_eq!(halves.getrandbits(32), 4071050724);
    }

    #[test]
    fn two_word_seed_random() {
        let mut rng = PyRandom::from_seed(17485029721327973432);
        let got: Vec<f64> = (0..3).map(|_| rng.random()).collect();
        assert_bits(
            &got,
            &[0.5208967008703396, 0.9421645052912904, 0.8306268041411967],
        );
    }

    #[test]
    fn getrandbits_wide_and_randrange_offsets() {
        let mut rng = PyRandom::from_seed(7);
        let got: Vec<u64> = [33, 48, 63, 64]
            .iter()
            .map(|&k| rng.getrandbits(k))
            .collect();
        assert_eq!(
            got,
            [
                5685818424,
                111132926676279,
                445363681616962640,
                15149836622520594227
            ]
        );
        let mut rng = PyRandom::from_seed(0);
        assert_eq!(rng.randrange(0, (1u64 << 63) + 5), 7106521602475165645);
        assert_eq!(rng.randrange(10, 20), 16);
    }

    #[test]
    fn seed_zero_uses_one_word_key() {
        let rng = PyRandom::from_seed(0);
        assert_eq!(rng.state[..2], [2147483648, 766982754]);
    }

    #[test]
    fn two_word_seed_differs_from_its_low_word() {
        let big = 17485029721327973432u64;
        let mut a = PyRandom::from_seed(big);
        let mut b = PyRandom::from_seed(big & 0xffff_ffff);
        let a10: Vec<u32> = (0..10).map(|_| a.genrand_u32()).collect();
        let b10: Vec<u32> = (0..10).map(|_| b.genrand_u32()).collect();
        assert_ne!(a10, b10);
    }

    #[test]
    fn fixture_rr_seed_99() {
        let mut rng = PyRandom::from_seed(99);
        let got: Vec<f64> = (0..5).map(|_| rng.random()).collect();
        assert_bits(
            &got,
            &[
                0.40397807494366633,
                0.20007544457494542,
                0.17880232058661227,
                0.24843131850096878,
                0.75987743650807793,
            ],
        );
    }

    #[test]
    fn fixture_ri_seed_5() {
        let mut rng = PyRandom::from_seed(5);
        let got: Vec<f64> = (0..3).map(|_| rng.expovariate(1.0 / 5.0)).collect();
        assert_bits(
            &got,
            &[4.8762468463219681, 6.7698520583964834, 7.9284498408703836],
        );
    }

    #[test]
    fn fixture_rt_seed_11() {
        let mut rng = PyRandom::from_seed(11);
        let got: Vec<f64> = (0..3).map(|_| rng.expovariate(1.0 / 3.0)).collect();
        assert_bits(
            &got,
            &[1.8065185436914499, 2.4613901443892892, 7.7393898810090835],
        );
    }

    #[test]
    fn clone_replays_the_same_draws() {
        let mut a = PyRandom::from_seed(5);
        a.random();
        let mut b = a.clone();
        assert_eq!(a.random().to_bits(), b.random().to_bits());
    }

    #[test]
    fn entropy_seeds_differ() {
        let mut a = PyRandom::from_entropy();
        let mut b = PyRandom::from_entropy();
        assert_ne!(a.getrandbits(64), b.getrandbits(64));
    }
}

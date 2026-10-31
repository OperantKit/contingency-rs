//! Float functions that `core` does not provide.
//!
//! With `std`, the inherent `f64` methods are used and this module adds
//! nothing. Without `std`, [`FloatExt`] supplies the same method names
//! backed by `libm`, so call sites stay `x.ln()`, `x.round()`, ….

#[cfg(not(feature = "std"))]
pub(crate) trait FloatExt {
    fn ln(self) -> f64;
    fn round(self) -> f64;
    fn floor(self) -> f64;
    fn powi(self, n: i32) -> f64;
    fn round_ties_even(self) -> f64;
}

#[cfg(not(feature = "std"))]
impl FloatExt for f64 {
    #[inline]
    fn ln(self) -> f64 {
        libm::log(self)
    }
    #[inline]
    fn round(self) -> f64 {
        libm::round(self)
    }
    #[inline]
    fn floor(self) -> f64 {
        libm::floor(self)
    }
    #[inline]
    fn round_ties_even(self) -> f64 {
        libm::roundeven(self)
    }
    /// Same square-and-multiply loop as `compiler_builtins`' `__powidf2`,
    /// which is what `f64::powi` lowers to with `std`, so both builds
    /// round the same way (`libm::pow` would not).
    fn powi(self, n: i32) -> f64 {
        let recip = n < 0;
        let mut b = n;
        let mut a = self;
        let mut r = 1.0;
        loop {
            if b & 1 != 0 {
                r *= a;
            }
            b /= 2;
            if b == 0 {
                break;
            }
            a *= a;
        }
        if recip {
            1.0 / r
        } else {
            r
        }
    }
}

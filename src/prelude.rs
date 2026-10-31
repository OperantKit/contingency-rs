//! Crate-internal prelude for the schedule core.
//!
//! Brings the `alloc` types that `std`'s prelude would otherwise supply
//! (`Box`, `String`, `Vec`, `format!`, `vec!`, …) so the same source
//! builds with and without `std`. Also defines the [`IndexMap`] alias
//! used by compound schedules.

#[allow(unused_imports)]
pub(crate) use alloc::{
    borrow::ToOwned,
    boxed::Box,
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};

#[cfg(not(feature = "std"))]
#[allow(unused_imports)]
pub(crate) use crate::math::FloatExt;

/// Insertion-ordered map used for compound schedule components.
///
/// With `std` this is exactly `indexmap::IndexMap<K, V>`. Without `std`
/// there is no `RandomState`, so a fixed FNV-1a hasher is used instead.
/// The hasher affects lookup cost only — iteration order is insertion
/// order either way.
#[cfg(feature = "std")]
pub type IndexMap<K, V> = indexmap::IndexMap<K, V>;

/// Insertion-ordered map used for compound schedule components (see the
/// `std` variant).
#[cfg(not(feature = "std"))]
pub type IndexMap<K, V> = indexmap::IndexMap<K, V, core::hash::BuildHasherDefault<Fnv1a>>;

/// 64-bit FNV-1a, the `no_std` hasher behind [`IndexMap`].
#[cfg(not(feature = "std"))]
#[derive(Clone, Copy)]
pub struct Fnv1a(u64);

#[cfg(not(feature = "std"))]
impl Default for Fnv1a {
    fn default() -> Self {
        Fnv1a(0xcbf2_9ce4_8422_2325)
    }
}

#[cfg(not(feature = "std"))]
impl core::hash::Hasher for Fnv1a {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= u64::from(b);
            self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
        }
    }
}

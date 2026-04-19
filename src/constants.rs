//! Module-wide constants.

/// Floating-point tolerance used uniformly across all monotonic-time
/// and event-time equality checks. Mirrors `TIME_TOL` in
/// `contingency-py`. A Rust port that uses a different tolerance will
/// diverge from the Python implementation on boundary-equal
/// conformance fixtures.
pub const TIME_TOL: f64 = 1e-9;

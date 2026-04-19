//! Smoke test for the PyO3 extension module.
//!
//! Running ``cargo test --features python`` compiles the whole
//! ``#[cfg(feature = "python")]`` module tree. This test is intentionally
//! trivial — the real verification is that the build succeeded at all.

#![cfg(feature = "python")]

#[test]
fn python_module_compiles() {
    // The cfg-gated smoke test only asserts compilation, so this is
    // deliberately empty of behaviour.
}

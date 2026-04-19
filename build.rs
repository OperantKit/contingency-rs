//! Build script for the `contingency` crate.
//!
//! Its sole responsibility is to emit the macOS-specific linker flag
//! required when the `python` feature is enabled. PyO3's
//! `extension-module` feature expects Python to resolve its own symbols
//! dynamically when the `.dylib` is loaded; `cargo build` does not know
//! that by default, and on macOS the linker (`ld`) rejects undefined
//! symbols unless we explicitly pass `-undefined dynamic_lookup`.
//!
//! On Linux the default shared-library link behaviour already tolerates
//! undefined symbols, so no flag is needed. Windows is not covered here
//! because `abi3` wheels on Windows pin to `python3.dll` at link time.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    // Only emit the flag when both the python feature is active and the
    // host is macOS. Using the `CARGO_FEATURE_PYTHON` env var avoids
    // `#[cfg(feature = "python")]` tracking on the build-script itself.
    let python_feature = std::env::var_os("CARGO_FEATURE_PYTHON").is_some();
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();

    if python_feature && target_os == "macos" {
        println!("cargo:rustc-link-arg-cdylib=-undefined");
        println!("cargo:rustc-link-arg-cdylib=dynamic_lookup");
    }
}

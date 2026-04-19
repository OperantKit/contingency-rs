//! Build script for the `contingency` crate.
//!
//! Responsibilities:
//!
//! 1. Emit the macOS-specific linker flag required when the `python`
//!    feature is enabled. PyO3's `extension-module` feature expects
//!    Python to resolve its own symbols dynamically when the `.dylib`
//!    is loaded; `cargo build` does not know that by default, and on
//!    macOS the linker (`ld`) rejects undefined symbols unless we
//!    explicitly pass `-undefined dynamic_lookup`. On Linux the default
//!    shared-library link behaviour already tolerates undefined
//!    symbols, so no flag is needed. Windows is not covered here
//!    because `abi3` wheels on Windows pin to `python3.dll` at link
//!    time.
//!
//! 2. Generate the C header `include/contingency.h` via cbindgen. The
//!    C ABI surface lives in `src/ffi.rs` and is always built on
//!    non-wasm targets; the header is always refreshed so downstream
//!    C consumers see the current surface.

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

    // Generate C header via cbindgen. Skip on wasm targets (no C FFI
    // there) and tolerate any cbindgen failure gracefully — a header
    // regeneration error should not block the build itself, since the
    // Rust `src/ffi.rs` module is the ultimate source of truth.
    let target_arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    if target_arch != "wasm32" {
        generate_c_header();
    }

    println!("cargo:rerun-if-changed=cbindgen.toml");
    println!("cargo:rerun-if-changed=src/ffi.rs");
}

fn generate_c_header() {
    let crate_dir = match std::env::var("CARGO_MANIFEST_DIR") {
        Ok(v) => v,
        Err(_) => return,
    };
    let crate_path = std::path::Path::new(&crate_dir);
    let include_dir = crate_path.join("include");
    if std::fs::create_dir_all(&include_dir).is_err() {
        return;
    }
    let out_path = include_dir.join("contingency.h");

    let config_path = crate_path.join("cbindgen.toml");
    let config = cbindgen::Config::from_file(&config_path).unwrap_or_default();

    match cbindgen::Builder::new()
        .with_crate(&crate_dir)
        .with_config(config)
        .generate()
    {
        Ok(bindings) => {
            let _ = bindings.write_to_file(&out_path);
        }
        Err(e) => {
            // Emit a build-time warning but do not fail the build — the
            // header is a convenience artefact; Rust-side FFI tests still
            // validate the ABI directly.
            println!("cargo:warning=cbindgen failed to generate header: {e}");
        }
    }
}

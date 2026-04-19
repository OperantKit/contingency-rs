//! Local `uniffi-bindgen` entry point.
//!
//! UniFFI 0.28 does not ship a standalone CLI on crates.io; each
//! consumer crate embeds `uniffi::uniffi_bindgen_main()` in a tiny
//! binary target. This bin is built under `--features uniffi` and
//! invoked by downstream binding-generation tooling to emit Kotlin /
//! Swift / KMP sources from the `#[uniffi::export]` surface defined
//! in `src/uniffi_api.rs`.

#![cfg(feature = "uniffi")]

fn main() {
    uniffi::uniffi_bindgen_main()
}

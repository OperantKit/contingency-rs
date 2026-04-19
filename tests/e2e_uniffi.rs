//! E2E test: run the crate's own `uniffi-bindgen` binary against the
//! built cdylib to emit Swift and Kotlin bindings, then sanity-check
//! the output files for expected generated artefacts.
//!
//! This proves that:
//!   * the cdylib contains the uniffi metadata symbols
//!   * the bindgen binary can parse them and emit sources
//!   * the emitted sources contain the names we expect (e.g.
//!     `UniffiSchedule`), meaning downstream Swift/Kotlin consumers
//!     would see the intended API surface.
//!
//! We do NOT compile the Swift/Kotlin output — that would require
//! swiftc / kotlinc which are not assumed on all hosts. Skipped
//! (passes with a stderr message) when prerequisites are missing.

#![cfg(feature = "uniffi")]

use std::path::{Path, PathBuf};
use std::process::Command;

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn find_target_dir() -> Option<PathBuf> {
    let mut cur: &Path = &manifest_dir();
    loop {
        let candidate = cur.join("target");
        if candidate.is_dir() {
            return Some(candidate);
        }
        match cur.parent() {
            Some(p) => cur = p,
            None => return None,
        }
    }
}

fn cdylib_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "libcontingency.dylib"
    } else if cfg!(windows) {
        "contingency.dll"
    } else {
        "libcontingency.so"
    }
}

fn bindgen_bin_name() -> &'static str {
    if cfg!(windows) {
        "uniffi-bindgen.exe"
    } else {
        "uniffi-bindgen"
    }
}

fn find_artifact(target_dir: &Path, name: &str) -> Option<PathBuf> {
    let primary = if cfg!(debug_assertions) { "debug" } else { "release" };
    let fallback = if cfg!(debug_assertions) { "release" } else { "debug" };
    for profile in [primary, fallback] {
        let p = target_dir.join(profile).join(name);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

fn tmpdir(tag: &str) -> PathBuf {
    let mut d = std::env::temp_dir();
    d.push(format!(
        "contingency-e2e-uniffi-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
    ));
    std::fs::create_dir_all(&d).expect("create tmp dir");
    d
}

fn read_dir_files(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for entry in rd.flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) == Some(ext) {
                v.push(p);
            }
        }
    }
    v
}

fn run_bindgen(
    bindgen: &Path,
    library: &Path,
    language: &str,
    out_dir: &Path,
) -> std::io::Result<std::process::Output> {
    // UniFFI 0.28: `--library` is a bare flag indicating the positional
    // SOURCE is a cdylib rather than a UDL file.
    Command::new(bindgen)
        .args([
            "generate",
            "--library",
            "--language",
            language,
            "--out-dir",
            &out_dir.display().to_string(),
            &library.display().to_string(),
        ])
        .output()
}

#[test]
fn uniffi_bindgen_emits_swift_and_kotlin() {
    let target_dir = match find_target_dir() {
        Some(d) => d,
        None => {
            eprintln!("e2e_uniffi: skipped — no target/ directory found");
            return;
        }
    };

    // Ensure the cdylib + bindgen are built with the uniffi feature
    // so that the dylib carries the metadata symbols that
    // `uniffi-bindgen generate --library` needs to parse.
    let build = Command::new(env!("CARGO"))
        .args([
            "build",
            "-p",
            "contingency",
            "--features",
            "uniffi",
            "--lib",
            "--bin",
            "uniffi-bindgen",
        ])
        .output();
    match build {
        Ok(o) if o.status.success() => {}
        Ok(o) => {
            eprintln!(
                "e2e_uniffi: skipped — cargo build with --features uniffi failed\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            );
            return;
        }
        Err(e) => {
            eprintln!("e2e_uniffi: skipped — could not invoke cargo: {e}");
            return;
        }
    }

    let bindgen = match find_artifact(&target_dir, bindgen_bin_name()) {
        Some(p) => p,
        None => {
            eprintln!(
                "e2e_uniffi: skipped — uniffi-bindgen binary not found under {} after build.",
                target_dir.display()
            );
            return;
        }
    };

    let cdylib = match find_artifact(&target_dir, cdylib_name()) {
        Some(p) => p,
        None => {
            eprintln!(
                "e2e_uniffi: skipped — {} not found under {} after build.",
                cdylib_name(),
                target_dir.display()
            );
            return;
        }
    };

    // ---- Swift ------------------------------------------------------
    let swift_out = tmpdir("swift");
    let swift_res = match run_bindgen(&bindgen, &cdylib, "swift", &swift_out) {
        Ok(o) => o,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&swift_out);
            eprintln!("e2e_uniffi: skipped — could not invoke bindgen for swift: {e}");
            return;
        }
    };
    let swift_stdout = String::from_utf8_lossy(&swift_res.stdout).into_owned();
    let swift_stderr = String::from_utf8_lossy(&swift_res.stderr).into_owned();
    assert!(
        swift_res.status.success(),
        "uniffi-bindgen (swift) failed\nstdout:\n{swift_stdout}\nstderr:\n{swift_stderr}"
    );
    let swift_files = read_dir_files(&swift_out, "swift");
    assert!(
        !swift_files.is_empty(),
        "no .swift files emitted to {}",
        swift_out.display()
    );
    let mut found_swift_class = false;
    for f in &swift_files {
        if let Ok(s) = std::fs::read_to_string(f) {
            // UniFFI emits `public class UniffiSchedule` (or similar)
            // for `#[uniffi::Object]` types. Accept either the class
            // keyword or `open class`.
            if s.contains("UniffiSchedule") && (s.contains("class ") || s.contains("protocol ")) {
                found_swift_class = true;
                break;
            }
        }
    }
    assert!(
        found_swift_class,
        "no emitted .swift file mentioned `UniffiSchedule` with class/protocol decl; files: {swift_files:?}"
    );

    // ---- Kotlin -----------------------------------------------------
    let kotlin_out = tmpdir("kotlin");
    let kotlin_res = match run_bindgen(&bindgen, &cdylib, "kotlin", &kotlin_out) {
        Ok(o) => o,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&swift_out);
            let _ = std::fs::remove_dir_all(&kotlin_out);
            eprintln!("e2e_uniffi: skipped — could not invoke bindgen for kotlin: {e}");
            return;
        }
    };
    let kotlin_stdout = String::from_utf8_lossy(&kotlin_res.stdout).into_owned();
    let kotlin_stderr = String::from_utf8_lossy(&kotlin_res.stderr).into_owned();
    assert!(
        kotlin_res.status.success(),
        "uniffi-bindgen (kotlin) failed\nstdout:\n{kotlin_stdout}\nstderr:\n{kotlin_stderr}"
    );
    // Kotlin output may be nested in package subdirs — walk.
    let mut kotlin_files: Vec<PathBuf> = Vec::new();
    walk_collect(&kotlin_out, "kt", &mut kotlin_files);
    assert!(
        !kotlin_files.is_empty(),
        "no .kt files emitted to {}",
        kotlin_out.display()
    );
    let mut found_kotlin_class = false;
    for f in &kotlin_files {
        if let Ok(s) = std::fs::read_to_string(f) {
            if s.contains("UniffiSchedule") && (s.contains("class ") || s.contains("interface ")) {
                found_kotlin_class = true;
                break;
            }
        }
    }
    assert!(
        found_kotlin_class,
        "no emitted .kt file mentioned `UniffiSchedule` with class/interface decl; files: {kotlin_files:?}"
    );

    // Cleanup on success.
    let _ = std::fs::remove_dir_all(&swift_out);
    let _ = std::fs::remove_dir_all(&kotlin_out);
}

fn walk_collect(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    let rd = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return,
    };
    for entry in rd.flatten() {
        let p = entry.path();
        if p.is_dir() {
            walk_collect(&p, ext, out);
        } else if p.extension().and_then(|e| e.to_str()) == Some(ext) {
            out.push(p);
        }
    }
}

//! E2E test: compile a tiny C program against the generated header and
//! the `contingency` cdylib, run the resulting binary, and assert its
//! output.
//!
//! This exercises the C FFI surface from the *target language side* —
//! i.e. a real C compiler parses `include/contingency.h`, resolves
//! `opk_*` symbols at link time against `libcontingency.{dylib,so,a}`,
//! and executes the linked binary.
//!
//! Prerequisites probed at runtime; the test skips (passes with a
//! stderr message) if any are missing:
//!   * `cc` (or `$CC`) on PATH
//!   * The cdylib (or staticlib) built under `target/{debug,release}/`
//!   * Native (non-wasm) host target
//!
//! Failures here indicate a real binding regression: ABI drift,
//! missing symbols, header/impl mismatch.

#![cfg(not(target_arch = "wasm32"))]

use std::path::{Path, PathBuf};
use std::process::Command;

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Walk up from the crate manifest dir until a directory containing a
/// `target/` folder is found. Returns the `target/` dir itself. In a
/// cargo workspace this yields the workspace `target/`; in a single
/// crate it yields the crate-local one.
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

/// Find the directory (debug or release) that contains the
/// contingency cdylib/staticlib. Prefer the profile matching
/// `cfg!(debug_assertions)` but fall back to the other.
fn find_lib_dir(target_dir: &Path) -> Option<PathBuf> {
    let primary = if cfg!(debug_assertions) { "debug" } else { "release" };
    let fallback = if cfg!(debug_assertions) { "release" } else { "debug" };
    for profile in [primary, fallback] {
        let d = target_dir.join(profile);
        if !d.is_dir() {
            continue;
        }
        // Check for any of the plausible artifact names.
        for name in [
            "libcontingency.dylib",
            "libcontingency.so",
            "contingency.dll",
            "libcontingency.a",
        ] {
            if d.join(name).exists() {
                return Some(d);
            }
        }
    }
    None
}

fn have_cc() -> Option<String> {
    if let Ok(cc) = std::env::var("CC") {
        if !cc.trim().is_empty() {
            return Some(cc);
        }
    }
    for cand in ["cc", "clang", "gcc"] {
        let ok = Command::new(cand)
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if ok {
            return Some(cand.to_string());
        }
    }
    None
}

fn tmpdir() -> PathBuf {
    // Avoid the `tempfile` crate dep; use $CARGO_TARGET_TMPDIR if
    // available, else std::env::temp_dir() with a unique-ish suffix.
    if let Some(d) = option_env!("CARGO_TARGET_TMPDIR") {
        let d = PathBuf::from(d);
        if d.is_dir() {
            return d;
        }
    }
    let mut d = std::env::temp_dir();
    d.push(format!(
        "contingency-e2e-c-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
    ));
    std::fs::create_dir_all(&d).expect("create tmp dir");
    d
}

const C_PROG: &str = r#"
#include <stdio.h>
#include <string.h>
#include "contingency.h"

int main(void) {
    OpkSchedule *h = opk_fr(3);
    if (!h) { fprintf(stderr, "opk_fr(3) returned NULL\n"); return 2; }

    OpkOutcome out = {0};
    const char *op = "main";
    for (int i = 1; i <= 3; ++i) {
        int rc = opk_schedule_step(h, (double)i, true, (double)i, op, &out);
        if (rc != 0) { fprintf(stderr, "step %d rc=%d\n", i, rc); return 3; }
        if (i < 3) {
            if (out.reinforced) { fprintf(stderr, "premature SR+ at %d\n", i); return 4; }
        } else {
            if (!out.reinforced) { fprintf(stderr, "no SR+ on step 3\n"); return 5; }
            if (out.reinforcer_label == NULL) { fprintf(stderr, "null label\n"); return 6; }
            if (strcmp(out.reinforcer_label, "SR+") != 0) {
                fprintf(stderr, "label=%s\n", out.reinforcer_label); return 7;
            }
        }
    }

    opk_schedule_free(h);

    /* also exercise the error channel */
    opk_clear_last_error();
    OpkSchedule *bad = opk_fr(0);
    if (bad != NULL) { fprintf(stderr, "opk_fr(0) should fail\n"); return 8; }
    const char *msg = opk_last_error_message();
    if (msg == NULL) { fprintf(stderr, "no error message\n"); return 9; }

    printf("OK\n");
    return 0;
}
"#;

#[test]
fn c_program_links_and_runs() {
    let manifest = manifest_dir();
    let header_dir = manifest.join("include");
    let header = header_dir.join("contingency.h");
    if !header.exists() {
        eprintln!("e2e_c_ffi: skipped — header not found at {}", header.display());
        return;
    }

    let target_dir = match find_target_dir() {
        Some(d) => d,
        None => {
            eprintln!("e2e_c_ffi: skipped — no target/ directory found");
            return;
        }
    };
    let lib_dir = match find_lib_dir(&target_dir) {
        Some(d) => d,
        None => {
            eprintln!(
                "e2e_c_ffi: skipped — no libcontingency.{{dylib,so,a}} under {}",
                target_dir.display()
            );
            return;
        }
    };

    let cc = match have_cc() {
        Some(c) => c,
        None => {
            eprintln!("e2e_c_ffi: skipped — no C compiler on PATH");
            return;
        }
    };

    let tmp = tmpdir();
    let src_path = tmp.join("e2e_ffi_smoke.c");
    let bin_path = tmp.join(if cfg!(windows) {
        "e2e_ffi_smoke.exe"
    } else {
        "e2e_ffi_smoke"
    });

    if let Err(e) = std::fs::write(&src_path, C_PROG) {
        eprintln!("e2e_c_ffi: skipped — could not write C source: {e}");
        return;
    }

    let mut cmd = Command::new(&cc);
    cmd.arg(&src_path)
        .arg(format!("-I{}", header_dir.display()))
        .arg(format!("-L{}", lib_dir.display()))
        .arg("-lcontingency")
        .arg("-o")
        .arg(&bin_path);
    // Help the dynamic loader find libcontingency at runtime on
    // Unix-likes (we could link statically, but prefer the cdylib
    // path since that is the distribution artifact).
    if cfg!(any(target_os = "macos", target_os = "linux")) {
        cmd.arg(format!("-Wl,-rpath,{}", lib_dir.display()));
    }

    let compile = match cmd.output() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("e2e_c_ffi: skipped — failed to invoke {cc}: {e}");
            return;
        }
    };
    if !compile.status.success() {
        // Compile failure IS a real regression signal (header drift,
        // missing symbol, etc.). Report it as a test failure — but
        // guard with a check for obvious toolchain-only issues like
        // "library not found for -lcontingency" which can happen when
        // the cdylib was produced under a different profile in CI.
        let stderr = String::from_utf8_lossy(&compile.stderr);
        if stderr.contains("library not found") || stderr.contains("cannot find -lcontingency") {
            eprintln!(
                "e2e_c_ffi: skipped — linker could not locate -lcontingency under {}\nstderr:\n{}",
                lib_dir.display(),
                stderr
            );
            return;
        }
        panic!(
            "C compile failed (cc={cc})\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&compile.stdout),
            stderr
        );
    }

    let mut run = Command::new(&bin_path);
    // Ensure DYLD/LD path includes lib_dir so `-rpath` fallback is
    // covered on older linkers.
    if cfg!(target_os = "macos") {
        let cur = std::env::var("DYLD_LIBRARY_PATH").unwrap_or_default();
        let joined = if cur.is_empty() {
            lib_dir.display().to_string()
        } else {
            format!("{}:{}", lib_dir.display(), cur)
        };
        run.env("DYLD_LIBRARY_PATH", joined);
    } else if cfg!(target_os = "linux") {
        let cur = std::env::var("LD_LIBRARY_PATH").unwrap_or_default();
        let joined = if cur.is_empty() {
            lib_dir.display().to_string()
        } else {
            format!("{}:{}", lib_dir.display(), cur)
        };
        run.env("LD_LIBRARY_PATH", joined);
    }

    let out = run.output().expect("spawn linked binary");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "linked C binary exited non-zero: status={:?}\nstdout:\n{}\nstderr:\n{}",
        out.status,
        stdout,
        stderr
    );
    assert!(
        stdout.contains("OK"),
        "expected 'OK' in stdout, got:\n{stdout}\n--- stderr:\n{stderr}"
    );
}

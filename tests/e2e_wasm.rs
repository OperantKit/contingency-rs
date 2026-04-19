//! E2E test: build the WASM bindings with `wasm-pack build --target
//! nodejs`, write a tiny Node.js driver that imports the generated JS
//! shim and exercises `Schedule.fr(3)`, then run it with `node`.
//!
//! This flavour avoids the dev-dep pull-in that `wasm-pack test` does
//! (proptest + its getrandom versions don't build for wasm32 without
//! extra cfg flags). The build-and-run pipeline still proves:
//!   1. The crate compiles to wasm32-unknown-unknown.
//!   2. wasm-bindgen generates a working JS shim for every exposed
//!      class / classmethod.
//!   3. Node.js can load the shim and call through it end-to-end.
//!
//! Soft-skipped (passes with a stderr message) when any of the
//! following is missing: `wasm-pack`, `node`, or the
//! `wasm32-unknown-unknown` rustc target.

#![cfg(not(target_arch = "wasm32"))]

use std::path::PathBuf;
use std::process::Command;

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn on_path(tool: &str) -> bool {
    Command::new(tool)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn has_wasm_target() -> bool {
    let out = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .lines()
            .any(|l| l.trim() == "wasm32-unknown-unknown"),
        _ => true,
    }
}

#[test]
fn wasm_pack_build_and_node_smoke() {
    if !on_path("wasm-pack") {
        eprintln!("e2e_wasm: skipped — wasm-pack not on PATH");
        return;
    }
    if !on_path("node") {
        eprintln!("e2e_wasm: skipped — node not on PATH");
        return;
    }
    if !has_wasm_target() {
        eprintln!("e2e_wasm: skipped — rustup target wasm32-unknown-unknown not installed");
        return;
    }

    let crate_dir = manifest_dir();
    let pkg_dir = crate_dir.join("pkg-e2e-smoke");
    // Clean any stale output.
    let _ = std::fs::remove_dir_all(&pkg_dir);

    // 1. wasm-pack build --target nodejs --dev
    let build = Command::new("wasm-pack")
        .args([
            "build",
            "--target",
            "nodejs",
            "--dev",
            "--out-dir",
            pkg_dir.file_name().unwrap().to_str().unwrap(),
        ])
        .current_dir(&crate_dir)
        .output()
        .expect("failed to invoke wasm-pack build");

    if !build.status.success() {
        let stderr = String::from_utf8_lossy(&build.stderr);
        // Soft-skip known environment issues.
        let skip_markers = [
            "failed to download",
            "Permission denied",
            "network",
            "command not found",
        ];
        if skip_markers.iter().any(|m| stderr.contains(m)) {
            eprintln!("e2e_wasm: skipped — wasm-pack environment issue:\n{stderr}");
            let _ = std::fs::remove_dir_all(&pkg_dir);
            return;
        }
        panic!(
            "wasm-pack build failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{stderr}",
            build.status,
            String::from_utf8_lossy(&build.stdout),
        );
    }

    // 2. Verify expected output files.
    let js_shim = pkg_dir.join("contingency.js");
    let wasm_bin = pkg_dir.join("contingency_bg.wasm");
    assert!(
        js_shim.exists(),
        "expected JS shim at {}",
        js_shim.display()
    );
    assert!(
        wasm_bin.exists(),
        "expected wasm binary at {}",
        wasm_bin.display()
    );

    // 3. Write a small Node.js driver.
    let driver_path = pkg_dir.join("smoke.mjs");
    std::fs::write(
        &driver_path,
        r#"// Node.js E2E smoke for contingency WASM bindings.
// Require CommonJS style since wasm-pack --target nodejs emits CJS.
const { Schedule, ResponseEvent } = require('./contingency.js');

function assert(cond, msg) {
    if (!cond) {
        console.error('FAIL:', msg);
        process.exit(1);
    }
}

// FR(3): 1st/2nd responses non-reinforced, 3rd reinforced.
const fr = Schedule.fr(3n);
const r1 = fr.step(1.0, new ResponseEvent(1.0, 'main'));
assert(!r1.reinforced, 'FR3 step 1 should not reinforce');
const r2 = fr.step(2.0, new ResponseEvent(2.0, 'main'));
assert(!r2.reinforced, 'FR3 step 2 should not reinforce');
const r3 = fr.step(3.0, new ResponseEvent(3.0, 'main'));
assert(r3.reinforced, 'FR3 step 3 should reinforce');
assert(r3.reinforcer, 'FR3 step 3 should carry a reinforcer');
assert(r3.reinforcer.label === 'SR+', 'expected SR+ label');
assert(Math.abs(r3.reinforcer.time - 3.0) < 1e-9, 'expected time=3.0');

// CRF reinforces every response.
const crf = Schedule.crf();
const c1 = crf.step(1.0, new ResponseEvent(1.0, 'main'));
assert(c1.reinforced, 'CRF should reinforce every response');

// EXT never reinforces.
const ext = Schedule.ext();
const e1 = ext.step(1.0, new ResponseEvent(1.0, 'main'));
assert(!e1.reinforced, 'EXT should never reinforce');

console.log('OK');
"#,
    )
    .expect("write node driver");

    // Rewrite as CJS if wasm-pack emitted CJS; but .mjs forces ESM, and
    // the shim is CJS. Rename to .js to use CJS + require().
    let driver_js = pkg_dir.join("smoke.js");
    std::fs::rename(&driver_path, &driver_js).expect("rename driver to .js");

    // 4. Run it.
    let run = Command::new("node")
        .arg(&driver_js)
        .output()
        .expect("failed to invoke node");

    let stdout = String::from_utf8_lossy(&run.stdout);
    let stderr = String::from_utf8_lossy(&run.stderr);

    // Cleanup regardless of outcome.
    let _ = std::fs::remove_dir_all(&pkg_dir);

    assert!(
        run.status.success() && stdout.contains("OK"),
        "node driver failed\nstatus: {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        run.status,
    );
}

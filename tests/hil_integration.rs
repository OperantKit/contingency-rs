//! Integration test for the `contingency-hil` binary.
//!
//! Spawns the binary on an ephemeral local port, pushes JSONL `response`
//! messages across a TCP socket, and asserts on the `reinforcer` JSONL
//! messages the binary emits back.
//!
//! The binary builds its schedule with `contingency::spec::from_spec`, so
//! every spec `type` that `from_spec` accepts must start a session. One
//! test drives each of those types once; others check VI, VR, and
//! Concurrent reinforcement over the wire and the start-up error for an
//! unknown type.
//!
//! The child process is always reaped — either via the in-band
//! `disconnect` message (graceful) or via `Child::kill` (fallback).

use std::collections::HashSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use contingency::spec::SPEC_TYPES;

/// Grab an ephemeral local port by binding to 0 and recording the
/// assigned port number, then immediately dropping the listener.
fn pick_free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral");
    let addr: SocketAddr = listener.local_addr().expect("local_addr");
    addr.port()
}

/// Ensure the child is reaped even if the test panics mid-flight.
struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        // Best-effort: if the child already exited, `try_wait` returns
        // `Ok(Some(_))` and we leave it alone. Otherwise kill + wait.
        match self.0.try_wait() {
            Ok(Some(_)) => {}
            _ => {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
    }
}

impl ChildGuard {
    /// Stderr of a child that has already exited.
    fn stderr_text(&mut self) -> String {
        let mut text = String::new();
        if let Some(err) = self.0.stderr.as_mut() {
            let _ = err.read_to_string(&mut text);
        }
        text
    }
}

fn spawn_hil(port: u16, spec: &str) -> ChildGuard {
    let child = Command::new(env!("CARGO_BIN_EXE_contingency-hil"))
        .args(["--port", &port.to_string(), "--schedule", spec])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn contingency-hil");
    ChildGuard(child)
}

/// Retry connection until the HIL binary has its listener up. Panics
/// with the binary's stderr if it exits before listening.
fn connect_with_retry(guard: &mut ChildGuard, port: u16, timeout: Duration) -> TcpStream {
    let deadline = Instant::now() + timeout;
    let mut last_err: Option<std::io::Error> = None;
    while Instant::now() < deadline {
        if let Ok(Some(status)) = guard.0.try_wait() {
            panic!(
                "contingency-hil exited ({status}) before listening: {}",
                guard.stderr_text()
            );
        }
        match TcpStream::connect(("127.0.0.1", port)) {
            Ok(s) => return s,
            Err(e) => {
                last_err = Some(e);
                thread::sleep(Duration::from_millis(25));
            }
        }
    }
    panic!(
        "could not connect to contingency-hil on 127.0.0.1:{port} within {:?}: {:?}",
        timeout, last_err
    );
}

/// Poll a child process until it exits or the deadline elapses; return
/// `true` iff the child exited on its own.
fn wait_for_exit(child: &mut Child, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Ok(Some(_)) = child.try_wait() {
            return true;
        }
        thread::sleep(Duration::from_millis(25));
    }
    false
}

/// Run one session: send `(time, operandum)` responses, then
/// `disconnect`, read every line the binary sends until it closes the
/// socket, and check that it exits successfully. Returns the parsed
/// `reinforcer` messages in arrival order.
fn run_session(spec: &Value, responses: &[(f64, &str)]) -> Vec<Value> {
    let port = pick_free_port();
    let mut guard = spawn_hil(port, &spec.to_string());
    let stream = connect_with_retry(&mut guard, port, Duration::from_secs(5));
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("set_read_timeout");
    let reader = BufReader::new(stream.try_clone().expect("clone stream"));
    let mut writer = stream;

    for (time, operandum) in responses {
        let msg = json!({"type": "response", "time": time, "operandum": operandum});
        writeln!(writer, "{msg}").expect("write response");
    }
    writeln!(writer, r#"{{"type":"disconnect"}}"#).expect("write disconnect");
    writer.flush().expect("flush");

    let mut reinforcers = Vec::new();
    for line in reader.lines() {
        let line = line.unwrap_or_else(|e| panic!("reading from contingency-hil ({spec}): {e}"));
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = serde_json::from_str(&line).expect("outbound line must parse as JSON");
        assert_eq!(
            msg["type"], "reinforcer",
            "unexpected outbound message {msg}"
        );
        reinforcers.push(msg);
    }
    drop(writer);

    assert!(
        wait_for_exit(&mut guard.0, Duration::from_secs(5)),
        "contingency-hil did not exit after disconnect ({spec})"
    );
    let status = guard.0.wait().expect("wait");
    assert!(
        status.success(),
        "contingency-hil exited with {status} for {spec}: {}",
        guard.stderr_text()
    );
    reinforcers
}

fn times(reinforcers: &[Value]) -> Vec<f64> {
    reinforcers
        .iter()
        .map(|r| r["time"].as_f64().expect("reinforcer time"))
        .collect()
}

fn fr(n: u64) -> Value {
    json!({"type": "FR", "params": {"n": n}})
}

/// One spec per `SPEC_TYPES` entry (the same parameters as
/// `tests/spec.rs`).
fn sample_specs() -> Vec<Value> {
    let fi2 = json!({"type": "FI", "params": {"interval": 2.0}});
    let params = [
        ("FR", json!({"n": 3})),
        ("CRF", json!({})),
        ("EXT", json!({})),
        ("VR", json!({"mean": 3.0, "n_intervals": 6, "seed": 1})),
        ("RR", json!({"probability": 0.5, "seed": 1})),
        ("FI", json!({"interval": 2.0})),
        (
            "VI",
            json!({"mean_interval": 2.0, "n_intervals": 6, "seed": 2}),
        ),
        ("RI", json!({"mean_interval": 2.0, "seed": 3})),
        ("FT", json!({"interval": 3.0})),
        (
            "VT",
            json!({"mean_interval": 2.0, "n_intervals": 6, "seed": 4}),
        ),
        ("RT", json!({"mean_interval": 2.0, "seed": 5})),
        ("LimitedHold", json!({"inner": fi2, "hold": 1.0})),
        (
            "Timeout",
            json!({"inner": fr(2), "duration": 1.0, "reset_on_response": true}),
        ),
        (
            "ResponseCost",
            json!({"inner": fr(2), "amount": 1.0, "unit": "token", "initial_balance": 3.0}),
        ),
        ("DRO", json!({"interval": 2.0, "dro_type": "momentary"})),
        ("DRL", json!({"interval": 1.0})),
        ("DRH", json!({"response_count": 2, "time_window": 1.0})),
        (
            "Percentile",
            json!({"target": "irt", "rank": 50, "window": 3}),
        ),
        ("Lag", json!({"n": 1, "length": 2})),
        (
            "Concurrent",
            json!({"components": {"left": fr(1), "right": fr(1)}, "cod": 0.0}),
        ),
        (
            "Alternative",
            json!({"first": fr(3), "second": {"type": "FT", "params": {"interval": 10.0}}}),
        ),
        ("Conjunctive", json!({"first": fr(2), "second": fi2})),
        (
            "Multiple",
            json!({"components": [fr(2), fr(3)], "stimuli": ["red", "green"]}),
        ),
        ("Mixed", json!({"components": [fr(2), fr(3)]})),
        ("Chained", json!({"components": [fr(2), fr(3)]})),
        ("Tandem", json!({"components": [fr(2), fr(3)]})),
        (
            "Interpolate",
            json!({"base": fr(2), "probe": {"type": "EXT"}, "interval": 2.0, "probe_duration": 1.0}),
        ),
        ("Overlay", json!({"base": fr(2), "punisher": fr(3)})),
        ("SecondOrder", json!({"overall": fr(2), "unit": fr(2)})),
        (
            "ProgressiveRatio",
            json!({"step": "arithmetic", "start": 1, "step_size": 1}),
        ),
        (
            "InterlockingSchedule",
            json!({"initial_ratio": 4, "decay_time": 8.0}),
        ),
        (
            "AdjustingSchedule",
            json!({"target": "ratio", "start": 2.0, "step": 1.0, "maximum": 3.0}),
        ),
        (
            "Sidman",
            json!({"shock_shock_interval": 2.0, "response_shock_interval": 3.0}),
        ),
        (
            "DiscriminatedAvoidance",
            json!({"warning_duration": 2.0, "iti": 2.0}),
        ),
        ("Escape", json!({"trial_duration": 2.0, "iti": 1.0})),
        ("MatchingToSample", json!({"n_comparisons": 2, "seed": 7})),
        ("GoNoGo", json!({"seed": 1})),
    ];
    params
        .into_iter()
        .map(|(ty, p)| json!({"type": ty, "params": p}))
        .collect()
}

#[test]
fn hil_fr3_reinforces_on_third_response() {
    let reinforcers = run_session(&fr(3), &[(1.0, "main"), (2.0, "main"), (3.0, "main")]);
    assert_eq!(
        reinforcers.len(),
        1,
        "FR 3 over 3 responses: {reinforcers:?}"
    );
    let r = &reinforcers[0];
    assert_eq!(r["channel"], "food");
    assert!(
        (r["time"].as_f64().unwrap() - 3.0).abs() < 1e-9,
        "expected reinforcer.time == 3.0, got {}",
        r["time"]
    );
    assert_eq!(r["label"], "SR+");
}

#[test]
fn hil_ignores_unknown_message_types() {
    let port = pick_free_port();
    let mut guard = spawn_hil(port, r#"{"type":"FR","params":{"n":1}}"#);
    let stream = connect_with_retry(&mut guard, port, Duration::from_secs(5));
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .expect("set_read_timeout");
    let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
    let mut writer = stream;

    // Garbage + forward-compat unknown types should be silently dropped.
    writeln!(writer, "not-json-at-all").unwrap();
    writeln!(writer, r#"{{"type":"probe","time":0.5}}"#).unwrap();
    writeln!(
        writer,
        r#"{{"type":"stimulus","time":0.5,"stimulus":"house_light","on":true}}"#
    )
    .unwrap();
    // A real response on CRF should emit exactly one reinforcer.
    writeln!(
        writer,
        r#"{{"type":"response","time":1.0,"operandum":"main"}}"#
    )
    .unwrap();
    writer.flush().unwrap();

    let mut line = String::new();
    reader
        .read_line(&mut line)
        .expect("expected reinforcer line");
    let parsed: Value = serde_json::from_str(line.trim()).expect("valid JSON");
    assert_eq!(parsed["type"], "reinforcer");

    // Tear down.
    let _ = writeln!(writer, r#"{{"type":"disconnect"}}"#);
    let _ = writer.flush();
    drop(writer);
    drop(reader);

    let exited = wait_for_exit(&mut guard.0, Duration::from_secs(3));
    assert!(exited, "contingency-hil did not exit after disconnect");
}

#[test]
fn hil_builds_every_spec_type() {
    let specs = sample_specs();
    let covered: HashSet<&str> = specs
        .iter()
        .map(|s| s["type"].as_str().expect("type"))
        .collect();
    let missing: Vec<_> = SPEC_TYPES
        .iter()
        .filter(|t| !covered.contains(*t))
        .collect();
    assert!(
        missing.is_empty(),
        "spec types without a HIL session: {missing:?}"
    );
    assert_eq!(specs.len(), SPEC_TYPES.len());

    // "left" is one of the Concurrent sample's operanda; a response on an
    // operandum a Concurrent schedule does not know is a step error, which
    // ends the session.
    let responses = [(0.5, "left"), (1.0, "left"), (1.5, "left")];
    for spec in &specs {
        run_session(spec, &responses);
    }
}

#[test]
fn hil_vi_reinforces_over_the_wire() {
    let spec = json!({"type": "VI", "params": {"mean_interval": 2.0, "n_intervals": 6, "seed": 2}});
    let responses: Vec<(f64, &str)> = (1..=40).map(|i| (i as f64, "main")).collect();
    let got = times(&run_session(&spec, &responses));
    // One response per second over 40 s on VI 2 s: reinforcement happens,
    // but not on every response, and only at a response time.
    assert!(!got.is_empty(), "VI 2 produced no reinforcer over 40 s");
    assert!(
        got.len() < responses.len(),
        "VI 2 reinforced every response"
    );
    assert!(got
        .iter()
        .all(|t| t.fract() == 0.0 && *t >= 1.0 && *t <= 40.0));

    // Same seed, same spec → the same reinforced times.
    assert_eq!(got, times(&run_session(&spec, &responses)));
}

#[test]
fn hil_vr_reinforces_over_the_wire() {
    let spec = json!({"type": "VR", "params": {"mean": 3.0, "n_intervals": 6, "seed": 1}});
    let responses: Vec<(f64, &str)> = (1..=30).map(|i| (i as f64 * 0.5, "main")).collect();
    let got = times(&run_session(&spec, &responses));
    // VR 3 over 30 responses: about 10 reinforcers.
    assert!(
        (5..=15).contains(&got.len()),
        "VR 3 over 30 responses gave {} reinforcers",
        got.len()
    );
}

#[test]
fn hil_concurrent_routes_by_operandum() {
    let spec = json!({
        "type": "Concurrent",
        "params": {"components": {"left": fr(1), "right": fr(2)}, "cod": 0.0}
    });
    let responses = [
        (1.0, "left"),  // FR 1 on left → reinforced
        (2.0, "right"), // first of FR 2 on right
        (3.0, "right"), // second → reinforced
        (4.0, "left"),  // FR 1 on left → reinforced
    ];
    assert_eq!(times(&run_session(&spec, &responses)), vec![1.0, 3.0, 4.0]);
}

#[test]
fn hil_rejects_unknown_schedule_type_at_start_up() {
    let port = pick_free_port();
    let mut guard = spawn_hil(port, r#"{"type":"NotASchedule"}"#);
    assert!(
        wait_for_exit(&mut guard.0, Duration::from_secs(5)),
        "contingency-hil kept running with an unknown schedule type"
    );
    let status = guard.0.wait().expect("wait");
    assert!(!status.success(), "unknown type must fail start-up");
    let stderr = guard.stderr_text();
    assert!(
        stderr.contains("NotASchedule"),
        "stderr should name the bad type: {stderr}"
    );
}

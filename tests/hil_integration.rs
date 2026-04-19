//! Integration test for the `contingency-hil` binary.
//!
//! Spawns the binary on an ephemeral local port, pushes JSONL `response`
//! messages across a TCP socket, and asserts that the binary emits a
//! `reinforcer` JSONL message when the configured FR(3) schedule fires.
//!
//! The child process is always reaped — either via the in-band
//! `disconnect` message (graceful) or via `Child::kill` (fallback).

use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// Grab an ephemeral local port by binding to 0 and recording the
/// assigned port number, then immediately dropping the listener.
fn pick_free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral");
    let addr: SocketAddr = listener.local_addr().expect("local_addr");
    addr.port()
}

/// Retry connection until the HIL binary has its listener up.
fn connect_with_retry(port: u16, timeout: Duration) -> TcpStream {
    let deadline = Instant::now() + timeout;
    let mut last_err: Option<std::io::Error> = None;
    while Instant::now() < deadline {
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

#[test]
fn hil_fr3_reinforces_on_third_response() {
    let port = pick_free_port();

    let child = Command::new(env!("CARGO_BIN_EXE_contingency-hil"))
        .args([
            "--port",
            &port.to_string(),
            "--schedule",
            r#"{"type":"FR","params":{"n":3}}"#,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn contingency-hil");
    let mut guard = ChildGuard(child);

    let stream = connect_with_retry(port, Duration::from_secs(5));
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .expect("set_read_timeout");
    let reader_stream = stream.try_clone().expect("clone stream");
    let mut reader = BufReader::new(reader_stream);
    let mut writer = stream;

    // Send three responses; only the third should be reinforced under FR(3).
    for i in 1..=3 {
        let msg = format!(
            r#"{{"type":"response","time":{i}.0,"operandum":"main"}}"#,
            i = i
        );
        writeln!(writer, "{msg}").expect("write response");
        writer.flush().expect("flush");
    }

    // Read lines until we see a reinforcer (there should be exactly one).
    let mut got_reinforcer: Option<String> = None;
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline && got_reinforcer.is_none() {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => break, // EOF
            Ok(_) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if trimmed.contains("\"reinforcer\"") {
                    got_reinforcer = Some(trimmed.to_string());
                }
            }
            Err(_) => break, // read timeout — rely on outer deadline
        }
    }

    let reinforcer = got_reinforcer.expect("expected one reinforcer line for FR(3) trial");
    let parsed: serde_json::Value =
        serde_json::from_str(&reinforcer).expect("reinforcer line must parse as JSON");
    assert_eq!(parsed["type"], "reinforcer");
    assert_eq!(parsed["channel"], "food");
    assert!(
        (parsed["time"].as_f64().unwrap() - 3.0).abs() < 1e-9,
        "expected reinforcer.time == 3.0, got {}",
        parsed["time"]
    );
    assert_eq!(parsed["label"], "SR+");

    // Send disconnect and confirm the binary winds down on its own.
    let _ = writeln!(writer, r#"{{"type":"disconnect"}}"#);
    let _ = writer.flush();
    drop(writer);
    drop(reader);

    let exited = wait_for_exit(&mut guard.0, Duration::from_secs(3));
    assert!(exited, "contingency-hil did not exit after disconnect");
}

#[test]
fn hil_ignores_unknown_message_types() {
    let port = pick_free_port();

    let child = Command::new(env!("CARGO_BIN_EXE_contingency-hil"))
        .args([
            "--port",
            &port.to_string(),
            "--schedule",
            r#"{"type":"FR","params":{"n":1}}"#,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn contingency-hil");
    let mut guard = ChildGuard(child);

    let stream = connect_with_retry(port, Duration::from_secs(5));
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .expect("set_read_timeout");
    let reader_stream = stream.try_clone().expect("clone stream");
    let mut reader = BufReader::new(reader_stream);
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
    let parsed: serde_json::Value = serde_json::from_str(line.trim()).expect("valid JSON");
    assert_eq!(parsed["type"], "reinforcer");

    // Tear down.
    let _ = writeln!(writer, r#"{{"type":"disconnect"}}"#);
    let _ = writer.flush();
    drop(writer);
    drop(reader);

    let exited = wait_for_exit(&mut guard.0, Duration::from_secs(3));
    assert!(exited, "contingency-hil did not exit after disconnect");
}

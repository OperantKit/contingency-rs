//! `contingency-hil-serial` — run a CRF session against a serial device
//! through [`SerialApparatus`] and fetch the device's latency report.
//!
//! Built only with the `serial` feature. The device is the fake operant
//! chamber used for hardware-in-the-loop timing runs: it accepts the
//! line protocol of [`SerialApparatus`] (`R <operandum>` in, `D <channel>
//! <magnitude>` out) once it has been switched with
//! `{"cmd":"proto","p":"rs","duration":<s>}`, and it measures, inside the
//! device, the time from each response to the arrival of its reinforcer
//! command.
//!
//! Control lines sent besides the session traffic (one JSON object per line):
//!
//! * `{"cmd":"stop"}` — stop producing responses.
//! * `{"cmd":"proto","p":"rs","duration":<s>}` — speak the `R` / `D` protocol;
//!   `D` lines hold the feeder for `<s>` seconds.
//! * `{"cmd":"sync","t":<s>}` — start producing responses.
//! * `{"cmd":"report"}` — reply with `{"kind":"report","n":..,"lat_us":[..]}`.
//!
//! The summary JSON has the same keys as the Python host-side runner of the
//! same bench (`dsl`, `target_reinforcers`, `finished`, `n_reinforcers`,
//! `n_responses`, `elapsed_s`, `device`, ...), so the same latency tooling
//! reads either. `host_minus_device_ms` is empty because the `R` line does
//! not carry the device's time.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use clap::Parser;
use serde_json::{json, Value};

use contingency::hw::serial_backend::{SerialApparatus, SerialLink};
use contingency::hw::Apparatus;
use contingency::schedules;
use contingency::Schedule;

const OPERANDUM: &str = "0";
const CHANNEL: &str = "0";
const SETTLE: Duration = Duration::from_millis(200);

#[derive(Parser, Debug)]
#[command(
    name = "contingency-hil-serial",
    about = "Run a CRF session on a serial device with SerialApparatus and fetch its latency report"
)]
struct Cli {
    /// Serial port, e.g. /dev/cu.usbmodem…
    #[arg(long)]
    port: String,

    #[arg(long, default_value_t = 115_200)]
    baud: u32,

    /// Stop after this many reinforcers.
    #[arg(long, default_value_t = 200)]
    reinforcers: u64,

    /// Feeder duration in seconds sent to the device with the `proto` command.
    #[arg(long, default_value_t = 0.05)]
    duration: f64,

    /// Sleep between polls in seconds (the Python runner uses 0.001).
    #[arg(long, default_value_t = 0.001)]
    poll_interval: f64,

    #[arg(long, default_value_t = 600.0)]
    max_duration: f64,

    #[arg(long, default_value_t = 5.0)]
    report_timeout: f64,

    /// Write the summary JSON here.
    #[arg(long)]
    summary: Option<PathBuf>,

    /// Open the port as a plain non-blocking file without serial settings.
    /// For pty loopback tests: the `serialport` crate cannot set the baud
    /// rate on a macOS pty.
    #[arg(long)]
    plain_tty: bool,
}

/// Session settings shared by the binary and its tests.
#[derive(Debug, Clone)]
struct Config {
    reinforcers: u64,
    duration_s: f64,
    poll_interval: Duration,
    max_duration: Duration,
    report_timeout: Duration,
}

/// Time source; tests use a fake that advances only on `sleep`.
trait Clock {
    fn now(&mut self) -> f64;
    fn sleep(&mut self, d: Duration);
}

struct SystemClock(Instant);

impl Clock for SystemClock {
    fn now(&mut self) -> f64 {
        self.0.elapsed().as_secs_f64()
    }
    fn sleep(&mut self, d: Duration) {
        std::thread::sleep(d);
    }
}

#[derive(Debug, Clone, PartialEq)]
struct RunOutcome {
    finished: bool,
    n_reinforcers: u64,
    n_responses: u64,
    elapsed_s: f64,
    /// Host time from the poll that returned the response to the end of
    /// the reinforcer write, one per reinforcer, in microseconds.
    host_proc_us: Vec<u64>,
    device: Option<Value>,
}

fn send_control<W: Write>(control: &mut W, command: &Value) -> Result<()> {
    let mut line = serde_json::to_string(command)?;
    line.push('\n');
    control
        .write_all(line.as_bytes())
        .and_then(|_| control.flush())
        .context("writing a control line")
}

/// Run the session on `apparatus` and read the report through `control`.
///
/// `control` must reach the same device as the apparatus link (a cloned
/// port handle in the binary).
fn run_crf<L: SerialLink, C: Read + Write, K: Clock>(
    mut apparatus: SerialApparatus<L>,
    control: &mut C,
    clock: &mut K,
    cfg: &Config,
) -> Result<RunOutcome> {
    if cfg.reinforcers == 0 {
        return Err(anyhow!("reinforcers must be >= 1"));
    }
    let mut schedule = schedules::crf();
    send_control(control, &json!({"cmd": "stop"}))?;
    send_control(
        control,
        &json!({"cmd": "proto", "p": "rs", "duration": cfg.duration_s}),
    )?;
    clock.sleep(SETTLE);
    apparatus.connect().map_err(|e| anyhow!("{e}"))?;
    // Drop whatever the device sent before the session.
    let _ = apparatus.poll_responses(0.0);

    let t0 = clock.now();
    send_control(control, &json!({"cmd": "sync", "t": t0}))?;
    let mut n_reinforcers = 0u64;
    let mut n_responses = 0u64;
    let mut host_proc_us = Vec::new();
    let mut last_t = 0.0f64;
    while n_reinforcers < cfg.reinforcers {
        let t = (clock.now() - t0).max(last_t);
        if t > cfg.max_duration.as_secs_f64() {
            break;
        }
        last_t = t;
        let polled_at = Instant::now();
        let events = apparatus.poll_responses(t).map_err(|e| anyhow!("{e}"))?;
        for event in &events {
            n_responses += 1;
            let outcome = schedule
                .step(t, Some(event))
                .map_err(|e| anyhow!("schedule.step failed: {e}"))?;
            if let (true, Some(r)) = (outcome.reinforced, outcome.reinforcer) {
                apparatus
                    .deliver_reinforcer(t, &r, CHANNEL)
                    .map_err(|e| anyhow!("{e}"))?;
                host_proc_us.push(polled_at.elapsed().as_micros() as u64);
                n_reinforcers += 1;
                if n_reinforcers >= cfg.reinforcers {
                    break;
                }
            }
        }
        clock.sleep(cfg.poll_interval);
    }
    let elapsed_s = clock.now() - t0;
    apparatus.disconnect().map_err(|e| anyhow!("{e}"))?;

    let device = fetch_report(control, clock, cfg.report_timeout)?;
    Ok(RunOutcome {
        finished: n_reinforcers >= cfg.reinforcers,
        n_reinforcers,
        n_responses,
        elapsed_s,
        host_proc_us,
        device,
    })
}

/// Stop the device and read lines until its report arrives or time runs out.
fn fetch_report<C: Read + Write, K: Clock>(
    control: &mut C,
    clock: &mut K,
    timeout: Duration,
) -> Result<Option<Value>> {
    send_control(control, &json!({"cmd": "stop"}))?;
    send_control(control, &json!({"cmd": "report"}))?;
    let deadline = clock.now() + timeout.as_secs_f64();
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    while clock.now() < deadline {
        let n = match control.read(&mut chunk) {
            Ok(n) => n,
            Err(e)
                if e.kind() == std::io::ErrorKind::TimedOut
                    || e.kind() == std::io::ErrorKind::WouldBlock =>
            {
                0
            }
            Err(e) => return Err(e).context("reading the device report"),
        };
        buf.extend_from_slice(&chunk[..n]);
        while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = buf.drain(..=pos).collect();
            if let Ok(v) = serde_json::from_slice::<Value>(&line) {
                if v.get("kind").and_then(Value::as_str) == Some("report") {
                    return Ok(Some(v));
                }
            }
        }
        if n == 0 {
            clock.sleep(Duration::from_millis(1));
        }
    }
    Ok(None)
}

fn summary_json(run: &RunOutcome, cfg: &Config) -> Value {
    json!({
        "host": "rust",
        "dsl": "CRF",
        "target_reinforcers": cfg.reinforcers,
        "reinforcer_duration_s": cfg.duration_s,
        "finished": run.finished,
        "n_reinforcers": run.n_reinforcers,
        "n_responses": run.n_responses,
        "elapsed_s": (run.elapsed_s * 1000.0).round() / 1000.0,
        "host_minus_device_ms": [],
        "host_proc_us": run.host_proc_us,
        "device": run.device,
    })
}

/// Open the device and return two handles to it: the control handle and
/// the apparatus link.
fn open_port(
    path: &str,
    baud: u32,
    plain_tty: bool,
) -> Result<(Box<dyn SerialLink>, Box<dyn SerialLink>)> {
    if plain_tty {
        return open_plain_tty(path);
    }
    let control = serialport::new(path, baud)
        .timeout(Duration::from_millis(0))
        .open()
        .with_context(|| format!("opening serial port {path:?}"))?;
    let link = control.try_clone().context("cloning the serial port")?;
    Ok((Box::new(control), Box::new(link)))
}

#[cfg(unix)]
fn open_plain_tty(path: &str) -> Result<(Box<dyn SerialLink>, Box<dyn SerialLink>)> {
    use std::os::unix::fs::OpenOptionsExt;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOCTTY)
        .open(path)
        .with_context(|| format!("opening {path:?}"))?;
    let link = file.try_clone().context("cloning the file handle")?;
    Ok((Box::new(file), Box::new(link)))
}

#[cfg(not(unix))]
fn open_plain_tty(_path: &str) -> Result<(Box<dyn SerialLink>, Box<dyn SerialLink>)> {
    Err(anyhow!("--plain-tty is only available on Unix"))
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let cfg = Config {
        reinforcers: cli.reinforcers,
        duration_s: cli.duration,
        poll_interval: Duration::from_secs_f64(cli.poll_interval),
        max_duration: Duration::from_secs_f64(cli.max_duration),
        report_timeout: Duration::from_secs_f64(cli.report_timeout),
    };
    let (mut control, link) = open_port(&cli.port, cli.baud, cli.plain_tty)?;
    let apparatus = SerialApparatus::with_link(
        link,
        vec![OPERANDUM.into()],
        vec![CHANNEL.into()],
        vec![],
        "teensy-fake-chamber".into(),
    )
    .map_err(|e| anyhow!("{e}"))?;
    let mut clock = SystemClock(Instant::now());
    let run = run_crf(apparatus, &mut control, &mut clock, &cfg)?;

    let summary = summary_json(&run, &cfg);
    if let Some(path) = &cli.summary {
        std::fs::write(path, format!("{summary}\n"))
            .with_context(|| format!("writing {}", path.display()))?;
    }
    let device_n = run.device.as_ref().and_then(|d| d.get("n")).cloned();
    println!(
        "finished={} reinforcers={}/{} responses={} elapsed={:.3} s device_report_n={}",
        run.finished,
        run.n_reinforcers,
        cfg.reinforcers,
        run.n_responses,
        run.elapsed_s,
        device_n.map_or("None".into(), |v| v.to_string()),
    );
    if !run.finished {
        eprintln!("warning: the session did not reach the reinforcer count");
    }
    if run.device.is_none() {
        eprintln!("warning: no report from the device");
    }
    std::process::exit(if run.finished && run.device.is_some() {
        0
    } else {
        1
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    /// Scripted device: after `sync` it yields one `R 0` per read until
    /// `presses` run out, records every line the host writes, and answers
    /// `report` with the number of `D` lines it saw.
    #[derive(Default)]
    struct Device {
        presses: u64,
        active: bool,
        rx: VecDeque<u8>,
        written: Vec<String>,
        pending: Vec<u8>,
        d_lines: u64,
    }

    #[derive(Clone, Default)]
    struct Link(Arc<Mutex<Device>>);

    impl Read for Link {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let mut d = self.0.lock().unwrap();
            if d.rx.is_empty() && d.active && d.presses > 0 {
                d.presses -= 1;
                d.rx.extend(b"R 0\n");
            }
            let n = buf.len().min(d.rx.len());
            for (slot, byte) in buf.iter_mut().zip(d.rx.drain(..n)) {
                *slot = byte;
            }
            Ok(n)
        }
    }

    impl Write for Link {
        fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
            let mut d = self.0.lock().unwrap();
            d.pending.extend_from_slice(data);
            while let Some(pos) = d.pending.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = d.pending.drain(..=pos).collect();
                let text = String::from_utf8_lossy(&line).trim().to_string();
                if text.starts_with("D ") {
                    d.d_lines += 1;
                } else if text.contains("\"sync\"") {
                    d.active = true;
                } else if text.contains("\"stop\"") {
                    d.active = false;
                } else if text.contains("\"report\"") {
                    let rep = format!("{{\"kind\":\"report\",\"n\":{}}}\n", d.d_lines);
                    d.rx.extend(rep.as_bytes());
                }
                d.written.push(text);
            }
            Ok(data.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    struct FakeClock(f64);

    impl Clock for FakeClock {
        fn now(&mut self) -> f64 {
            self.0
        }
        fn sleep(&mut self, d: Duration) {
            self.0 += d.as_secs_f64();
        }
    }

    fn cfg(reinforcers: u64) -> Config {
        Config {
            reinforcers,
            duration_s: 0.05,
            poll_interval: Duration::from_millis(1),
            max_duration: Duration::from_secs(60),
            report_timeout: Duration::from_secs(1),
        }
    }

    fn apparatus(link: Link) -> SerialApparatus<Link> {
        SerialApparatus::with_link(
            link,
            vec![OPERANDUM.into()],
            vec![CHANNEL.into()],
            vec![],
            "mock".into(),
        )
        .unwrap()
    }

    #[test]
    fn crf_reinforces_every_response_and_stops_at_the_count() {
        let link = Link::default();
        link.0.lock().unwrap().presses = 50;
        let mut control = link.clone();
        let run = run_crf(
            apparatus(link.clone()),
            &mut control,
            &mut FakeClock(0.0),
            &cfg(5),
        )
        .unwrap();
        assert!(run.finished);
        assert_eq!(run.n_reinforcers, 5);
        assert_eq!(run.n_responses, 5);
        assert_eq!(run.host_proc_us.len(), 5);
        let d = link.0.lock().unwrap();
        assert_eq!(d.d_lines, 5);
        assert!(d.written.iter().any(|l| l == "D 0 1"));
        assert_eq!(run.device.unwrap()["n"], json!(5));
    }

    #[test]
    fn switches_the_device_to_the_rs_protocol_before_sync() {
        let link = Link::default();
        link.0.lock().unwrap().presses = 3;
        let mut control = link.clone();
        run_crf(
            apparatus(link.clone()),
            &mut control,
            &mut FakeClock(0.0),
            &cfg(1),
        )
        .unwrap();
        let d = link.0.lock().unwrap();
        let proto = d
            .written
            .iter()
            .position(|l| l.contains("\"proto\""))
            .unwrap();
        let sync = d
            .written
            .iter()
            .position(|l| l.contains("\"sync\""))
            .unwrap();
        assert!(proto < sync);
        let v: Value = serde_json::from_str(&d.written[proto]).unwrap();
        assert_eq!(v["p"], json!("rs"));
        assert_eq!(v["duration"], json!(0.05));
    }

    #[test]
    fn unfinished_when_the_device_stops_pressing() {
        let link = Link::default();
        link.0.lock().unwrap().presses = 2;
        let mut control = link.clone();
        let mut c = cfg(5);
        c.max_duration = Duration::from_millis(500);
        let run = run_crf(apparatus(link), &mut control, &mut FakeClock(0.0), &c).unwrap();
        assert!(!run.finished);
        assert_eq!(run.n_reinforcers, 2);
        assert!(run.device.is_some());
    }

    #[test]
    fn summary_uses_the_python_runner_keys() {
        let run = RunOutcome {
            finished: true,
            n_reinforcers: 1,
            n_responses: 1,
            elapsed_s: 0.30049,
            host_proc_us: vec![12],
            device: Some(json!({"kind": "report", "n": 1, "lat_us": [900]})),
        };
        let s = summary_json(&run, &cfg(1));
        for key in [
            "dsl",
            "target_reinforcers",
            "reinforcer_duration_s",
            "finished",
            "n_reinforcers",
            "n_responses",
            "elapsed_s",
            "host_minus_device_ms",
            "device",
        ] {
            assert!(s.get(key).is_some(), "missing {key}");
        }
        assert_eq!(s["elapsed_s"], json!(0.3));
        assert_eq!(s["device"]["lat_us"], json!([900]));
    }

    #[test]
    fn zero_reinforcers_is_rejected() {
        let link = Link::default();
        let mut control = link.clone();
        assert!(run_crf(apparatus(link), &mut control, &mut FakeClock(0.0), &cfg(0)).is_err());
    }
}

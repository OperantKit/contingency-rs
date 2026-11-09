//! `contingency-hil` — Hardware-In-the-Loop bridge binary.
//!
//! Speaks the JSONL TCP wire protocol consumed by the Python
//! `HilBridgeApparatus` under `contingency-py`. The binary plays the role
//! of an opaque apparatus *and* schedule-runner: it accepts inbound
//! `response` messages from a connected client, feeds them into a
//! configured [`Schedule`] via [`Schedule::step`], and emits `reinforcer`
//! messages back on the same socket whenever the schedule fires.
//!
//! Wire protocol (one JSON object per line, `\n` terminated):
//!
//! * Inbound (host → apparatus):
//!   - `{"type": "response", "time": <f64>, "operandum": "<name>"}`
//!   - `{"type": "reinforcer", "time": <f64>, "channel": "<name>",
//!      "magnitude": <f64>, "label": "<name>"}` — ignored by this binary
//!     (the binary is the reinforcement authority; the host's outbound
//!     `reinforcer` messages are dropped so both directions can share a
//!     socket symmetrically).
//!   - `{"type": "stimulus", "time": <f64>, "stimulus": "<name>",
//!      "on": <bool>}` — dropped (no HAL wiring yet).
//!   - `{"type": "disconnect"}` — graceful shutdown.
//!
//! * Outbound (apparatus → host):
//!   - `{"type": "reinforcer", "time": <f64>, "channel": "<name>",
//!      "magnitude": <f64>, "label": "<name>"}`
//!
//! The schedule is built once at start-up by
//! [`contingency::spec::from_spec`], so every schedule type in
//! [`contingency::spec::SPEC_TYPES`] is available; a spec it rejects
//! makes the binary exit with an error before it listens. The schedule
//! is stepped only when a `response` arrives — there is no clock tick on
//! the wire, so time-only events (e.g. FT/VT/RT deliveries) are not
//! emitted between responses.
//!
//! Unknown message `type` values are silently dropped for forward
//! compatibility.
//! JSON parse errors on individual lines are dropped and the stream
//! continues. EOF on the reader closes the session cleanly.

use anyhow::{anyhow, Context, Result};
use clap::Parser;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;

use contingency::{from_spec, ResponseEvent, Schedule};

/// Default reinforcer channel emitted when none is otherwise specified.
const DEFAULT_CHANNEL: &str = "food";

#[derive(Parser, Debug)]
#[command(
    name = "contingency-hil",
    about = "JSONL/TCP HIL bridge for the contingency schedule engine"
)]
struct Cli {
    /// TCP port to listen on.
    #[arg(long, default_value_t = 7788)]
    port: u16,

    /// Bind host.
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// Outbound reinforcer channel label.
    #[arg(long, default_value = DEFAULT_CHANNEL)]
    channel: String,

    /// Schedule spec as JSON. Shape mirrors `contingency-py`'s
    /// conformance `schedule` field, e.g. `{"type":"FR","params":{"n":5}}`.
    /// Any spec `type` that `contingency::spec::from_spec` accepts works,
    /// including nested compound schedules.
    #[arg(long)]
    schedule: Option<String>,

    /// Path to a schedule spec JSON file (alternative to `--schedule`).
    #[arg(long, conflicts_with = "schedule")]
    schedule_file: Option<PathBuf>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let spec_json = if let Some(path) = &cli.schedule_file {
        std::fs::read_to_string(path)
            .with_context(|| format!("reading schedule file {}", path.display()))?
    } else if let Some(s) = &cli.schedule {
        s.clone()
    } else {
        return Err(anyhow!("provide either --schedule or --schedule-file"));
    };

    let spec: Value = serde_json::from_str(&spec_json).context("parsing schedule spec JSON")?;
    let mut schedule = from_spec(&spec).context("building schedule")?;

    let listener = TcpListener::bind((cli.host.as_str(), cli.port))
        .with_context(|| format!("binding {}:{}", cli.host, cli.port))?;
    eprintln!("contingency-hil listening on {}:{}", cli.host, cli.port);

    // One-shot design: serve a single client, then exit. Callers that
    // want multi-session support can wrap this binary in a supervisor
    // that re-launches on exit.
    if let Some(stream) = listener.incoming().next() {
        let stream = stream.context("accepting TCP client")?;
        handle_client(stream, schedule.as_mut(), &cli.channel)?;
    }

    Ok(())
}

fn handle_client(stream: TcpStream, schedule: &mut dyn Schedule, channel: &str) -> Result<()> {
    let peer = stream
        .peer_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|_| "<unknown>".to_string());
    eprintln!("contingency-hil: client connected from {peer}");

    let reader_stream = stream
        .try_clone()
        .context("cloning TCP stream for buffered read")?;
    let reader = BufReader::new(reader_stream);
    let mut writer = stream;

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(err) => {
                eprintln!("contingency-hil: read error: {err}; closing session");
                break;
            }
        };
        if line.trim().is_empty() {
            continue;
        }

        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue, // drop malformed lines, per forward-compat
        };

        let ty = msg.get("type").and_then(Value::as_str);
        match ty {
            Some("response") => {
                let time = msg.get("time").and_then(Value::as_f64).unwrap_or(0.0);
                let operandum = msg
                    .get("operandum")
                    .and_then(Value::as_str)
                    .unwrap_or("main")
                    .to_string();
                let event = ResponseEvent {
                    time,
                    operandum: operandum.clone(),
                };
                let outcome = schedule
                    .step(time, Some(&event))
                    .map_err(|e| anyhow!("schedule.step failed: {e}"))?;
                if outcome.reinforced {
                    if let Some(r) = outcome.reinforcer {
                        let payload = json!({
                            "type": "reinforcer",
                            "time": r.time,
                            "channel": channel,
                            "magnitude": r.magnitude,
                            "label": r.label,
                        });
                        let mut line_out =
                            serde_json::to_string(&payload).context("serialising reinforcer")?;
                        line_out.push('\n');
                        writer
                            .write_all(line_out.as_bytes())
                            .context("writing reinforcer to socket")?;
                        writer.flush().context("flushing reinforcer")?;
                    }
                }
            }
            Some("disconnect") => {
                eprintln!("contingency-hil: client requested disconnect");
                break;
            }
            Some("stimulus") | Some("reinforcer") => {
                // No HAL outputs wired yet; ignore host-side control
                // traffic. The `reinforcer` branch is present so that
                // symmetric peers which echo the message shape do not
                // get surprising errors — the binary is the authority.
            }
            Some(_) | None => {
                // Unknown type: drop silently for forward compatibility.
            }
        }
    }

    eprintln!("contingency-hil: client disconnected");
    Ok(())
}

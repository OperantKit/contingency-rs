/**
 * OKL v1 (OperantKitLog) writer for the trials recorded by the contingency plugin.
 *
 * One jsPsych experiment becomes one OKL session; each contingency trial becomes one
 * phase (`phase_enter` ... `phase_exit`). Only canonical OKL v1 event types are written.
 */

export type OklMetaValue = string | number | boolean;

export type OklOptions = {
  /** Required: human-readable session identifier. */
  session_name: string;
  subject_id?: string;
  replication_index?: number;
  experiment_name?: string;
  protocol_id?: string;
  /** ISO-8601, timezone-aware, e.g. `jsPsych.getStartTime().toISOString()`. */
  wall_clock_start?: string;
  /** Free-form `# meta:` entries. */
  meta?: Record<string, OklMetaValue>;
};

type TrialEvent = {
  t: number;
  type: string;
  id: number;
  operandum: number | null;
  potency?: number;
};

type ContingencyTrial = {
  trial_type: string;
  trial_index: number;
  schedule: unknown;
  schedule_label: string;
  trial_start_total_ms: number;
  trial_start_perf_ms: number;
  trial_end_s: number;
  events: TrialEvent[];
};

const TRIAL_TYPE = "contingency";

const CODEBOOK: [string, string][] = [
  ["response", "id:int operandum:int?"],
  ["reinforcer_start", "id:int potency:float operandum:int?"],
  ["reinforcer_end", "id:int operandum:int?"],
  ["phase_enter", "label:str name:str? time:str? location:str? cue:str?"],
  ["phase_exit", "label:str"],
];

// Written after subject_id / replication_index, in the order of the header spec.
const LATER_STRING_KEYS = ["experiment_name", "protocol_id"] as const;

// Same shape as the reference reader's header key pattern.
const META_KEY = /^[A-Za-z_][A-Za-z0-9_.-]*$/;

// Characters that neither escape scheme can carry: control characters other than
// TAB / LF / CR, and characters that readers using Unicode line splitting treat as
// line breaks.
const UNREPRESENTABLE = /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f\u0085\u2028\u2029]/;

// Event times and trial starts come from one clock (performance.now), so they can only
// disagree by rounding.
const CLOCK_TOLERANCE_S = 1e-6;

function representable(value: string, where: string): string {
  const match = UNREPRESENTABLE.exec(value);
  if (match) {
    const code = match[0].charCodeAt(0).toString(16).padStart(4, "0");
    throw new Error(`toOkl: ${where} contains U+${code}, which OKL v1 cannot represent`);
  }
  return value;
}

/** TSV escaping for string body columns (OKL v1 wire format §5). */
export function escapeTsv(value: string): string {
  representable(value, "a body string");
  if (value === "-") return "\\-";
  return value.replace(/[\\\t\n\r]/g, (c) => ({ "\\": "\\\\", "\t": "\\t", "\n": "\\n", "\r": "\\r" })[c]);
}

/** TOML basic string for header values (only the escapes the OKL grammar allows). */
function tomlString(value: string, where: string): string {
  representable(value, where);
  const escaped = value.replace(/[\\"\t\n\r]/g, (c) => ({ "\\": "\\\\", '"': '\\"', "\t": "\\t", "\n": "\\n", "\r": "\\r" })[c]);
  return `"${escaped}"`;
}

function finite(value: number, what: string): number {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    throw new Error(`toOkl: ${what} must be a finite number, got ${String(value)}`);
  }
  return value;
}

/** Float spelling that always reads back as a float (`1` -> `1.0`). */
function formatFloat(value: number, what: string): string {
  // 15 significant digits drop the binary noise of ms -> s sums (5.250000000000001).
  const text = String(Number(finite(value, what).toPrecision(15)));
  return /^-?\d+$/.test(text) ? `${text}.0` : text;
}

function tomlValue(value: OklMetaValue, key: string): string {
  if (typeof value === "string") return tomlString(value, `meta.${key}`);
  if (typeof value === "boolean") return String(value);
  if (Number.isInteger(value)) {
    if (!Number.isSafeInteger(value)) {
      throw new Error(`toOkl: meta.${key} = ${value} is too large to write as an integer`);
    }
    return String(value);
  }
  return formatFloat(value, `meta.${key}`);
}

function metaKey(key: string): string {
  if (!META_KEY.test(key)) {
    throw new Error(`toOkl: meta key ${JSON.stringify(key)} must match ${META_KEY}`);
  }
  return key;
}

function contingencyRecords(data: unknown): Record<string, unknown>[] {
  const maybe = data as { filter?: unknown };
  if (data != null && !Array.isArray(data) && typeof maybe.filter === "function") {
    // jsPsych DataCollection
    const filtered = (maybe.filter as (q: object) => { values(): unknown[] })({ trial_type: TRIAL_TYPE });
    return filtered.values() as Record<string, unknown>[];
  }
  if (!Array.isArray(data)) {
    throw new Error("toOkl: expected jsPsych.data.get() or an array of trial records");
  }
  return data.filter((record) => record && record.trial_type === TRIAL_TYPE);
}

/**
 * Trials written by the plugin, in trial order. Records without the plugin's data (for
 * example a trial ended by `jsPsych.abortExperiment`) are counted and skipped.
 */
function splitTrials(records: Record<string, unknown>[]): { trials: ContingencyTrial[]; skipped: number } {
  const trials = records.filter((r) => Array.isArray(r.events)) as unknown as ContingencyTrial[];
  trials.sort((a, b) => a.trial_index - b.trial_index);
  return { trials, skipped: records.length - trials.length };
}

function header(options: OklOptions, extraMeta: [string, OklMetaValue][]): string[] {
  if (!options || typeof options.session_name !== "string" || options.session_name === "") {
    throw new Error("toOkl: options.session_name is required");
  }
  const lines = [
    "# OKL v1",
    `# session_name = ${tomlString(options.session_name, "session_name")}`,
    `# clock_type = "PerformanceNow"`,
    "# session_start = 0.0",
  ];
  if (options.subject_id != null) lines.push(`# subject_id = ${tomlString(options.subject_id, "subject_id")}`);
  if (options.replication_index != null) {
    if (!Number.isSafeInteger(options.replication_index)) {
      throw new Error("toOkl: replication_index must be an integer");
    }
    lines.push(`# replication_index = ${options.replication_index}`);
  }
  for (const key of LATER_STRING_KEYS) {
    const value = options[key];
    if (value != null) lines.push(`# ${key} = ${tomlString(value, key)}`);
  }
  if (options.wall_clock_start != null) {
    lines.push(`# wall_clock_start = ${tomlString(options.wall_clock_start, "wall_clock_start")}`);
  }
  const meta = [...Object.entries(options.meta ?? {}), ...extraMeta];
  if (meta.length > 0) {
    lines.push("# meta:");
    for (const [key, value] of meta) lines.push(`#   ${metaKey(key)} = ${tomlValue(value, key)}`);
  }
  return lines;
}

function optionalInt(value: number | null | undefined): string {
  return value == null ? "-" : String(value);
}

/** Serialise the contingency trials of a jsPsych experiment as one OKL v1 session. */
export function toOkl(data: unknown, options: OklOptions): string {
  const { trials, skipped } = splitTrials(contingencyRecords(data));
  const extraMeta: [string, OklMetaValue][] = trials.map((trial) => [
    `trial_${trial.trial_index}_schedule`,
    JSON.stringify(trial.schedule),
  ]);
  if (skipped > 0) extraMeta.push(["skipped_trials", skipped]);
  const lines = header(options, extraMeta);

  lines.push("# events:");
  if (trials.length > 0) {
    for (const [name, fields] of CODEBOOK) lines.push(`#   ${name.padEnd(18)}: ${fields}`);
  }
  lines.push("# ---");

  // Session time 0 is the start of the experiment: the first trial's start on jsPsych's
  // clock, carried over to the performance clock that all event times are on.
  const origin =
    trials.length > 0
      ? finite(trials[0].trial_start_perf_ms, "trial_start_perf_ms") -
        finite(trials[0].trial_start_total_ms, "trial_start_total_ms")
      : 0;

  let responseId = 0;
  let reinforcerId = 0;
  let last = 0;
  const stamp = (seconds: number, where: string) => {
    if (seconds < last - CLOCK_TOLERANCE_S) {
      throw new Error(`toOkl: ${where} at ${seconds} s is earlier than the previous record (${last} s)`);
    }
    last = Math.max(last, seconds);
    return formatFloat(last, where);
  };

  for (const trial of trials) {
    const where = `trial ${trial.trial_index}`;
    const start = (finite(trial.trial_start_perf_ms, "trial_start_perf_ms") - origin) / 1000;
    const end = start + finite(trial.trial_end_s, "trial_end_s");
    const label = escapeTsv(`trial ${trial.trial_index} ${trial.schedule_label}`);
    lines.push(`${stamp(start, where)}\tphase_enter\t${label}\t-\t-\t-\t-`);

    // Trial-local reinforcer id -> [session id, operandum] for presentations still open.
    const open = new Map<number, [number, number | null]>();
    const events = [...trial.events].sort((a, b) => a.t - b.t);
    for (const event of events) {
      const ts = stamp(start + finite(event.t, `${where} event t`), where);
      const operandum = optionalInt(event.operandum);
      switch (event.type) {
        case "response":
          lines.push(`${ts}\tresponse\t${responseId++}\t${operandum}`);
          break;
        case "reinforcer_start": {
          const id = reinforcerId++;
          open.set(event.id, [id, event.operandum]);
          const potency = formatFloat(event.potency ?? 1, `${where} potency`);
          lines.push(`${ts}\treinforcer_start\t${id}\t${potency}\t${operandum}`);
          break;
        }
        case "reinforcer_end": {
          const entry = open.get(event.id);
          if (entry === undefined) {
            throw new Error(`toOkl: reinforcer_end ${event.id} in ${where} has no start`);
          }
          open.delete(event.id);
          lines.push(`${ts}\treinforcer_end\t${entry[0]}\t${operandum}`);
          break;
        }
        default:
          throw new Error(`toOkl: unsupported event type ${JSON.stringify(event.type)} in ${where}`);
      }
    }
    // The trial end clears the display, which ends any presentation still running.
    const exit = stamp(end, where);
    for (const [id, operandum] of open.values()) {
      lines.push(`${exit}\treinforcer_end\t${id}\t${optionalInt(operandum)}`);
    }
    lines.push(`${exit}\tphase_exit\t${label}`);
  }
  return lines.join("\n") + "\n";
}

/** Offer the OKL text as a file download (jsPsych's localSave only writes csv/json). */
export function downloadOkl(text: string, filename = "session.txt"): void {
  const url = URL.createObjectURL(new Blob([text], { type: "text/plain;charset=utf-8" }));
  const link = document.createElement("a");
  link.href = url;
  link.download = filename;
  document.body.appendChild(link);
  link.click();
  link.remove();
  URL.revokeObjectURL(url);
}

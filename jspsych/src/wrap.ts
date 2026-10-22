import { Schedule } from "contingency";

/** A schedule given as plain data in the trial parameters. */
export type ScheduleSpec =
  | { type: "FR"; n: number }
  | { type: "VI"; mean: number; n?: number; seed?: number | null }
  | { type: "CRF" }
  | { type: "EXT" };

/** Number of Fleshler-Hoffman intervals when a VI spec does not give `n`. */
export const DEFAULT_VI_INTERVALS = 12;

const positiveInteger = (value: unknown, name: string): bigint => {
  if (typeof value !== "number" || !Number.isInteger(value) || value < 1) {
    throw new Error(`contingency: ${name} must be a positive integer, got ${String(value)}`);
  }
  // u64 parameters cross the wasm-bindgen boundary as bigint.
  return BigInt(value);
};

const positiveSeconds = (value: unknown, name: string): number => {
  if (typeof value !== "number" || !Number.isFinite(value) || value <= 0) {
    throw new Error(`contingency: ${name} must be a positive number of seconds, got ${String(value)}`);
  }
  return value;
};

const optionalSeed = (value: unknown): bigint | null => {
  if (value == null) return null;
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 0) {
    throw new Error(`contingency: VI seed must be a non-negative integer, got ${String(value)}`);
  }
  return BigInt(value);
};

/** Build an engine schedule from its plain-data description. */
export function buildSchedule(spec: ScheduleSpec): Schedule {
  if (spec == null || typeof spec !== "object") {
    throw new Error("contingency: the `schedule` parameter is required");
  }
  switch (spec.type) {
    case "FR":
      return Schedule.fr(positiveInteger(spec.n, "FR n"));
    case "VI": {
      // n_intervals is a usize, which wasm-bindgen takes as a plain number.
      const n = Number(positiveInteger(spec.n ?? DEFAULT_VI_INTERVALS, "VI n"));
      return Schedule.vi(positiveSeconds(spec.mean, "VI mean"), n, optionalSeed(spec.seed));
    }
    case "CRF":
      return Schedule.crf();
    case "EXT":
      return Schedule.ext();
    default:
      throw new Error(`contingency: unsupported schedule type ${JSON.stringify((spec as { type?: unknown }).type)}`);
  }
}

/** Decimal places kept when a time is written in a label. */
const LABEL_SECONDS_DECIMALS = 3;

/**
 * Seconds rounded to three decimals with trailing zeros dropped ("0.3", "2.5", "30"), so
 * that binary rounding noise such as 0.1 + 0.2 = 0.30000000000000004 does not show.
 * A positive time that would round to 0 keeps three significant digits instead.
 */
function formatSeconds(seconds: number): string {
  const rounded = Number(seconds.toFixed(LABEL_SECONDS_DECIMALS));
  return String(rounded > 0 ? rounded : Number(seconds.toPrecision(LABEL_SECONDS_DECIMALS)));
}

/** Label in contingency-dsl notation, e.g. "FR 5" or "VI 30s". */
export function scheduleLabel(spec: ScheduleSpec): string {
  switch (spec.type) {
    case "FR":
      return `FR ${spec.n}`;
    case "VI":
      return `VI ${formatSeconds(spec.mean)}s`;
    default:
      return spec.type;
  }
}

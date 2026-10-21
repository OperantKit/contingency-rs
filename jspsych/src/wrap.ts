import { Schedule } from "contingency";

/** A schedule given as plain data in the trial parameters. */
export type ScheduleSpec = { type: "FR"; n: number } | { type: "CRF" } | { type: "EXT" };

const positiveInteger = (value: unknown, name: string): bigint => {
  if (typeof value !== "number" || !Number.isInteger(value) || value < 1) {
    throw new Error(`contingency: ${name} must be a positive integer, got ${String(value)}`);
  }
  // u64 parameters cross the wasm-bindgen boundary as bigint.
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
    case "CRF":
      return Schedule.crf();
    case "EXT":
      return Schedule.ext();
    default:
      throw new Error(`contingency: unsupported schedule type ${JSON.stringify((spec as { type?: unknown }).type)}`);
  }
}

/** Human-readable label, e.g. "FR 5". */
export function scheduleLabel(spec: ScheduleSpec): string {
  switch (spec.type) {
    case "FR":
      return `FR ${spec.n}`;
    default:
      return spec.type;
  }
}

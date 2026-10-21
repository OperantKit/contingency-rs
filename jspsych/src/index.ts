import { JsPsych, JsPsychPlugin, ParameterType, TrialType } from "jspsych";
import { ResponseEvent } from "contingency";

import { version } from "../package.json";
import { loadEngine } from "./load";
import { buildSchedule, scheduleLabel, ScheduleSpec } from "./wrap";

const info = <const>{
  name: "contingency",
  version: version,
  parameters: {
    /** Schedule as plain data, e.g. `{ type: "FR", n: 5 }`. Supported: FR, CRF, EXT. */
    schedule: {
      type: ParameterType.OBJECT,
      default: undefined,
    },
    /** One button per operandum; the names are passed to the engine. */
    operanda: {
      type: ParameterType.STRING,
      array: true,
      default: ["main"],
    },
    /** Returns the HTML of one button. */
    button_html: {
      type: ParameterType.FUNCTION,
      default: function (operandum: string, operandum_index: number) {
        return `<button class="jspsych-btn">${operandum}</button>`;
      },
    },
    /** HTML shown above the buttons. */
    stimulus: {
      type: ParameterType.HTML_STRING,
      default: "",
    },
    /** HTML shown in place of the stimulus while a reinforcer is presented. */
    reinforcer_html: {
      type: ParameterType.HTML_STRING,
      default: '<div class="contingency-reinforcer" style="font-size:48px;">+</div>',
    },
    /** How long the reinforcer is presented, in milliseconds. */
    reinforcer_duration: {
      type: ParameterType.INT,
      default: 1000,
    },
    /** `"ignore"` drops clicks during reinforcer presentation; `"count"` records them and passes them to the engine. */
    responses_during_reinforcer: {
      type: ParameterType.SELECT,
      options: ["ignore", "count"],
      default: "ignore",
    },
    /** End the trial after this many milliseconds (null: no limit). */
    trial_duration: {
      type: ParameterType.INT,
      default: null,
    },
    /** End the trial once this many reinforcers have been presented (null: no limit). */
    max_reinforcers: {
      type: ParameterType.INT,
      default: null,
    },
    /** Period, in milliseconds, at which the engine is stepped without a response (time-based schedules). */
    tick_interval: {
      type: ParameterType.INT,
      default: 50,
    },
    /** Key that ends the trial (null: none). */
    end_key: {
      type: ParameterType.KEY,
      default: null,
    },
    /** URL of contingency_bg.wasm (null: next to this script). */
    wasm_url: {
      type: ParameterType.STRING,
      default: null,
    },
  },
  data: {
    /** Copy of the `schedule` parameter. */
    schedule: { type: ParameterType.OBJECT },
    /** Human-readable schedule, e.g. "FR 5". */
    schedule_label: { type: ParameterType.STRING },
    /** Copy of the `operanda` parameter. */
    operanda: { type: ParameterType.STRING, array: true },
    /** `jsPsych.getTotalTime()` when the trial started, in milliseconds. */
    trial_start_total_ms: { type: ParameterType.FLOAT },
    /** Responses and reinforcer onsets/offsets; `t` is seconds from the trial start. */
    events: {
      type: ParameterType.COMPLEX,
      array: true,
      nested: {
        t: { type: ParameterType.FLOAT },
        type: { type: ParameterType.STRING },
        id: { type: ParameterType.INT },
        operandum: { type: ParameterType.INT },
        potency: { type: ParameterType.FLOAT },
      },
    },
    /** Number of recorded responses. */
    n_responses: { type: ParameterType.INT },
    /** Number of reinforcers delivered. */
    n_reinforcers: { type: ParameterType.INT },
    /** `"duration"`, `"max_reinforcers"` or `"end_key"`. */
    end_reason: { type: ParameterType.STRING },
  },
  // prettier-ignore
  citations: '__CITATIONS__',
};

type Info = typeof info;

type ContingencyEvent = {
  t: number;
  type: "response" | "reinforcer_start" | "reinforcer_end";
  id: number;
  operandum: number | null;
  potency?: number;
};

/**
 * **contingency**
 *
 * Runs one reinforcement schedule from the contingency engine (WebAssembly) for the
 * length of a trial. Each button is an operandum; clicks are passed to the engine and
 * reinforcers are presented when the schedule arranges them.
 */
class ContingencyPlugin implements JsPsychPlugin<Info> {
  static info = info;

  constructor(private jsPsych: JsPsych) {}

  async trial(display_element: HTMLElement, trial: TrialType<Info>) {
    await loadEngine(trial.wasm_url);
    const spec = trial.schedule as ScheduleSpec;
    const schedule = buildSchedule(spec);
    const operanda: string[] = trial.operanda;

    const t0 = performance.now();
    const trialStartTotalMs = this.jsPsych.getTotalTime();
    const now = () => (performance.now() - t0) / 1000;

    const events: ContingencyEvent[] = [];
    let nResponses = 0;
    let nReinforcers = 0;
    let presenting = 0;
    let finished = false;

    display_element.innerHTML =
      `<div id="jspsych-contingency-stimulus">${trial.stimulus}</div>` +
      `<div id="jspsych-contingency-reinforcer" style="display:none;">${trial.reinforcer_html}</div>` +
      `<div id="jspsych-contingency-btngroup" class="jspsych-btn-group-flex"></div>`;
    const stimulusEl = display_element.querySelector<HTMLElement>("#jspsych-contingency-stimulus");
    const reinforcerEl = display_element.querySelector<HTMLElement>("#jspsych-contingency-reinforcer");
    const buttonGroup = display_element.querySelector<HTMLElement>("#jspsych-contingency-btngroup");

    return new Promise<object>((resolve) => {
      const finish = (reason: string) => {
        if (finished) return;
        finished = true;
        schedule.free();
        this.jsPsych.pluginAPI.cancelAllKeyboardResponses();
        resolve({
          schedule: spec,
          schedule_label: scheduleLabel(spec),
          operanda: operanda,
          trial_start_total_ms: trialStartTotalMs,
          events: events,
          n_responses: nResponses,
          n_reinforcers: nReinforcers,
          end_reason: reason,
        });
      };

      const reachedMaxReinforcers = () =>
        trial.max_reinforcers !== null && nReinforcers >= trial.max_reinforcers;

      const setPresenting = (on: boolean) => {
        stimulusEl.style.display = on ? "none" : "";
        reinforcerEl.style.display = on ? "" : "none";
      };

      const deliver = (outcome: { reinforced: boolean; reinforcer?: { magnitude: number } }, operandum: number | null) => {
        if (!outcome.reinforced) return;
        const id = nReinforcers++;
        const potency = outcome.reinforcer ? outcome.reinforcer.magnitude : 1.0;
        events.push({ t: now(), type: "reinforcer_start", id, operandum, potency });
        const end = () => {
          events.push({ t: now(), type: "reinforcer_end", id, operandum });
          presenting = Math.max(0, presenting - 1);
          if (presenting === 0) setPresenting(false);
          if (reachedMaxReinforcers()) finish("max_reinforcers");
        };
        if (trial.reinforcer_duration > 0) {
          presenting++;
          setPresenting(true);
          this.jsPsych.pluginAPI.setTimeout(end, trial.reinforcer_duration);
        } else {
          presenting++;
          end();
        }
      };

      // The engine requires `now` to equal the event time when a response is passed.
      const step = (t: number, event: ResponseEvent | null, operandum: number | null) => {
        const outcome = schedule.step(t, event);
        const result = {
          reinforced: outcome.reinforced,
          reinforcer: outcome.reinforcer ? { magnitude: outcome.reinforcer.magnitude } : undefined,
        };
        outcome.free();
        deliver(result, operandum);
      };

      const respond = (index: number) => {
        if (finished) return;
        if (presenting > 0 && trial.responses_during_reinforcer === "ignore") return;
        const t = now();
        events.push({ t, type: "response", id: nResponses++, operandum: index });
        step(t, new ResponseEvent(t, operanda[index]), index);
      };

      operanda.forEach((operandum, index) => {
        buttonGroup.insertAdjacentHTML("beforeend", trial.button_html(operandum, index));
        const button = buttonGroup.lastElementChild as HTMLElement;
        button.dataset.operandum = String(index);
        button.addEventListener("click", () => respond(index));
      });

      const tick = () => {
        if (finished) return;
        step(now(), null, null);
        this.jsPsych.pluginAPI.setTimeout(tick, trial.tick_interval);
      };
      this.jsPsych.pluginAPI.setTimeout(tick, trial.tick_interval);

      if (trial.trial_duration !== null) {
        this.jsPsych.pluginAPI.setTimeout(() => finish("duration"), trial.trial_duration);
      }
      if (trial.end_key !== null) {
        this.jsPsych.pluginAPI.getKeyboardResponse({
          callback_function: () => finish("end_key"),
          valid_responses: [trial.end_key],
          rt_method: "performance",
          persist: false,
          allow_held_key: false,
        });
      }
    });
  }
}

export default ContingencyPlugin;

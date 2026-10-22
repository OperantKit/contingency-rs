# Concurrent — compound with Changeover Delay / Ratio

:jp: [日本語版](../../ja/algorithms/concurrent.md)

## References

Catania, A. C. (1963). Concurrent performances: Reinforcement
interaction and response independence. *Journal of the Experimental
Analysis of Behavior*, 6(2), 253-263.
https://doi.org/10.1901/jeab.1963.6-253

Herrnstein, R. J. (1961). Relative and absolute strength of response
as a function of frequency of reinforcement. *JEAB*, 4(3), 267-272.
https://doi.org/10.1901/jeab.1961.4-267

## Mathematical definition

A `Concurrent` schedule presents `k >= 2` component schedules on
distinct operanda. Responses with `event.operandum == key` are routed
to `components[key]`; all other components receive a tick at the same
`now`.

### Changeover Delay (COD)

When the subject switches from operandum A to B, no response on the
new operandum is reinforced for `cod` time units after the switch.
The timer anchors on the switching response and re-anchors on every
subsequent confirmed switch.

A reinforcer that B's component sets up during the COD is **held**,
not lost. It is delivered by the first response on B after the COD
ends, and B's component does not start its next interval (or ratio)
until that delivery. Switching away from B during the COD keeps the
reinforcer on B. This is the procedure of Herrnstein (1961): no
response was reinforced within 1.5 s of a changeover, and the
programmed reinforcer stayed available until collected. If another
component delivers a reinforcer on its tick at the step where B's
reinforcer is held and no punisher fires, that reinforcer is still
returned.

### Fixed-Ratio Changeover (FRCO)

With `frco > 0`, a switch only *counts* as a changeover after `frco`
consecutive responses on the new operandum. The COD timer arms only
on the `frco`-th such response. Responses `1..frco-1` on the new
operandum are not yet "a changeover" and are **not** COD-gated.

## State variables

| Name | Type | Purpose |
|---|---|---|
| `components` | dict[str, Schedule] | Component schedules by operandum key. |
| `cod` | float (const) | COD duration (`>= 0`; `0` disables). |
| `frco` | int (const) | FRCO threshold (`>= 0`; `0` disables). |
| `last_operandum` | str? | Operandum of the most recent **confirmed** response. |
| `switch_time` | float? | Time of the last confirmed changeover (for COD). |
| `consecutive_new_count` | int | Running count during a FRCO streak. |
| `pending` | dict[str, list[Reinforcer]] | Reinforcers held by the compound per operandum, oldest first (components other than FR/VR/RR/FI/VI/RI). |
| `last_now` | float? | Monotonic-time check. |

## Step pseudocode

```
fn step(now, event):
    check_monotonic(last_now, now)
    check_event_time(now, event)
    last_now = now

    if event is None:
        // Pure tick: advance every component; no changeover logic.
        outcome, _, _ = advance_components(now, None, None, gated=false)
        return outcome

    operandum = event.operandum
    if operandum not in components:
        raise Config(...)

    // Register changeover state BEFORE gating: the changeover
    // response itself is subject to the COD it opens.
    register_event(operandum, now)
    gated = cod_active(operandum, now)

    outcome, from_event, held = advance_components(now, operandum, event, gated)
    if not gated:
        outcome, from_event = deliver_pending(operandum, now, outcome, from_event)

    if held:
        // The reinforcer stays set up on the component. A reinforcer
        // another component delivered on its tick is still returned.
        if outcome.reinforced and not from_event:
            return outcome with meta["cod_suppressed"] = true
        // Otherwise meta records that this response was not
        // reinforced because of the COD.
        return Outcome::unreinforced_with_meta({
            "cod_suppressed": true, "operandum": operandum,
        })
    return outcome
```

### `advance_components(now, event_operandum, event, gated)`

Steps every component with `(now, None)` except the matched one,
which gets `(now, event)`. When `gated`, the matched component is
stepped through `step_gated` instead. Returns
`(outcome, from_event, held)`:

1. Event-matched component's outcome if it reinforced → `from_event=True`.
2. Otherwise, first reinforced tick outcome from any other component
   (insertion order) → `from_event=False`.
3. Otherwise, event-matched component's (unreinforced) outcome,
   preserving its `meta` → `from_event=True`.
4. Otherwise empty `Outcome()` → `from_event=False`.

`held` is `true` iff `step_gated` held a reinforcer on the matched
component.

Rationale: a time-based component (FT/VT/RT) that fires on a pure
tick must not be silently dropped just because an event arrived on
another operandum.

### `step_gated(component, now, event)`

```
if component is FR | VR | RR | FI | VI | RI:
    probe = deep_copy(component)
    if probe.step(now, event).reinforced:
        component.step(now, None)  // tick only: reinforcer stays set up
        return Outcome(), held=true
    return component.step(now, event), held=false

out = component.step(now, event)
if out.reinforced:
    pending[event.operandum].push(out.reinforcer)
    return Outcome(), held=true
return out, held=false
```

For ratio and interval schedules the response is tried on a copy
first. If it would reinforce, the real component sees only a time
tick, so an interval schedule stays armed and a ratio schedule stays
one response short of its requirement; the first response after the
COD then completes it, and the next interval or ratio starts there. A
response that would not reinforce is applied to the real component,
so responses inside the COD still count toward a ratio.

Any other component (FT/VT/RT, DRO/DRL/DRH, nested compounds) can
reinforce on a tick or needs to see every response, so withholding
the response would lose the reinforcer or corrupt the component. The
response is applied as usual and the reinforcer is queued in
`pending`; the component's own next cycle starts at once.

In the Rust port the type test is `Schedule::probe_response`: FR, VR,
RR, FI, VI and RI return `Some(would_reinforce)` by stepping a `clone`
of themselves, and every other schedule (including `LimitedHold` and
user-defined schedules) returns the default `None` and takes the
queue path. `pending` is an `IndexMap<String, VecDeque<Reinforcer>>`.

### `deliver_pending(operandum, now, outcome, from_event)`

Called after `advance_components` for an ungated response. If
`pending[operandum]` is empty, returns its inputs. Otherwise a
reinforcer the component produced on this response joins the back of
the queue, and the oldest one is delivered with `time = now`
(`from_event = true`), so each response delivers at most one
reinforcer. `reset()` clears `pending`.

### `register_event(operandum, now)`

- First response ever (`last_operandum is None`): record operandum,
  reset streak. Not a changeover.
- `operandum == last_operandum`: reset `consecutive_new_count = 0`.
- `operandum != last_operandum`:
  - if `frco == 0`: set `switch_time = now`,
    `last_operandum = operandum`, reset streak. Immediate changeover.
  - if `frco > 0`: increment `consecutive_new_count`. When it reaches
    `frco`, confirm the changeover (set `switch_time = now`,
    `last_operandum = operandum`, reset streak).

### `cod_active(operandum, now)`

Returns `True` iff:

- `cod > 0`
- `switch_time is not None`
- `operandum == last_operandum` (gating applies only to the operandum
  we have switched **to**)
- `(now - switch_time) < cod - TIME_TOL`

## Reset semantics

- `component.reset()` for every component.
- `last_operandum = None`
- `switch_time = None`
- `consecutive_new_count = 0`
- `last_now = None`
- `pending = {}`

## Edge cases

- `< 2` components: `Config`.
- Negative `cod` or `frco`: `Config`.
- Unknown operandum in event: `Config`.
- **Gating order**: the changeover response is covered by the COD
  window it opens. Switching to a new operandum and earning a
  reinforcer on the same response holds that reinforcer until the
  first response after the COD.
- **Tick-side reinforcement is never gated.** If a response on A and
  a timer-fired reinforcer on B land on the same `step()` call, B's
  reinforcer is surfaced with `from_event=False` and not COD-gated.
- If multiple non-event components fire on the same tick, the first
  (insertion order) wins; the others are silently dropped. Callers
  should step often enough to avoid this.

## Directional COD

Per-direction COD overrides are supported via `cod_directional`,
which maps `(from_operandum, to_operandum)` → seconds. When a
matching transition occurs, the directional value replaces the base
`cod` for that switch only. Self-transitions and negative values are
rejected at construction (`Config` error). The base `cod` continues
to apply to any direction not present in the map.

In the Rust port this is `cod_directional: IndexMap<(String, String), f64>`
on `Concurrent`. The Python port exposes it as a
`dict[tuple[str, str], float]` keyword argument.

## Component-form punishment

`punish: dict[str, Schedule]` (Rust:
`IndexMap<String, Box<dyn Schedule>>`) attaches a per-operandum
punishment schedule that is stepped after the reinforcement decision.
Punishment schedules do **not** share COD state with the main
components. Negative-magnitude reinforcers (`label="SR-"`) emitted by
these sub-schedules surface in the returned `Outcome` as the active
reinforcer; reinforcement and punishment are mutually exclusive
within a single step.

## Determinism

Inherits the determinism of its components. No RNG in the compound
itself. The probe in `step_gated` is a copy, so a discarded probe
does not advance the real component's RNG.

## Divergence from Swift original

No concurrent schedule in the legacy Swift package. This compound is
new in both ports.

# Concurrent — compound with Changeover Delay / Ratio

:jp: [日本語版](../../ja/algorithms/concurrent.md)

## References

Catania, A. C. (1966). Concurrent performances: Reinforcement
interaction and response independence. *Journal of the Experimental
Analysis of Behavior*, 9(3), 253-263.
https://doi.org/10.1901/jeab.1966.9-253

Herrnstein, R. J. (1961). Relative and absolute strength of response
as a function of frequency of reinforcement. *JEAB*, 4(3), 267-272.
https://doi.org/10.1901/jeab.1961.4-267

## Mathematical definition

A `Concurrent` schedule presents `k >= 2` component schedules on
distinct operanda. Responses with `event.operandum == key` are routed
to `components[key]`; all other components receive a tick at the same
`now`.

### Changeover Delay (COD)

When the subject switches from operandum A to B, reinforcement on
the new operandum is suppressed (consumed silently) for `cod` time
units after the switch. The timer anchors on the switching response
and re-anchors on every subsequent confirmed switch.

### Changeover Ratio (COR / FRCO)

With `cor > 0`, a switch only *counts* as a changeover after `cor`
consecutive responses on the new operandum. The COD timer arms only
on the `cor`-th such response. Responses `1..cor-1` on the new
operandum are not yet "a changeover" and are **not** COD-gated.

## State variables

| Name | Type | Purpose |
|---|---|---|
| `components` | dict[str, Schedule] | Component schedules by operandum key. |
| `cod` | float (const) | COD duration (`>= 0`; `0` disables). |
| `cor` | int (const) | COR threshold (`>= 0`; `0` disables). |
| `last_operandum` | str? | Operandum of the most recent **confirmed** response. |
| `switch_time` | float? | Time of the last confirmed changeover (for COD). |
| `consecutive_new_count` | int | Running count during a COR streak. |
| `last_now` | float? | Monotonic-time check. |

## Step pseudocode

```
fn step(now, event):
    check_monotonic(last_now, now)
    check_event_time(now, event)
    last_now = now

    if event is None:
        // Pure tick: advance every component; no changeover logic.
        outcome, _ = advance_components(now, None, None)
        return outcome

    operandum = event.operandum
    if operandum not in components:
        raise Config(...)

    outcome, from_event = advance_components(now, operandum, event)

    // Register changeover state BEFORE gating: the changeover
    // response itself is subject to the COD it opens.
    register_event(operandum, now)

    if outcome.reinforced and from_event and cod_active(operandum, now):
        // Consume reinforcement; meta records the suppression.
        return Outcome::unreinforced_with_meta({
            "cod_suppressed": true, "operandum": operandum,
        })
    return outcome
```

### `advance_components(now, event_operandum, event)`

Steps every component with `(now, None)` except the matched one,
which gets `(now, event)`. Returns `(outcome, from_event)`:

1. Event-matched component's outcome if it reinforced → `from_event=True`.
2. Otherwise, first reinforced tick outcome from any other component
   (insertion order) → `from_event=False`.
3. Otherwise, event-matched component's (unreinforced) outcome,
   preserving its `meta` → `from_event=True`.
4. Otherwise empty `Outcome()` → `from_event=False`.

Rationale: a time-based component (FT/VT/RT) that fires on a pure
tick must not be silently dropped just because an event arrived on
another operandum.

### `register_event(operandum, now)`

- First response ever (`last_operandum is None`): record operandum,
  reset streak. Not a changeover.
- `operandum == last_operandum`: reset `consecutive_new_count = 0`.
- `operandum != last_operandum`:
  - if `cor == 0`: set `switch_time = now`,
    `last_operandum = operandum`, reset streak. Immediate changeover.
  - if `cor > 0`: increment `consecutive_new_count`. When it reaches
    `cor`, confirm the changeover (set `switch_time = now`,
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

## Edge cases

- `< 2` components: `Config`.
- Negative `cod` or `cor`: `Config`.
- Unknown operandum in event: `Config`.
- **Gating order**: the changeover response is covered by the COD
  window it opens. Switching to a new operandum and earning a
  reinforcer on the same response suppresses that reinforcer.
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
itself.

## Divergence from Swift original

No concurrent schedule in the legacy Swift package. This compound is
new in both ports.

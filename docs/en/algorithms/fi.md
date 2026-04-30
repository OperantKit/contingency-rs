# FI — Fixed Interval

:jp: [日本語版](../../ja/algorithms/fi.md)

## References

Ferster, C. B., & Skinner, B. F. (1957). *Schedules of reinforcement*.
Appleton-Century-Crofts.

Catania, A. C., & Reynolds, G. S. (1968). A quantitative analysis of
the responding maintained by interval schedules of reinforcement.
*Journal of the Experimental Analysis of Behavior*, 11(3, Pt. 2),
327-383. https://doi.org/10.1901/jeab.1968.11-s327

## Mathematical definition

The first response emitted at or after `interval` time units since
construction (or since the previous reinforcer) is reinforced.
Responses during the interval do not reset the clock and are not
reinforced. Ticks (`event=None`) never reinforce — the schedule
requires a response.

Criterion (Swift original): `numOfResponses > previousNumOfResponses
AND elapsed >= interval`.

## State variables

| Name | Type | Purpose |
|---|---|---|
| `interval` | float (const) | Fixed interval length (`> 0`). |
| `arm_time` | float | Absolute time at which the next response can reinforce. Initialised to `interval`. |
| `last_now` | float? | Monotonic-time check. |

## Step pseudocode

```
fn step(now, event):
    check_monotonic(now, last_now)
    check_event_time(now, event)
    last_now = now
    if event is None:
        return Outcome::unreinforced()
    if now + TIME_TOL >= arm_time:
        arm_time = now + interval
        return Outcome::reinforced(Reinforcer { time: now })
    return Outcome::unreinforced()
```

## Reset semantics

- `arm_time = interval`
- `last_now = None`

## Edge cases

- `interval <= 0` raises `Config`.
- The initial `arm_time = interval` means the first interval is
  anchored at `t = 0`, **not** at the first step. A caller whose first
  step arrives at `now = 0` behaves identically to one whose first
  step arrives at `now = interval / 2`, provided the caller never
  presents a response before `interval`.
- Boundary equality: a response with `now == arm_time` reinforces
  (inclusive comparison with `TIME_TOL` slack).

## LimitedHold hook

- `_arm_time` (read) — the currently armed expiry time.
- `_withdraw_and_rearm(now)` — sets `arm_time = now + interval`
  without emitting a reinforcer. Used by `LimitedHold` on window
  expiry.

## Determinism

Deterministic. No RNG.

## Divergence from Swift original

Swift computed elapsed time from a `ResponseEntity.milliseconds`
field. This port owns an absolute `arm_time` anchor so the elapsed
calculation is the subtraction `now - last_reinforcement`. The `>`
vs `>=` condition is preserved via the `TIME_TOL`-padded inclusive
comparison.

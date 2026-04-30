# FT — Fixed Time

:jp: [日本語版](../../ja/algorithms/ft.md)

## References

Zeiler, M. D. (1968). Fixed and variable schedules of
response-independent reinforcement. *Journal of the Experimental
Analysis of Behavior*, 11(4), 405-414.
https://doi.org/10.1901/jeab.1968.11-405

Catania, A. C., & Reynolds, G. S. (1968). A quantitative analysis of
the responding maintained by interval schedules of reinforcement.
*JEAB*, 11(3 Pt 2), 327-383. https://doi.org/10.1901/jeab.1968.11-s327

## Mathematical definition

A reinforcer is delivered every `interval` time units, independent of
the subject's behavior. Responses are ignored for the reinforcement
decision (but their timestamps are still validated).

Fires as soon as `now - anchor >= interval`, where `anchor` is the
schedule's internal reference time.

## State variables

| Name | Type | Purpose |
|---|---|---|
| `interval` | float (const) | Fixed inter-reinforcer interval. |
| `anchor` | float? | Reference time for elapsed calculation. `None` before the first step. |
| `last_now` | float? | Monotonic-time check. |

## Step pseudocode

```
fn step(now, event):
    check_monotonic(last_now, now)
    check_event_time(now, event)
    last_now = now
    if anchor is None:          // first-step anchoring
        anchor = now
        return Outcome::unreinforced()
    if now - anchor + TIME_TOL >= interval:
        anchor = now
        return Outcome::reinforced(Reinforcer { time: now })
    return Outcome::unreinforced()
```

## Reset semantics

- `anchor = None`
- `last_now = None`

## Edge cases

- `interval <= 0` raises `Config`.
- **First-step anchoring.** The first call to `step()` anchors the
  clock and returns unreinforced. The first reinforcer fires on the
  next step for which `now - anchor >= interval`. This means a caller
  whose first step is at `now = 0` sees the first reinforcer at
  `now >= interval`; a caller whose first step is at `now = 5` sees
  the first reinforcer at `now >= 5 + interval`.
- **Single-fire per step.** If the caller steps late by `k > 1`
  intervals since the previous fire, **only one** reinforcer is
  emitted and `anchor = now`. The next reinforcer therefore requires a
  full additional `interval`. FT does not queue missed reinforcers.

## Determinism

Deterministic. No RNG.

## Divergence from Swift original

Swift treated FT as a pure predicate (`milliseconds >= value`) with
external state tracking by the Rx pipeline. This port owns the anchor
and explicitly defines first-step anchoring semantics to avoid any
ambiguity about when the clock starts.

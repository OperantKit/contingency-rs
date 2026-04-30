# RI — Random Interval

:jp: [日本語版](../../ja/algorithms/ri.md)

## References

Catania, A. C., & Reynolds, G. S. (1968). A quantitative analysis of
the responding maintained by interval schedules of reinforcement.
*Journal of the Experimental Analysis of Behavior*, 11(3, Pt. 2),
327-383. https://doi.org/10.1901/jeab.1968.11-s327

## Mathematical definition

Each inter-reinforcer interval is drawn independently from an
exponential distribution `Exp(1 / mean_interval)`. The process is
memoryless: at any moment the hazard of the next reinforcer becoming
available in `[t, t+dt)` is `dt / mean_interval` independent of
elapsed time. As with FI/VI, a response is required to collect the
reinforcer.

## State variables

| Name | Type | Purpose |
|---|---|---|
| `mean_interval` | float (const) | Mean of the exponential. |
| `rng` | Random | Source of `expovariate` draws. |
| `initial_state` | Random.State | Snapshot at construction. |
| `arm_time` | float | Absolute time of next reinforcement availability. Initialised to `rng.expovariate(1/mean)`. |
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
        arm_time = now + rng.expovariate(1 / mean_interval)
        return Outcome::reinforced(Reinforcer { time: now })
    return Outcome::unreinforced()
```

## Reset semantics

- `rng.setstate(initial_state)`
- `arm_time = rng.expovariate(1 / mean_interval)` (new initial draw)
- `last_now = None`

Note: after `reset()`, the very first draw comes from the restored
RNG state, producing the same initial `arm_time` as after construction.

## Edge cases

- `mean_interval <= 0` raises `Config`.
- Like RI's Swift original (which is a no-op wrapper over FI in the
  legacy source), the present implementation adds exponential sampling
  on top of the interval mechanics.

## LimitedHold hook

- `_arm_time`, `_withdraw_and_rearm(now)` (draws a fresh exponential
  interval).

## Determinism

Same PRNG caveats as RR. Conformance fixtures for RI should pin the
draw sequence by pre-recording each `arm_time` in the fixture.

## Divergence from Swift original

The Swift file `RI.swift` is a thin delegate to `FI`; the actual
exponential sampling was supplied externally by the reactive pipeline.
This port makes the exponential sampling explicit and internal.

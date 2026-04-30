# RT — Random Time

:jp: [日本語版](../../ja/algorithms/rt.md)

## Reference

Catania, A. C., & Reynolds, G. S. (1968). A quantitative analysis of
the responding maintained by interval schedules of reinforcement.
*JEAB*, 11(3 Pt 2), 327-383. https://doi.org/10.1901/jeab.1968.11-s327

## Mathematical definition

Response-independent reinforcement (like FT) with inter-reinforcer
intervals drawn from `Exp(1 / mean_interval)`. Memoryless: constant
hazard `1 / mean_interval` regardless of elapsed time.

## State variables

| Name | Type | Purpose |
|---|---|---|
| `mean` | float (const) | Mean of the exponential. |
| `rng` | Random | Source of `expovariate`. |
| `initial_state` | Random.State | Snapshot at construction. |
| `requirement` | float | Current interval length (drawn at construction, re-drawn after each fire). |
| `anchor` | float? | Reference time; `None` before first step. |
| `last_now` | float? | Monotonic-time check. |

## Step pseudocode

```
fn step(now, event):
    check_monotonic(last_now, now)
    check_event_time(now, event)
    last_now = now
    if anchor is None:
        anchor = now
        return Outcome::unreinforced()
    if now - anchor + TIME_TOL >= requirement:
        anchor = now
        requirement = rng.expovariate(1 / mean)
        return Outcome::reinforced(Reinforcer { time: now })
    return Outcome::unreinforced()
```

## Reset semantics

- `rng.setstate(initial_state)`
- `requirement = rng.expovariate(1 / mean)` (fresh initial draw)
- `anchor = None`
- `last_now = None`

## Edge cases

- `mean <= 0` raises `Config`.
- First-step anchoring and single-fire-per-step semantics as FT.
- Because the first `requirement` is drawn at construction before any
  step occurs, the initial draw is taken with the construction-time
  RNG state. After `reset()` the draw is reproduced exactly.

## Determinism

Conformance fixtures pin the successive `requirement` values.

## Divergence from Swift original

Swift RT delegated to FT with externally supplied intervals. Same
structural divergence as RI.

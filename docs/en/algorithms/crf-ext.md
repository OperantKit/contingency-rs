# CRF & EXT — Limiting-case schedules

:jp: [日本語版](../../ja/algorithms/crf-ext.md)

## Reference

Skinner, B. F. (1957). *Schedules of reinforcement* (with C. B.
Ferster). Appleton-Century-Crofts.

## CRF — Continuous Reinforcement

Every response is reinforced. CRF is the limiting case of FR with
ratio 1.

### Definition

```python
def CRF() -> FR:
    return FR(1)
```

A factory, not a separate class. Rust port should provide
`Crf::new() -> Fr { Fr::new(1) }`.

## EXT — Extinction

No response is ever reinforced. `step()` always returns an
unreinforced `Outcome`, regardless of `now` or `event`.

### State variables

| Name | Type | Purpose |
|---|---|---|
| `last_now` | float? | Monotonic-time check. |

No other state.

### Step pseudocode

```
fn step(now, event):
    check_monotonic(last_now, now)
    check_event_time(now, event)
    last_now = now
    return Outcome::unreinforced()
```

### Reset semantics

- `last_now = None`

### Determinism

Fully deterministic. No RNG.

### Divergence from Swift original

Identical semantics. Swift implementation returned `false` from a pure
predicate; this port returns an unreinforced `Outcome` and still
enforces the monotonic-time contract of the `Schedule` protocol.

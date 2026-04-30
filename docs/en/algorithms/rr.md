# RR — Random Ratio

:jp: [日本語版](../../ja/algorithms/rr.md)

## Reference

Ferster, C. B., & Skinner, B. F. (1957). *Schedules of reinforcement*.
Appleton-Century-Crofts.

## Mathematical definition

Each response is reinforced independently with probability
`p ∈ (0, 1]`. Bernoulli draws are statistically independent and
memoryless with respect to response count.

## State variables

| Name | Type | Purpose |
|---|---|---|
| `p` | float (const) | Reinforcement probability. |
| `rng` | Random | Source of Bernoulli draws. |
| `initial_state` | Random.State | RNG snapshot at construction for `reset()`. |
| `last_now` | float? | Monotonic-time check. |

## Step pseudocode

```
fn step(now, event):
    check_monotonic(last_now, now)
    check_event_time(now, event)
    last_now = now
    if event is None:
        return Outcome::unreinforced()
    if rng.random() < p:
        return Outcome::reinforced(Reinforcer { time: now })
    return Outcome::unreinforced()
```

## Reset semantics

- `rng.setstate(initial_state)` — restore the Bernoulli stream.
- `last_now = None`

The initial-state snapshot is taken at construction, not on every
`reset()`, so a caller who shares a `Random` across schedules sees the
schedule return to **its** initial draw even if the shared RNG was
advanced elsewhere.

## Edge cases

- `p <= 0` or `p > 1` raises `Config`.
- `p == 1.0` is equivalent to CRF / FR(1).
- `rng=None` creates a fresh unseeded `Random()` internally; a
  deterministic conformance fixture therefore requires supplying a
  seeded `Random` at construction.

## Determinism

Deterministic when `rng` is supplied and seeded. Same PRNG caveat as
VR for cross-language bit identity; for conformance fixtures pin the
per-step reinforcement decisions by writing the boolean outcomes
directly into the JSON `expect` block.

## Divergence from Swift original

Swift's RR delegated to FR with a runtime-sampled value. This port is
a direct Bernoulli process; the two are statistically equivalent if
the Swift ratio-sampler is interpreted as a geometric draw with
`p = 1 / mean`.

# FR — Fixed Ratio

:jp: [日本語版](../../ja/algorithms/fr.md)

## Reference

Ferster, C. B., & Skinner, B. F. (1957). *Schedules of reinforcement*.
Appleton-Century-Crofts.

## Mathematical definition

Given ratio requirement `n >= 1`, reinforcement is delivered on the
`k`-th response whenever `k mod n == 0` (i.e. every `n`-th response).

## State variables

| Name | Type | Purpose |
|---|---|---|
| `n` | int (const) | Ratio requirement (`>= 1`). |
| `count` | int | Responses accumulated since last reinforcer. |
| `last_now` | float? | Last observed `now`, for monotonic check. |

## Step pseudocode

```
fn step(now, event):
    check_monotonic(last_now, now)
    check_event_time(now, event)
    last_now = now
    if event is None:
        return Outcome::unreinforced()
    count += 1
    if count >= n:
        count = 0
        return Outcome::reinforced(Reinforcer { time: now })
    return Outcome::unreinforced()
```

## Reset semantics

- `count = 0`
- `last_now = None`

## Edge cases

- `n = 1` is equivalent to CRF; every response reinforces.
- `n < 1` raises `Config` error.
- `n` must be an `int` (booleans rejected).
- A `None` event never advances `count`.

## Determinism

Deterministic. No RNG.

## Divergence from Swift original

Swift implementation was a pure pure-function predicate
(`numOfResponses >= value`) driven by Rx reactive streams; this port
owns the counter state and resets it on reinforcement. Semantics
match when the reactive chain's accumulator is interpreted as
per-reinforcement-cycle counts.

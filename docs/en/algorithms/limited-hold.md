# LimitedHold — bounded-availability wrapper

:jp: [日本語版](../../ja/algorithms/limited-hold.md)

## References

Ferster, C. B., & Skinner, B. F. (1957). *Schedules of reinforcement*.
Appleton-Century-Crofts. (Chapter 5)

Nevin, J. A. (1974). Response strength in multiple schedules.
*Journal of the Experimental Analysis of Behavior*, 21(3), 389-408.
https://doi.org/10.1901/jeab.1974.21-389

## Mathematical definition

A decorator around an interval-family schedule (FI, VI, RI) that adds
a bounded availability window of length `hold` to each reinforcement
opportunity. If the subject fails to respond within
`[arm_time, arm_time + hold]`, the opportunity is withdrawn:
the inner schedule is re-armed at `now` with a fresh interval drawn
(without a reinforcer being delivered).

Required hooks on the inner schedule (duck-typed):

- `_arm_time: float` — absolute time at which the current
  opportunity becomes available.
- `_withdraw_and_rearm(now: float)` — draws a fresh interval and
  anchors it at `now` without emitting a reinforcer.

## State variables

| Name | Type | Purpose |
|---|---|---|
| `inner` | Schedule | The wrapped interval schedule. |
| `hold` | float (const) | Availability window (`> 0`). |
| `last_now` | float? | Monotonic-time check on the wrapper. |

All other state lives in `inner`.

## Step pseudocode

```
fn step(now, event):
    check_monotonic(last_now, now)
    check_event_time(now, event)
    last_now = now

    // Expire-if-needed: withdraw the previously-armed opportunity if
    // the hold window has closed.
    if now > inner._arm_time + hold + TIME_TOL:
        inner._withdraw_and_rearm(now)

    if event is None:
        return Outcome::unreinforced()

    arm_time = inner._arm_time
    if now + TIME_TOL >= arm_time and now <= arm_time + hold + TIME_TOL:
        // Delegate to inner so it advances its sequence/cursor/rng.
        return inner.step(now, event)

    return Outcome::unreinforced()
```

## Reset semantics

- `inner.reset()`
- `last_now = None`

## Edge cases

- `hold <= 0` raises `Config`.
- Duck-type check: `Config` raised if `inner` lacks `_arm_time` or
  `_withdraw_and_rearm`.
- Expiry is checked on **every** step (tick or response), before any
  event is dispatched. If the wrapper is stepped after a long gap, the
  gap correctly closes multiple stale windows — but note that since
  `_withdraw_and_rearm` anchors the next window at `now`, only the
  most recent window is actually regenerated, not each of the missed
  windows. The caller is expected to step often enough for this to be
  consistent with the experimenter's intent.
- Boundary equality: both `now >= arm_time` and
  `now <= arm_time + hold` are inclusive comparisons with `TIME_TOL`.
  A response at exactly `now == arm_time + hold` still reinforces.
- The wrapper does not observe the inner's reinforcement decision; it
  merely gates whether the event reaches the inner.

## Determinism

Inherits the inner's determinism. The wrapper adds no RNG of its own.

## Divergence from Swift original

No LimitedHold in the Swift source. This is a new construct
introduced by `contingency-py` to model the bounded-availability
semantics described in Ferster & Skinner (1957) Chapter 5.

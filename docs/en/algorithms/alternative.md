# Alternative — whichever-first binary compound

:jp: [日本語版](../../ja/algorithms/alternative.md)

## Reference

Ferster, C. B., & Skinner, B. F. (1957). *Schedules of reinforcement*.
Appleton-Century-Crofts.

## Mathematical definition

A binary compound of two component schedules. On every step, the
same `(now, event)` is forwarded to **both** components. If either
returns a reinforced outcome, both components are reset and the
reinforced outcome is surfaced with
`meta["alternative_winner"] ∈ {"first", "second"}`.

If both would reinforce on the same step, **first wins**; both are
still reset.

## State variables

| Name | Type |
|---|---|
| `first` | Schedule |
| `second` | Schedule |
| `last_now` | float? |

## Step pseudocode

```
fn step(now, event):
    // monotonic + event validation
    first_outcome = first.step(now, event)
    second_outcome = second.step(now, event)

    if first_outcome.reinforced:
        first.reset()
        second.reset()
        return Outcome::reinforced_with_meta(
            first_outcome.reinforcer,
            {"alternative_winner": "first"})
    if second_outcome.reinforced:
        first.reset()
        second.reset()
        return Outcome::reinforced_with_meta(
            second_outcome.reinforcer,
            {"alternative_winner": "second"})
    return Outcome::unreinforced()
```

## Reset semantics

- `first.reset()`, `second.reset()`, `last_now = None`.

## Edge cases

- Strictly binary. Use left-associative nesting for 3+ components:
  `Alternative(Alternative(a, b), c)` (mirrors the DSL bridge's
  behaviour).
- Both components are stepped on every call regardless of which (if
  any) will win — this is essential so that a response-based and a
  time-based component can race correctly.
- After a win, the components are reset and the clocks restart — the
  next cycle is fresh.

## Determinism

Inherits component determinism.

## Divergence from Swift original

No Alternative in the legacy Swift package.

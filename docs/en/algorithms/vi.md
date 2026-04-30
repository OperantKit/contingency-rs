# VI — Variable Interval

:jp: [日本語版](../../ja/algorithms/vi.md)

## References

Ferster, C. B., & Skinner, B. F. (1957). *Schedules of reinforcement*.
Appleton-Century-Crofts.

Fleshler, M., & Hoffman, H. S. (1962). A progression for generating
variable-interval schedules. *Journal of the Experimental Analysis of
Behavior*, 5(4), 529-530. https://doi.org/10.1901/jeab.1962.5-529

## Mathematical definition

Intervals between successive reinforcers are drawn from a shuffled
Fleshler-Hoffman progression with arithmetic mean `mean_interval`.
Like FI, a response is required to collect the reinforcer — a tick
alone does not suffice. When the pool is exhausted, a fresh pool is
generated using a sub-seed drawn from the master RNG.

## State variables

| Name | Type | Purpose |
|---|---|---|
| `mean_interval` | float (const) | Target arithmetic mean. |
| `n_intervals` | int (const) | Pool size per cycle (default 12). |
| `seed` | int? (const) | Master seed. |
| `rng` | Random | Draws sub-seeds (`rng.getrandbits(64)`). |
| `sequence` | list[float] | Current pool of interval lengths. |
| `cursor` | int | Next-index into `sequence` (starts at 1; index 0 is consumed by `arm_time`). |
| `arm_time` | float | Next reinforcement target (initialised to `sequence[0]`). |
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
        arm_time = now + next_interval()
        return Outcome::reinforced(Reinforcer { time: now })
    return Outcome::unreinforced()

fn next_interval() -> float:
    if cursor >= len(sequence):
        sequence = generate_sequence()   // fresh pool using rng.getrandbits(64)
        cursor = 0
    value = sequence[cursor]
    cursor += 1
    return value
```

## Reset semantics

- `rng = Random(seed)`
- Regenerate sequence deterministically
- `cursor = 1`
- `arm_time = sequence[0]`
- `last_now = None`

## Edge cases

- `mean_interval <= 0` raises `Config`.
- `n_intervals < 1` raises `Config`.
- Sub-seed domain: `rng.getrandbits(64)` — 64 random bits, passed as
  the seed to the Fleshler-Hoffman generator.

## LimitedHold hook

- `_arm_time` (read)
- `_withdraw_and_rearm(now)` — `arm_time = now + next_interval()`

## Determinism

Same reasoning as VR: deterministic within Python; cross-language
bit identity is tracked by pinning the **sequence** in the
conformance fixture rather than requiring Rust to reproduce Python's
Mersenne Twister.

## Divergence from Swift original

Swift's VI delegated to FI with the next value supplied by the
`FleshlerHoffman` generator. This port binds the generator to the VI
schedule and makes sequence regeneration deterministic by deriving
sub-seeds from the master RNG.

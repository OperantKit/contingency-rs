# VR — Variable Ratio

:jp: [日本語版](../../ja/algorithms/vr.md)

## References

Ferster, C. B., & Skinner, B. F. (1957). *Schedules of reinforcement*.
Appleton-Century-Crofts.

Fleshler, M., & Hoffman, H. S. (1962). A progression for generating
variable-interval schedules. *Journal of the Experimental Analysis of
Behavior*, 5(4), 529-530. https://doi.org/10.1901/jeab.1962.5-529

## Mathematical definition

Ratio requirements are drawn from a Fleshler-Hoffman progression with
arithmetic mean `mean`. On each reinforcement, the next ratio
requirement in the sequence becomes the active requirement. When the
sequence is exhausted, a fresh sequence is generated using a sub-seed
derived from the master RNG.

See `fleshler-hoffman.md` for the generator definition.

## State variables

| Name | Type | Purpose |
|---|---|---|
| `mean` | float (const) | Target arithmetic mean ratio. |
| `n_intervals` | int (const) | Pool size per cycle (default 12). |
| `seed` | int? (const) | Master seed for determinism. |
| `rng` | Random | Master RNG, derives per-cycle sub-seeds. |
| `sequence` | list[int] | Current pool of ratio requirements. |
| `cursor` | int | Index into `sequence`. |
| `count` | int | Responses accumulated toward current requirement. |
| `requirement` | int | Current ratio requirement (`sequence[cursor]`). |
| `last_now` | float? | Monotonic-time check. |

## Step pseudocode

```
fn step(now, event):
    check_monotonic(last_now, now)
    check_event_time(now, event)
    last_now = now
    if event is None:
        return Outcome::unreinforced()
    count += 1
    if count >= requirement:
        count = 0
        advance_requirement()
        return Outcome::reinforced(Reinforcer { time: now })
    return Outcome::unreinforced()

fn advance_requirement():
    cursor += 1
    if cursor >= len(sequence):
        reload_sequence()
    else:
        requirement = sequence[cursor]

fn reload_sequence():
    sub_seed = rng.randrange(0, 2**31 - 1)
    sequence = fleshler_hoffman::generate_ratios(mean, n_intervals, seed=sub_seed)
    cursor = 0
    requirement = sequence[0]
```

## Reset semantics

- `rng = Random(seed)` (re-seed from stored master seed)
- `count = 0`
- `last_now = None`
- Regenerate sequence: `reload_sequence()` — this yields a
  bit-identical trajectory when `seed` is set.

## Edge cases

- `mean <= 0` raises `Config`.
- `n_intervals < 1` raises `Config`.
- Sub-seed domain is `[0, 2**31 - 1)`; Rust must use the same range to
  keep bit-identical output with Python's `Random`.

## Determinism

Fully deterministic when `seed` is supplied. The cross-language
bit-identity requirement only holds if the Rust port reproduces
`random.Random(seed).randrange(0, 2**31 - 1)` and
`random.Random(sub_seed).shuffle(...)` — Python's Mersenne Twister.
For `contingency-rs`, the recommended approach is to treat the
seeded random stream as an opaque **fixture input**: the conformance
corpus pins the VR trajectory by seed, but Rust implementations may
use a different PRNG and instead validate by replaying a fixture whose
`sequence` field is read from JSON.

(See the `conformance/` corpus: stochastic fixtures include the raw
`sequence` pool where applicable so Rust can consume it directly.)

## Divergence from Swift original

Swift's VR delegated to FR with a variable value supplied by the
reactive stream; the Fleshler-Hoffman generator produced the pool
externally. This port binds the generator into the VR class so the
pool is regenerated deterministically on sequence exhaustion.

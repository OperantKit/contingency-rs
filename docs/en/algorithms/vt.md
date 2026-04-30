# VT — Variable Time

:jp: [日本語版](../../ja/algorithms/vt.md)

## References

Zeiler, M. D. (1968). Fixed and variable schedules of
response-independent reinforcement. *JEAB*, 11(4), 405-414.
https://doi.org/10.1901/jeab.1968.11-405

Fleshler, M., & Hoffman, H. S. (1962). A progression for generating
variable-interval schedules. *JEAB*, 5(4), 529-530.
https://doi.org/10.1901/jeab.1962.5-529

## Mathematical definition

Response-independent reinforcement (like FT) but with inter-reinforcer
intervals drawn from a Fleshler-Hoffman progression of mean
`mean_interval`. When the pool is exhausted a fresh pool is regenerated
deterministically.

## State variables

Same as VR, with `anchor` replacing the response counter:

| Name | Type | Purpose |
|---|---|---|
| `mean` | float (const) | Target arithmetic mean. |
| `n_intervals` | int (const) | Pool size per cycle (default 12). |
| `seed` | int? (const) | Master seed. |
| `rng` | Random | Master RNG for sub-seed generation. |
| `sequence` | list[float] | Current pool of interval lengths. |
| `cursor` | int | Index into pool. |
| `requirement` | float | Current interval length (`sequence[cursor]`). |
| `anchor` | float? | Reference for elapsed time; `None` before first step. |
| `last_now` | float? | Monotonic-time check. |

Sub-seed domain: `rng.randrange(0, 2**31 - 1)` (same as VR).

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
        advance_requirement()
        return Outcome::reinforced(Reinforcer { time: now })
    return Outcome::unreinforced()
```

## Reset semantics

- `rng = Random(seed)`
- Regenerate sequence deterministically (same trajectory as after
  construction)
- `anchor = None`
- `last_now = None`

## Edge cases

- Same as VR plus FT's first-step anchoring and single-fire-per-step.

## Determinism

Same PRNG caveat as VR/VI; cross-language fixtures pin the sequence
directly.

## Divergence from Swift original

Swift's VT delegated to FT with the next interval supplied by a
separate Fleshler-Hoffman generator. This port binds the generator
internally.

# Progressive Ratio — step-function parameterised schedule

:jp: [日本語版](../../ja/algorithms/progressive-ratio.md)

## References

Hodos, W. (1961). Progressive ratio as a measure of reward strength.
*Science*, 134(3483), 943-944.
https://doi.org/10.1126/science.134.3483.943

Hursh, S. R. (1980). Economic concepts for the analysis of behavior.
*JEAB*, 34(2), 219-238. https://doi.org/10.1901/jeab.1980.34-219

Richardson, N. R., & Roberts, D. C. S. (1996). Progressive ratio
schedules in drug self-administration studies in rats: A method to
evaluate reinforcing efficacy. *Journal of Neuroscience Methods*,
66(1), 1-11. https://doi.org/10.1016/0165-0270(95)00153-0

## Mathematical definition

Each successive reinforcer requires more responses than the previous.
The rule that maps a reinforcement index `n` (0-based) to the
required response count is the **step function**:

```
r_n = step_fn(n)  where r_n >= 1 (positive integer)
```

`ProgressiveRatio` wraps a `step_fn` and never terminates by itself
(no breakpoint detection — that lives in the session runner).

## Step-function families

### Arithmetic (`arithmetic(start, step)`)

`r_n = start + n * step`. Constraints: `start >= 1`, `step >= 1`.

### Geometric (`geometric(start, ratio)`)

`r_n = max(1, round(start * ratio ** n))`.
Constraints: `start >= 1`, `ratio > 1.0`.

### Richardson-Roberts (`richardson_roberts()`)

Hardcoded 30-element series (indices 0..29):

```
1, 2, 4, 6, 9, 12, 16, 20, 25, 32, 40, 50, 62, 77, 95, 118,
145, 178, 219, 268, 328, 402, 492, 603, 737, 901, 1102, 1347,
1647, 2012
```

Beyond index 29, the series extrapolates geometrically using the
ratio between the last two values (`2012 / 1647 ≈ 1.2217`):

```
r_n = max(1, round(2012 * (2012/1647) ** (n - 29)))  for n >= 30
```

## State variables

| Name | Type | Purpose |
|---|---|---|
| `step_fn` | callable `(int) -> int` | Step function. |
| `index` | int | 0-based index of the **next** reinforcer to be earned. |
| `count` | int | Responses accumulated toward `step_fn(index)`. |
| `last_now` | float? | Monotonic-time check. |

## Step pseudocode

```
fn step(now, event):
    // monotonic + event validation
    last_now = now
    if event is None:
        return Outcome::unreinforced()
    count += 1
    requirement = resolve_requirement(index)
    if count >= requirement:
        count = 0
        index += 1
        return Outcome::reinforced(Reinforcer { time: now })
    return Outcome::unreinforced()

fn resolve_requirement(i):
    v = step_fn(i)
    if v is not int or v < 1:
        raise Config(...)
    return v
```

## Reset semantics

- `index = 0`, `count = 0`, `last_now = None`.
- Step function is retained (it is part of the configuration).

## Edge cases

- `step_fn` must be callable (lazy validation).
- Invalid return values (non-int, `< 1`) raise `Config` at the
  **first response that consults `step_fn(index)`**, not at
  construction. Rust port should mirror this laziness.
- No breakpoint termination — the schedule continues forever.

## Determinism

Deterministic (step functions are pure).

## Divergence from Swift original

No PR in the legacy Swift package. The Richardson-Roberts and
arithmetic/geometric families are all new in `contingency-py`.

# Fleshler-Hoffman — VI / VR pool generator

:jp: [日本語版](../../ja/algorithms/fleshler-hoffman.md)

## References

Fleshler, M., & Hoffman, H. S. (1962). A progression for generating
variable-interval schedules. *Journal of the Experimental Analysis of
Behavior*, 5(4), 529-530. https://doi.org/10.1901/jeab.1962.5-529

Hantula, D. A. (1991). A simple BASIC program to generate values for
variable-interval schedules of reinforcement. *Journal of Applied
Behavior Analysis*, 24(4), 799-801.
https://doi.org/10.1901/jaba.1991.24-799

## Purpose

Generates a pool of `n` interval (or ratio) values whose arithmetic
mean equals a target `v`. The pool is shuffled deterministically
when a seed is supplied. Used internally by VI, VR, VT.

## Algorithm (raw progression)

For `m ∈ 1..n`:

- If `m == n`:

  ```
  vi[m] = v * (1 + log(n))
  ```

- Else:

  ```
  s1 = (1 + log(n)) + (n - m) * log(n - m)
  s2 = (n - m + 1) * log(n - m + 1)
  vi[m] = v * (s1 - s2)
  ```

This produces a list of `n` real values whose arithmetic mean is
approximately `v`.

## Public functions

### `generate_intervals(v, n=12, seed=None) -> list[float]`

1. Compute the raw progression `vi[1..n]`.
2. Mean-preservation: adjust `vi[0] += v * n - sum(vi)` so that the
   shuffled pool's mean is exactly `v` within float rounding. Python
   uses `math.fsum` for the sum to minimise float error.
3. Shuffle in-place using `random.Random(seed)`.
4. Return the shuffled list.

Empty pool when `n == 0`.

### `generate_ratios(v, n=12, seed=None) -> list[int]`

1. Compute raw progression.
2. Round each value to a positive integer: `max(1, round(vi[m]))`.
3. Correct the tail:
   - Let `head_sum = sum(rd[:-1])` and `target_total = round(v * n)`.
   - `surplus = target_total - head_sum`.
   - If `surplus >= 1`: `rd[-1] = surplus`.
   - Else: set `rd[-1] = 1`, `surplus -= 1`, then walk from tail
     decrementing any `rd[i] >= 2` and incrementing `surplus` until
     `surplus >= 0`. This matches the Swift fallback.
4. Shuffle with `random.Random(seed)`.
5. Return integers.

### `generate_intervals_hantula1991(v, n=12, seed=None) -> list[int]`

Reproduces Hantula's (1991) BASIC-program variant: random placement
into a zero-initialised slot via retry loop (Hantula's GOTO 130).
Same mean as `generate_intervals` but integer-valued and with
Hantula's specific placement strategy. Rarely used; included for
literature parity.

## Determinism

Deterministic when `seed` is supplied. Cross-language caveat: Python's
`random.Random` is Mersenne Twister. Conformance fixtures pin the
generated pool directly so Rust can consume it without reproducing
MT bit-exact.

## Edge cases

- `v` may be `int` or `float`; internally coerced to `float`.
- `n == 0` → empty list.
- For `generate_ratios`, the "surplus fallback" path is triggered
  when the raw progression rounds up enough that the head already
  sums to more than the target — the loop walks the tail decrementing
  values `>= 2` until balance is restored. This matches the Swift
  source's fallback path.

## Divergence from Swift original

Algorithmically identical. The Python port:

1. Uses `math.fsum` instead of naive summation to reduce float drift.
2. Drops the Hantula-style retry loop for the primary generator
   (it produces the same distribution via `random.shuffle`).
3. Keeps `generate_intervals_hantula1991` available for exact
   literature reproduction.

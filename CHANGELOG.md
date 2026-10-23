# Changelog

## Unreleased

### Changed

- Seeded schedules (VR, VI, VT, RR, RI, RT, MatchingToSample, GoNoGo) and the Fleshler-Hoffman generators now draw from `helpers::py_random::PyRandom`, a port of CPython's `random.Random`, instead of `rand::rngs::SmallRng`. A seeded sequence is now bit-equivalent to `contingency-py` with the same seed. **The sequence produced by an existing seed changes**; runs recorded with an earlier build cannot be replayed from the seed alone.
- `generate_intervals` sums the progression with a correctly rounded `fsum` (as `math.fsum`), and `generate_ratios` / `generate_intervals_hantula1991` round half to even (as Python `round`), so the generators match the Python port value for value.
- `tests/conformance.rs` replays every fixture of the Python corpus bit-equivalently, including the 8 seeded fixtures that were previously `#[ignore]`d and checked only by reinforcer count.

### Removed

- The `rand` dependency. OS entropy for unseeded schedules comes from `getrandom`.

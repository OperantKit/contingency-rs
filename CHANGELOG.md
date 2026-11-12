# Changelog

## Unreleased

### Added

- `opk_from_spec` (C), `Schedule.fromSpec` (WebAssembly), and `UniffiSchedule::from_spec` (Swift/Kotlin) build any schedule from a schedule spec given as a JSON string, through `spec::from_spec`. The per-schedule constructors are unchanged.
- `spec::from_spec`: `Concurrent` takes the optional `cod_directional` (an array of `{"from", "to", "cod"}`; a repeated pair is a `Config` error) and `punish` (an object of operandum to schedule spec), built with `Concurrent::with_extensions`.
- Python: `contingency_core.from_spec` and `Schedule.from_spec` build any schedule from a spec given as a dict or a JSON string; `contingency_core.SPEC_TYPES` lists the spec types. `Outcome`, `Reinforcer`, and `ResponseEvent` can be constructed with the same arguments and defaults as in contingency-py (`Outcome` validates `reinforced` against `reinforcer` the same way) and compare by value. `pyproject.toml` lets `maturin develop` build the module as the `contingency-core` distribution.

### Changed

- Seeded schedules (VR, VI, VT, RR, RI, RT, MatchingToSample, GoNoGo) and the Fleshler-Hoffman generators now draw from `helpers::py_random::PyRandom`, a port of CPython's `random.Random`, instead of `rand::rngs::SmallRng`. A seeded sequence is now bit-equivalent to `contingency-py` with the same seed. **The sequence produced by an existing seed changes**; runs recorded with an earlier build cannot be replayed from the seed alone.
- `generate_intervals` sums the progression with a correctly rounded `fsum` (as `math.fsum`), and `generate_ratios` / `generate_intervals_hantula1991` round half to even (as Python `round`), so the generators match the Python port value for value.
- `tests/conformance.rs` replays every fixture of the Python corpus bit-equivalently, including the 8 seeded fixtures that were previously `#[ignore]`d and checked only by reinforcer count.
- `tests/conformance.rs` also compares `Outcome.meta` when a fixture step records it: every recorded key must be present with an equal value (numbers within 1e-9), the same rule as the Python conformance runner.
- `GoNoGo`: the outcome of the step whose response closes a trial now reports `meta.phase = "iti"` (the phase after the step), as contingency-py does, instead of the `"go"` / `"nogo"` phase before it.

### Removed

- The `rand` dependency. OS entropy for unseeded schedules comes from `getrandom`.

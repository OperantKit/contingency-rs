# contingency-rs

:jp: [日本語版 README](README.ja.md)

Rust reinforcement schedule engine. Port of [`contingency-py`](../contingency-py/). Executable specification: the Python package's 20 conformance fixtures under `contingency-py/conformance/`, replayed here.

## Scope

- Atomic schedules: FR, VR, RR, CRF, FI, VI, RI, FT, VT, RT, EXT
- LimitedHold wrapper
- Compound: Concurrent (+ COD + COR), Multiple, Chained, Tandem, Alternative
- Differential: DRO (Resetting / Momentary), DRL, DRH
- Progressive Ratio + step functions (arithmetic, geometric, Richardson-Roberts)
- Fleshler-Hoffman VI/VR generators (1962 + Hantula 1991)
- `contingency-hil` binary speaking the HAL JSONL wire protocol

## Build

```sh
cargo build --release
cargo test
```

### Feature flags

- `python` — build the PyO3 `contingency_core` extension module
- `uniffi` — build Swift / Kotlin / KMP scaffolding via UniFFI

## Semantic invariants (shared with Python port)

See `docs/correspondence.md` and the Python package's `docs/handoff-summary.md` for the full list. Notable:

- `TIME_TOL = 1e-9` applied uniformly to monotonic and event-time checks.
- First-step anchoring: FT/VT/RT/DRO anchor on the first `step()`; FI/VI/RI anchor at construction.
- `Concurrent` advances every component on every step; `Chained`/`Tandem` only step the active component.
- COD gates only the event-matched component (tick-side reinforcements on other operanda pass through).
- Momentary DRO uses a half-open window `[anchor, now)`.
- `RR`/`RI`/`RT` snapshot the RNG state at construction so `reset()` replays the same draw sequence.

## References

- Ferster, C. B., & Skinner, B. F. (1957). *Schedules of reinforcement*. Appleton-Century-Crofts.
- Fleshler, M., & Hoffman, H. S. (1962). A progression for generating variable-interval schedules. *JEAB*, 5(4), 529-530. https://doi.org/10.1901/jeab.1962.5-529
- Hantula, D. A. (1991). A simple BASIC program to generate values for variable-interval schedules of reinforcement. *JABA*, 24(4), 799-801.
- Catania, A. C. (1966). Concurrent operants. In W. K. Honig (Ed.), *Operant behavior* (pp. 213-270). Appleton-Century-Crofts.
- Reynolds, G. S. (1961). Behavioral contrast. *JEAB*, 4(1), 57-71.
- Hodos, W. (1961). Progressive ratio as a measure of reward strength. *Science*, 134, 943-944.
- Hursh, S. R. (1980). Economic concepts for the analysis of behavior. *JEAB*, 34(2), 219-238.

## License

MIT. See [LICENSE](LICENSE).

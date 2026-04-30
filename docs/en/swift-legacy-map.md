# Swift legacy source map

:jp: [日本語版](../ja/swift-legacy-map.md)

Reference table mapping each schedule in `contingency-rs` (and the
`contingency-py` mirror) to the original Swift source in the archived
upstream `OperantKit` Swift package (Mizutani, 2018-2020). Paths below
are relative to that Swift package's repository root.

| Schedule | Swift file | Lines | Notes |
|---|---|---|---|
| FR | `Sources/Common/Schedules/FR.swift` | 21-24 | Predicate `numOfResponses >= value`. |
| CRF | `Sources/Common/Schedules/CRF.swift` | 18-20 | Delegates to `fixedRatio(1)`. |
| VR | `Sources/Common/Schedules/VR.swift` | 16-26 | Delegates to `FR` with a runtime-sampled value. |
| RR | `Sources/Common/Schedules/RR.swift` | 16-26 | Delegates to `FR` (legacy treated RR structurally as FR). |
| FI | `Sources/Common/Schedules/FI.swift` | 14-17 | Predicate `numOfResponses > previous && fixedTime(value)`. |
| VI | `Sources/Common/Schedules/VI.swift` | 18-30 | Delegates to `FI`. |
| RI | `Sources/Common/Schedules/RI.swift` | 18-30 | Delegates to `FI`; exponential sampling was external. |
| FT | `Sources/Common/Schedules/FT.swift` | 15-17 | Predicate `milliseconds >= value`. |
| VT | `Sources/Common/Schedules/VT.swift` | 18-30 | Delegates to `FT`. |
| RT | `Sources/Common/Schedules/RT.swift` | 18-30 | Delegates to `FT`. |
| EXT | `Sources/Common/Schedules/EXT.swift` | 16-18 | Always returns `false`. |
| Fleshler-Hoffman | `Sources/Common/Helpers/FleshlerHoffman.swift` | 11-95 | `generatedInterval` (15-52) and `generatedRatio` (55-95); Hantula variant 100-129. |
| Concurrent | — | — | No legacy source; new in `contingency-py`. |
| Alternative | — | — | No legacy source. |
| Multiple / Chained / Tandem | — | — | No legacy source. |
| LimitedHold | — | — | No legacy source. |
| DRO / DRL / DRH | — | — | No legacy source. |
| ProgressiveRatio | — | — | No legacy source. |

## Semantic-equivalence notes

- The Swift package used a reactive (RxSwift) pipeline with a
  `ResponseEntity` carrying `(numOfResponses, milliseconds)`. Each
  schedule was a pure predicate over that entity; state was owned by
  the surrounding stream combinators.
- Both ports invert the state ownership: each schedule is a stateful
  object driven by explicit `step(now, event)` calls. The predicates
  are preserved (see `FR >= value`, `FI > previous && elapsed >= interval`,
  `FT elapsed >= value`) but the surrounding anchor / counter /
  sequence state is now internal.
- Random-family schedules (RR, RI, RT) in the Swift source delegated
  to their Fixed counterparts with externally supplied variable values.
  Both ports make the random sampling explicit and internal
  (`Random.random()` / `Bernoulli` for RR; `Random.expovariate` /
  `Exp` for RI and RT).
- The Fleshler-Hoffman Swift implementation worked in integer
  milliseconds. Both ports are unit-agnostic (`f64` / `float` on a
  caller-declared clock); Python uses `math.fsum` and Rust uses
  `iter().sum::<f64>()` with the same Kahan-equivalent ordering for
  numerical stability.

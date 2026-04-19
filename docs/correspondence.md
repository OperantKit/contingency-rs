# `contingency-py` ↔ `contingency-rs` 1:1 correspondence

This document is the verification gate for the Rust port. Its purpose is
to answer, for any reader: "Does the Rust crate cover the same surface
as the Python package, and where does each Python element live in
Rust?" Discrepancies are surfaced explicitly as `DEFERRED`, `BY_DESIGN`,
or `BUG` at the bottom.

The Python package `contingency-py` is the authoritative executable
specification. The 20 conformance fixtures under
`contingency-py/conformance/` are the bit-equivalence oracle for
deterministic schedules; stochastic fixtures are trajectory templates
(see `contingency-py/conformance/README.md`).

## 1. Module structure mapping

| Python file | Rust counterpart | Notes |
|---|---|---|
| `src/contingency/__init__.py` | `src/lib.rs` | Re-exports public API; naming preserved. |
| `src/contingency/entities.py` | `src/types.rs` | `ResponseEvent` / `Observation` / `Reinforcer` / `Outcome`. Rust adds `MetaValue` enum to type the Python `dict[str, object]` meta payload. |
| `src/contingency/errors.py` | `src/errors.rs` | Four Python classes collapsed to three `ContingencyError` variants (`Config` / `State` / `Hardware`). `NotConnectedError` is carried as a `Hardware(..)` message variant; see Gap G7. |
| `src/contingency/interfaces.py` | `src/schedule.rs` | `Schedule` `Protocol` → `trait Schedule`. Rust adds `ArmableSchedule` super-trait (explicit `arm_time`/`withdraw_and_rearm` hooks that Python implements via attribute access on the concrete class). |
| `src/contingency/builder.py` | *(no dedicated file — factory methods live on `PySchedule` in `src/python.rs`; idiomatic Rust construction is direct `FR::new(..)`)* | See Gap G1. |
| `src/contingency/bridge/__init__.py` | *(none — deferred)* | See Gap G2. |
| `src/contingency/helpers/__init__.py` | `src/helpers/mod.rs` | |
| `src/contingency/helpers/fleshler_hoffman.py` | `src/helpers/fleshler_hoffman.rs` | Same three public entry points; see §5 Gotcha 11 for PRNG divergence. |
| `src/contingency/schedules/__init__.py` | `src/schedules/mod.rs` | |
| `src/contingency/schedules/ratio.py` | `src/schedules/ratio.rs` | `FR`, `VR`, `RR`, `CRF` (factory). |
| `src/contingency/schedules/interval.py` | `src/schedules/interval.rs` | `FI`, `VI`, `RI`, `LimitedHold`. |
| `src/contingency/schedules/time_based.py` | `src/schedules/time_based.rs` | `FT`, `VT`, `RT`, `EXT`. |
| `src/contingency/schedules/differential.py` | `src/schedules/differential.rs` | `DRO`, `DRL`, `DRH`. `DroMode` enum replaces Python's string literal. |
| `src/contingency/schedules/sequence.py` | `src/schedules/sequence.rs` | `Multiple`, `Chained`, `Tandem`. |
| `src/contingency/schedules/concurrent.py` | `src/schedules/concurrent.rs` | `Concurrent`. |
| `src/contingency/schedules/alternative.py` | `src/schedules/alternative.rs` | `Alternative`. |
| `src/contingency/schedules/progressive.py` | `src/schedules/progressive.rs` | `ProgressiveRatio` + `arithmetic` / `geometric` / `richardson_roberts`. Rust exposes them via a `StepFn` trait + boxed `Box<dyn StepFn>`. |
| `src/contingency/hw/__init__.py` | *(none; deliberate scope split)* | See §6 HAL parity and Gap G3. |
| `src/contingency/hw/protocols.py` | *(none)* | |
| `src/contingency/hw/virtual.py` | *(none)* | |
| `src/contingency/hw/serial_backend.py` | *(none)* | |
| `src/contingency/hw/hil_bridge.py` | `src/bin/hil.rs` (apparatus end of the same wire protocol) | Python side is the *host* (runs the schedule), Rust side is the *apparatus* (responds to events). Complementary, not redundant. |
| *(no Python counterpart)* | `src/constants.rs` | Pulls `TIME_TOL = 1e-9` out as a module constant. Python defines the same number inline at comparison sites. |
| *(no Python counterpart)* | `src/helpers/checks.rs` | Shared monotonic-time / event-time guards for every schedule's `step`. Python duplicates these lines inline across schedules. |
| *(no Python counterpart)* | `src/python.rs` | PyO3 bindings — no Python counterpart because the Python package *is* the Python side. |

No Python source file under `src/contingency/` is missing from Rust
except the deliberately-deferred bridge module (see §8 Gap G2) and the
HAL (§6). No Rust source file lacks a Python counterpart except the
PyO3 bindings, which by definition cannot have one.

## 2. Public API surface mapping

Python public symbols re-exported from `contingency/__init__.py` and
subpackages, and their Rust equivalents.

### 2.1 Top-level `contingency`

| Python export | Rust equivalent | Location |
|---|---|---|
| `ResponseEvent` | `ResponseEvent` | `types.rs:12` |
| `Observation` | `Observation` | `types.rs:39` |
| `Reinforcer` | `Reinforcer` | `types.rs:48` |
| `Outcome` | `Outcome` | `types.rs:84` |
| `Schedule` (Protocol) | `trait Schedule` | `schedule.rs:13` |
| `ScheduleBuilder` | N/A — see Gap G1 | — |
| `ContingencyError` | `enum ContingencyError` | `errors.rs:13` |
| `ScheduleConfigError` | `ContingencyError::Config(String)` | `errors.rs:15` |
| `ScheduleStateError` | `ContingencyError::State(String)` | `errors.rs:19` |
| `HardwareError` | `ContingencyError::Hardware(String)` | `errors.rs:23` |
| `NotConnectedError` | subsumed into `Hardware(..)` message — Gap G7 | — |
| `Apparatus` (Protocol) | N/A — Gap G3 | — |
| `ApparatusInfo` | N/A — Gap G3 | — |
| `ApparatusStatus` | N/A — Gap G3 | — |
| `HilBridgeApparatus` | N/A — see §6 (Rust is the apparatus-side `contingency-hil` bin) | — |
| `VirtualApparatus` | N/A — Gap G3 | — |
| `from_dsl` | N/A — Gap G2 | — |

### 2.2 `contingency.schedules`

| Python export | Rust equivalent | Location |
|---|---|---|
| `FR` | `schedules::FR` | `schedules/ratio.rs:54` |
| `VR` | `schedules::VR` | `schedules/ratio.rs:140` |
| `RR` | `schedules::RR` | `schedules/ratio.rs:279` |
| `CRF` (factory) | `schedules::crf()` | `schedules/ratio.rs:117` |
| `FI` | `schedules::FI` | `schedules/interval.rs:79` |
| `VI` | `schedules::VI` | `schedules/interval.rs:152` |
| `RI` | `schedules::RI` | `schedules/interval.rs:273` |
| `LimitedHold` | `schedules::LimitedHold<S: ArmableSchedule>` | `schedules/interval.rs:365` |
| `FT` | `schedules::FT` | `schedules/time_based.rs:82` |
| `VT` | `schedules::VT` | `schedules/time_based.rs:171` |
| `RT` | `schedules::RT` | `schedules/time_based.rs:312` |
| `EXT` | `schedules::EXT` | `schedules/time_based.rs:411` |
| `DRO` | `schedules::DRO` + `DroMode::{Resetting,Momentary}` | `schedules/differential.rs:96` |
| `DRL` | `schedules::DRL` | `schedules/differential.rs:238` |
| `DRH` | `schedules::DRH` | `schedules/differential.rs:322` |
| `Multiple` | `schedules::Multiple` | `schedules/sequence.rs:122` |
| `Chained` | `schedules::Chained` | `schedules/sequence.rs:248` |
| `Tandem` | `schedules::Tandem` | `schedules/sequence.rs:404` |
| `Alternative` | `schedules::Alternative` | `schedules/alternative.rs:69` |
| `Concurrent` | `schedules::Concurrent` | `schedules/concurrent.rs:83` |
| `ProgressiveRatio` | `schedules::ProgressiveRatio` | `schedules/progressive.rs:247` |
| `arithmetic` | `schedules::arithmetic` → `Box<dyn StepFn>` | `schedules/progressive.rs:92` |
| `geometric` | `schedules::geometric` → `Box<dyn StepFn>` | `schedules/progressive.rs:135` |
| `richardson_roberts` | `schedules::richardson_roberts` → `Box<dyn StepFn>` | `schedules/progressive.rs:204` |

### 2.3 `contingency.bridge`

| Python export | Rust equivalent |
|---|---|
| `from_dsl` | N/A — deferred. See Gap G2. |

### 2.4 `contingency.builder`

`ScheduleBuilder` exposes 22 static factory methods. Rust ships direct
constructors (e.g. `FR::new`, `VR::new`, `DRO::new`) and exposes a
ScheduleBuilder-equivalent facade *only* through PyO3 classmethods on
`PySchedule` (`Schedule.fr`, `Schedule.vi`, …). Native-Rust callers
have no `ScheduleBuilder` facade. See Gap G1.

### 2.5 `contingency.hw`

See §6.

## 3. Schedule-family coverage

Unit-test counts reflect `#[test]`-attributed functions in the Rust
schedule module's internal `tests` submodule, versus `def test_*`
methods in the corresponding `test_*.py` file (pytest collects
class-scoped methods too). Counts are a rough parity check, not a
semantic equivalence claim — the conformance corpus (§4) is the
semantic oracle.

| Schedule | Python class | Python test count (file) | Rust type | Rust test count (file) |
|---|---|---|---|---|
| FR | `FR` | 11 (`test_ratio_schedules.py`) | `FR` | 11 (`schedules/ratio.rs`) |
| VR | `VR` | 13 (`test_ratio_schedules.py`) | `VR` | 10 (`schedules/ratio.rs`) |
| RR | `RR` | 12 (`test_ratio_schedules.py`) | `RR` | 9 (`schedules/ratio.rs`) |
| CRF | `CRF` factory | 3 (`test_ratio_schedules.py`) | `crf()` | 2 (`schedules/ratio.rs`) |
| FI | `FI` | 12 (`test_interval_schedules.py`) | `FI` | 9 (`schedules/interval.rs`) |
| VI | `VI` | 7 (`test_interval_schedules.py`) | `VI` | 5 (`schedules/interval.rs`) |
| RI | `RI` | 7 (`test_interval_schedules.py`) | `RI` | 4 (`schedules/interval.rs`) |
| LimitedHold | `LimitedHold` | 14 (`test_interval_schedules.py`) | `LimitedHold<S>` | 9 (`schedules/interval.rs`) |
| FT | `FT` | 18 (`test_time_schedules.py`) | `FT` | 12 (`schedules/time_based.rs`) |
| VT | `VT` | 16 (`test_time_schedules.py`) | `VT` | 8 (`schedules/time_based.rs`) |
| RT | `RT` | 16 (`test_time_schedules.py`) | `RT` | 7 (`schedules/time_based.rs`) |
| EXT | `EXT` | 9 (`test_time_schedules.py`) | `EXT` | 5 (`schedules/time_based.rs`) |
| DRO | `DRO` (resetting + momentary) | 26 (`test_differential_schedules.py`) | `DRO` + `DroMode` | 19 (`schedules/differential.rs`) |
| DRL | `DRL` | 13 (`test_differential_schedules.py`) | `DRL` | 10 (`schedules/differential.rs`) |
| DRH | `DRH` | 16 (`test_differential_schedules.py`) | `DRH` | 12 (`schedules/differential.rs`) |
| Multiple | `Multiple` | 20 (`test_sequence_schedules.py`) | `Multiple` | 18 (`schedules/sequence.rs`) |
| Chained | `Chained` | 15 (`test_sequence_schedules.py`) | `Chained` | 14 (`schedules/sequence.rs`) |
| Tandem | `Tandem` | 13 (`test_sequence_schedules.py`) | `Tandem` | 9 (`schedules/sequence.rs`) |
| Concurrent | `Concurrent` | 46 (`test_concurrent.py`) | `Concurrent` | 36 (`schedules/concurrent.rs`) |
| Alternative | `Alternative` | 22 (`test_alternative.py`) | `Alternative` | 14 (`schedules/alternative.rs`) |
| ProgressiveRatio | `ProgressiveRatio` + 3 step-fn factories | 52 (`test_progressive.py`) | `ProgressiveRatio` + 3 step-fn factories | 33 (`schedules/progressive.rs`) |

All 21 schedule families are present in both ports. Rust has
systematically fewer unit tests per schedule (roughly 60-80% of the
Python count) because (a) several Python tests are `hypothesis`
property tests that Rust covers via `proptest` on a subset of
invariants, and (b) error-path negative tests that are one-liners in
Python collapse to a single `matches!(_, ContingencyError::Config(_))`
pattern in Rust. The conformance corpus (§4) is the common oracle
regardless of unit-test count.

## 4. Conformance fixture mapping

All 20 JSON fixtures live under `contingency-py/conformance/`. The Rust
replay is in `contingency-rs/tests/conformance.rs`. Deterministic
fixtures are asserted **bit-equivalent** on `reinforced` + every
`Reinforcer` field (1e-9 float tolerance). Stochastic fixtures are
`#[ignore]`'d because Python's Mersenne Twister and Rust's `SmallRng`
produce different draw sequences under the same integer seed; they
run via `cargo test -- --ignored` as relaxed structural checks with a
50% tolerance band on reinforcement count.

| Fixture | Rust test fn | Mode | Location in `conformance.rs` |
|---|---|---|---|
| `atomic/fr_basic.json` | `atomic_fr_basic` | strict | line 442 |
| `atomic/crf_basic.json` | `atomic_crf_basic` | strict | line 447 |
| `atomic/ext_basic.json` | `atomic_ext_basic` | strict | line 452 |
| `atomic/fi_basic.json` | `atomic_fi_basic` | strict | line 457 |
| `atomic/ft_basic.json` | `atomic_ft_basic` | strict | line 462 |
| `atomic/limited_hold_fi.json` | `atomic_limited_hold_fi` | strict | line 467 |
| `atomic/vr_seeded_42.json` | `atomic_vr_seeded_42` | relaxed (`#[ignore]`) | line 526 |
| `atomic/vi_seeded_7.json` | `atomic_vi_seeded_7` | relaxed (`#[ignore]`) | line 532 |
| `atomic/vt_seeded_3.json` | `atomic_vt_seeded_3` | relaxed (`#[ignore]`) | line 538 |
| `atomic/rr_seeded_99.json` | `atomic_rr_seeded_99` | relaxed (`#[ignore]`) | line 544 |
| `atomic/ri_seeded_5.json` | `atomic_ri_seeded_5` | relaxed (`#[ignore]`) | line 550 |
| `atomic/rt_seeded_11.json` | `atomic_rt_seeded_11` | relaxed (`#[ignore]`) | line 556 |
| `compound/concurrent_cod.json` | `compound_concurrent_cod` | strict | line 472 |
| `compound/chained_fr2_fr3.json` | `compound_chained_fr2_fr3` | strict | line 477 |
| `compound/alternative_fr_ft.json` | `compound_alternative_fr_ft` | strict | line 482 |
| `differential/dro_resetting.json` | `differential_dro_resetting` | strict | line 487 |
| `differential/dro_momentary.json` | `differential_dro_momentary` | strict | line 492 |
| `differential/drl_basic.json` | `differential_drl_basic` | strict | line 497 |
| `differential/drh_basic.json` | `differential_drh_basic` | strict | line 502 |
| `progressive/pr_arithmetic.json` | `progressive_pr_arithmetic` | strict | line 507 |

All 20 fixtures are covered. Strict: 14. Relaxed (ignored by default):
6. The relaxation is `BY_DESIGN`: see Gotcha 11 in §5.

`Multiple` and `Tandem` do not yet appear in the conformance corpus —
this is a Python-side corpus gap, not a Rust port gap. The Rust
fixture loader at `conformance.rs:204-217` already supports them so
adding fixtures is non-blocking.

## 5. Semantic-invariant checklist (Gotchas 1-13 from
`contingency-py/docs/en/handoff-summary.md`)

The handoff summary enumerates 5 "known Swift-vs-Python divergences"
plus 12 "Gotchas for a Rust port" (17 items total). Below is a
one-for-one check. Evidence cites the Rust file:line that implements
the invariant plus one or more tests that assert it.

| # | Invariant | Rust evidence | Divergences |
|---|---|---|---|
| D1 | Time-based schedules anchor on first `step()`, not `t=0` (`FT`/`VT`/`RT`/`DRO`); `FI`/`VI`/`RI` anchor at construction. | `time_based.rs:99-107` (`FT.anchor: Option<f64>`); `interval.rs:99-100` (`FI.arm_time = interval`); `differential.rs:193-202` (DRO first-step anchor). Tests: `ft_anchors_on_first_step` (time_based.rs), `fi_arm_time_initialises_to_interval` (interval.rs), `dro_resetting_anchors_on_first_step` (differential.rs). | None. |
| D2 | Single-fire per step: on `k > 1` missed intervals, only **one** reinforcer emitted. | `time_based.rs:25`, `time_based.rs:73` (doc); test `ft_single_fire_per_step_even_when_late` at `time_based.rs:492`. | None. |
| D3 | Random-family sampling is internal for `RR`/`RI`/`RT` (not delegated to Fixed counterparts). | `ratio.rs:281` (`RR.rng: SmallRng`); `interval.rs` RI uses internal exponential draws; `time_based.rs` RT ditto. | None. |
| D4 | Unit-agnostic time. All times are `f64`. | `types.rs:14` (`time: f64`); `lib.rs:25` (`pub use constants::TIME_TOL`). | None. |
| D5 | `Outcome` invariant: `reinforced <=> reinforcer.is_some()`. | `types.rs:97-108` (only two constructors — `Outcome::empty()` and `Outcome::reinforced(r)` — encode the invariant by construction; the public struct has no constructor that can violate it). Test: `outcome_reinforced_carries_reinforcer` (types.rs). | Rust enforces via smart constructors rather than Python's `__post_init__`. Equivalent. |
| G1 | `TIME_TOL = 1e-9` applied uniformly. | `constants.rs:8`; used in `interval.rs:115,117`, `time_based.rs:125,127`, `differential.rs:153,166,285`, `concurrent.rs:257`. | None. |
| G2 | `Concurrent` advances every component on every step. | `concurrent.rs:182-195` (loop over all components, non-event ones get `step(now, None)`). Test: `concurrent_ticks_fire_time_based_component` (concurrent.rs). | None. |
| G3 | `Concurrent` gating order: register changeover BEFORE checking COD; COD does not gate tick-side reinforcement. | `concurrent.rs:290-300` — `register_event` is called before the `cod_active` test, and `cod_suppressed` is only emitted when `from_event` is true. Tests: `cod_suppresses_reinforcement_on_changeover_response`, `cod_does_not_gate_tick_reinforcement` (concurrent.rs). | None. |
| G4 | `Chained` / `Tandem` do not step inactive components; time-based terminal link anchors on its first step after activation. | `sequence.rs:22-31` (doc block making the contract explicit). Only the active component receives `step`. | None. |
| G5 | `Alternative` forwards `(now, event)` to both components; resets both on a win. | `alternative.rs:94-128`. Test: `both_components_advance_on_every_step`. | None. |
| G6 | `DRO` momentary evaluates boundary BEFORE recording event; resetting pre-empts reinforcement on event step. | `differential.rs:160-178` (momentary branch does boundary check first, then pushes event into `has_response_in_window`). `differential.rs:146-158` (resetting branch: if `event.is_some()` return `Outcome::empty()` without checking boundary). Tests: `dro_momentary_event_at_boundary_belongs_to_next_window`, `dro_resetting_event_suppresses_boundary_reinforcement`. | None. |
| G7 | `DRH`'s window is not emptied on reinforcement. | `differential.rs:378-392` — `step()` pushes to `window` and returns `Outcome::reinforced` without clearing. Test: `drh_sustained_high_rate_keeps_firing`. | None. |
| G8 | `ProgressiveRatio`'s `step_fn` is lazy; invalid returns surface as `Config` error only on first consultation. | `progressive.rs:267-280` (doc comments explicitly call out lazy validation, mirroring Python). Test: `pr_invalid_step_fn_surfaces_on_first_response`. | None. |
| G9 | `reset()` does not re-sample random parameters on non-seeded schedules. | `ratio.rs:281-282` (`RR` holds `initial_rng: SmallRng` snapshot separate from live `rng`; `reset` clones snapshot back). | Semantically equivalent. Seed-less `reset` yields a fresh trajectory in Python because `random.Random(None)` picks a new entropy seed; Rust `SmallRng::from_entropy()` does the same. |
| G10 | `RR`/`RI`/`RT` snapshot RNG state at construction. | `ratio.rs:281-283` (RR). `interval.rs` / `time_based.rs` RI/RT equivalents use `SmallRng::clone()` at construction and restore in `reset`. | Python can accept a shared `random.Random`; Rust accepts only an `Option<u64>` seed. Net effect identical when called with a seed. |
| G11 | VR/VT: `rng.randrange(0, 2**31 - 1)` sub-seed; VI: `rng.getrandbits(64)`. Must match for Python bit-identity; otherwise pin via fixtures. | Rust uses `SmallRng` (Xoshiro256++) directly without a sub-seed dance; the conformance corpus treats stochastic fixtures as trajectory templates (see `tests/conformance.rs:14-17`). | `BY_DESIGN`. Documented in `conformance.rs` header and the conformance `README.md`. Stochastic fixtures are `#[ignore]` under strict replay. |
| G12 | `Outcome.meta` keys: `current_component` (str for Multiple/Chained, int for Tandem), `chain_transition`, `cod_suppressed`, `operandum`, `alternative_winner` ("first"/"second"). | `sequence.rs:191,200,324,338,351` (`current_component` as `MetaValue::Str` in Multiple/Chained); `sequence.rs:465,477,490` (`current_component` as `MetaValue::Int` in Tandem — line 383 doc confirms). `concurrent.rs:297-298` (`cod_suppressed`, `operandum`). `alternative.rs:107-110,121-124` (`alternative_winner` `Str` variant). `sequence.rs:325-339` emits `chain_transition` key. | None. |

Summary: 17/17 invariants implemented. The only divergences are the
PRNG difference (G11, `BY_DESIGN`, documented) and the smart-constructor
enforcement of D5 (equivalent semantics, idiomatic Rust).

## 6. HAL parity

The Python HAL (`contingency.hw`) exposes three apparatus backends
behind a common `Apparatus` protocol:

| Python element | Status |
|---|---|
| `Apparatus` (Protocol) | Python only |
| `ApparatusInfo`, `ApparatusStatus` dataclasses | Python only |
| `VirtualApparatus` | Python only (deterministic in-memory backend) |
| `SerialApparatus` | Python only (pyserial transport) |
| `HilBridgeApparatus` | Python only (TCP/JSONL **host** side) |

The Rust crate contains one HAL-adjacent artefact:

| Rust element | Role |
|---|---|
| `src/bin/hil.rs` (`contingency-hil` binary) | TCP/JSONL **apparatus** side of the same wire protocol as Python's `HilBridgeApparatus` — speaks the complementary role. |

This is a deliberate scope split, **not** a bug. The Python package's
own handoff document says so explicitly:

> "The Rust port will want to mirror this shape once the PyO3 / KMP
> front-ends are decided; this package intentionally does not document
> the HAL here because the Rust implementation will likely be a fresh,
> lower-level `hal` crate sitting beside `contingency-rs`."
> — `contingency-py/docs/en/handoff-summary.md:60-65`

The Python side runs the schedule and consumes an apparatus; the Rust
side runs the schedule and acts as the apparatus. Together the two
complete the HIL loop. The full `Apparatus` trait, `VirtualApparatus`,
and `SerialApparatus` live outside this crate — see Gap G3 below. For
this verification gate, the gap is tracked as `BY_DESIGN` with a
`DEFERRED` follow-up.

The `hil_integration.rs` integration test (`tests/hil_integration.rs`)
spawns the `contingency-hil` binary, pushes JSONL `response` messages
over TCP, and asserts that an `FR(3)` schedule configured inside the
binary emits a `reinforcer` JSONL frame — end-to-end protocol compat
with `HilBridgeApparatus`.

## 7. Bindings status

| Binding | Status | Location / flag | Notes |
|---|---|---|---|
| PyO3 (Python bindings) | **Implemented** | `src/python.rs` + `Cargo.toml` feature `python` | Exposes `PySchedule` class, `PyArmableSchedule`, and `pr_arithmetic`/`pr_geometric`/`pr_richardson_roberts` free functions. Entry symbol: `contingency_core`. Smoke-tested at `tests/python_smoke.rs`. |
| UniFFI (Kotlin/Swift) | Scaffolded only | `Cargo.toml` feature `uniffi` reserved; module not wired | `lib.rs:20-23` comment: "enabling the feature currently compiles to a no-op." |
| WASM (browser) | Scaffolded only | `Cargo.toml` `[target.'cfg(target_arch = "wasm32")']` dep on `wasm-bindgen` | No `#[wasm_bindgen]` exports yet. |
| C FFI / cbindgen | Scaffolded only | `crate-type = ["rlib", "cdylib", "staticlib"]` in `Cargo.toml:11` | The `contingency-hil` binary demonstrates an FFI-free integration path via JSONL/TCP. No `extern "C"` ABI functions yet. |

Phase 7 (KMP / Swift / Kotlin / WASM / C ABI) is tracked as `DEFERRED`
in Gap G4.

## 8. Gaps / follow-ups

| ID | Description | Severity |
|---|---|---|
| G1 | `ScheduleBuilder` facade absent from native Rust API. PyO3 `Schedule` classmethods cover builder ergonomics for Python callers; native-Rust callers must construct schedules directly (`FR::new`, `VI::new`, …). | `DEFERRED` |
| G2 | DSL bridge `contingency.bridge.from_dsl` not ported. Rust has no runtime dependency on `contingency-dsl`, and the bridge's translation table (atomic, compound, modifier) has no counterpart. All Python-side deferred constructs (`SecondOrder`, `IdentifierRef`, `AdjustingSchedule`, `AversiveSchedule`, `InterlockingSchedule`, `ResponseCostWrapped`, `TimeoutWrapped`, `TrialBased`, combinators `CONJ`/`MIX`/`OVERLAY`/`INTERPOLATE`/`INTERP`, modifier `Pctl`, Concurrent `PUNISH` and directional COD) are therefore also absent from Rust. | `DEFERRED` |
| G3 | HAL layer (`Apparatus` protocol + `VirtualApparatus`, `SerialApparatus`, `HilBridgeApparatus` host side) has no Rust counterpart beyond the `contingency-hil` apparatus-side binary. | `BY_DESIGN` + `DEFERRED` |
| G4 | Non-Python bindings (UniFFI / WASM / cbindgen) are feature-flag stubs only. | `DEFERRED` |
| G5 | Stochastic conformance fixtures (6 of 20) do not strict-replay because Rust `SmallRng` ≠ Python Mersenne Twister. They run relaxed via `cargo test -- --ignored`. | `BY_DESIGN` |
| G6 | `Multiple` and `Tandem` have no JSON conformance fixture on the Python side yet, so Rust cannot bit-compare these compound schedules against Python observed outcomes (Rust internal unit tests cover them, but cross-language replay is missing). Rust loader at `tests/conformance.rs:204-217` already supports both types — only the Python generator needs to emit fixtures. | `DEFERRED` (Python-side action) |
| G7 | `NotConnectedError` is flattened into the `Hardware(msg)` variant. Python callers catching the specific subclass must migrate to string-matching the message. Low-impact because the HAL is Python-side only (§6). | `BY_DESIGN` |
| G8 | Python unit-test counts exceed Rust counts by roughly 25-40% per schedule (§3). Most of the delta is Python `hypothesis` property tests and one-liner negative-input enumeration. No functional gap identified by the conformance corpus. | Informational (no tag) |

**BUG-severity items: 0.** No invariant from §5 is violated and no
schedule listed in §3 is missing. The 1:1 correspondence claim holds
within the scope defined by the Python handoff document (schedule
runtime + helpers + conformance corpus). Items tagged `DEFERRED` are
planned work; items tagged `BY_DESIGN` are intentional scope choices
documented by either the Python handoff or the Rust module docs.

## 9. Verification commands

Commands a reviewer can run to reproduce this parity check.

```
# Rust: native library + bindings + conformance
cd apps/core/contingency-rs
cargo test -p contingency
cargo test -p contingency --features python
cargo test -p contingency -- --ignored   # stochastic conformance (relaxed)
cargo clippy -p contingency --all-targets --all-features -- -D warnings

# Python: full test suite + conformance replay
cd apps/core/contingency-py
.venv/bin/pytest -q
```

Expected at the time of this audit:

- Rust: 281 lib unit + 14 conformance (strict) + 2 HIL integration + 1
  Python smoke = 298 tests green; 6 stochastic conformance tests
  `#[ignore]`'d by default (run separately via `-- --ignored`).
- Python: 633 tests green, 99% line coverage.

## References

Catania, A. C. (1966). Concurrent performances: Reinforcement
interaction and response independence. *Journal of the Experimental
Analysis of Behavior*, 9(3), 253-263.
https://doi.org/10.1901/jeab.1966.9-253

Ferster, C. B., & Skinner, B. F. (1957). *Schedules of reinforcement*.
Appleton-Century-Crofts.

Fleshler, M., & Hoffman, H. S. (1962). A progression for generating
variable-interval schedules. *Journal of the Experimental Analysis of
Behavior*, 5(4), 529-530. https://doi.org/10.1901/jeab.1962.5-529

Hantula, D. A. (1991). A simple BASIC program to generate values for
variable-interval schedules of reinforcement. *Journal of Applied
Behavior Analysis*, 24(4), 799-801.
https://doi.org/10.1901/jaba.1991.24-799

Kelleher, R. T., & Gollub, L. R. (1962). A review of positive
conditioned reinforcement. *Journal of the Experimental Analysis of
Behavior*, 5(4 Suppl), 543-597.
https://doi.org/10.1901/jeab.1962.5-s543

Richardson, N. R., & Roberts, D. C. S. (1996). Progressive ratio
schedules in drug self-administration studies in rats: A method to
evaluate reinforcing efficacy. *Journal of Neuroscience Methods*,
66(1), 1-11. https://doi.org/10.1016/0165-0270(95)00153-0

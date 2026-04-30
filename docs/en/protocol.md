# Schedule Protocol — Language-Neutral Contract

:jp: [日本語版](../ja/protocol.md)

This document is the authoritative surface that `contingency-rs`
implements. Every schedule in the family conforms to it; compound
schedules compose over it.

## Source of truth

`contingency-py` is the authoritative executable specification. The
22 conformance fixtures under `contingency-py/conformance/` are the
bit-equivalence oracle for deterministic schedules. The Rust crate
re-implements the same contract and is held to the fixtures.

Python definitions:

- `contingency-py/src/contingency/interfaces.py` — the `Schedule` Protocol.
- `contingency-py/src/contingency/entities.py` — value objects.
- `contingency-py/src/contingency/errors.py` — error taxonomy.

Rust mirrors (navigation pointers for this crate):

- [src/schedule.rs](../../src/schedule.rs) — the `Schedule` trait.
- [src/types.rs](../../src/types.rs) — value objects.
- [src/errors.rs](../../src/errors.rs) — error taxonomy.

## Schedule interface

### Rust

```rust
pub trait Schedule {
    fn step(&mut self, now: f64, event: Option<&ResponseEvent>) -> Result<Outcome>;
    fn reset(&mut self);
}
```

A mutable borrow is required because schedules are stateful. Implementers
that need interior mutability (e.g. wrappers storing `Box<dyn Schedule>`)
should use `&mut self` at the trait level and keep RNG / counters in
private fields. A dedicated `Box<dyn Schedule>` impl is provided to
re-dispatch through the inner value.

### Python (canonical)

```python
@runtime_checkable
class Schedule(Protocol):
    def step(self, now: float, event: ResponseEvent | None = None) -> Outcome: ...
    def reset(self) -> None: ...
```

## Semantic contract

### `step(now, event)`

1. **Monotonic time.** `now` must be `>= last_now` (where `last_now` is
   the `now` passed on the previous call; unset before the first call).
   If `now < last_now - 1e-9` the schedule raises
   `ScheduleStateError`.
2. **Event timestamp consistency.** If `event` is not `None`, then
   `|event.time - now| <= 1e-9`. Otherwise raises
   `ScheduleStateError`.
3. **Single outcome per call.** Exactly one `Outcome` is returned. Even
   if the elapsed time since last step is larger than multiple scheduled
   intervals, at most one reinforcer fires on this step (time-based
   schedules re-anchor to `now` after firing).
4. **Idempotent on `event=None`.** A pure tick may or may not produce a
   reinforcer depending on the schedule family:
   - Ratio schedules (FR, VR, RR): never reinforce on a tick.
   - Interval schedules (FI, VI, RI): never reinforce on a tick.
   - Time-based schedules (FT, VT, RT): may reinforce on a tick once
     the scheduled time has elapsed.
   - Differential schedules (DRO): the resetting variant reinforces on
     a tick after the interval elapses with no intervening response;
     the momentary variant reinforces at interval boundaries.
   - Compound (Concurrent, Multiple, Chained, Tandem, Alternative): may
     reinforce on a tick iff any active component does.
5. **Reinforcer timestamp.** Any emitted `Reinforcer` has
   `Reinforcer.time == now`.
6. **Anchor on first step.** Schedules that track "elapsed time since
   anchor" (FT, VT, RT, DRO) anchor their internal clock at the first
   `now` received. The first step therefore never reinforces.

### `reset()`

Returns the schedule to the state it had immediately after construction.

- RNG state is restored:
  - Schedules owning a seed re-seed from the original seed (VR, VI, VT).
  - Schedules borrowing an `rng: random.Random` snapshot the RNG state
    at construction and restore it (RR, RI, RT).
- Counters are zeroed; sequence cursors rewind; sequence pools are
  regenerated bit-identically if seeded.
- `last_now` and time anchors become `None` (unset).
- Compound schedules propagate `reset()` to every component, then clear
  their own bookkeeping.

`reset()` does **not** change the configured parameters (ratio, mean,
step function, component list, COD value, etc.).

## Value objects

All four are immutable. Rust uses `#[derive(Clone, Debug, PartialEq)]`.

### `ResponseEvent`

```rust
#[derive(Clone, Debug, PartialEq)]
pub struct ResponseEvent {
    pub time: f64,
    pub operandum: String,  // default "main"
}
```

```python
@dataclass(frozen=True)
class ResponseEvent:
    time: float
    operandum: str = "main"
```

- `operandum` identifies the operandum (lever, key, button) that was
  pressed. Used by `Concurrent` to route the event.
- Default `"main"` is used by non-compound schedules; its value is never
  inspected unless a compound schedule routes on it.

### `Observation`

```rust
#[derive(Clone, Debug, PartialEq)]
pub struct Observation {
    pub time: f64,
    pub response_count: u64,  // default 0
}
```

```python
@dataclass(frozen=True)
class Observation:
    time: float
    response_count: int = 0
```

Observation is a snapshot currently unused by the runtime itself (kept
for future analyser hooks).

### `Reinforcer`

```rust
#[derive(Clone, Debug, PartialEq)]
pub struct Reinforcer {
    pub time: f64,
    pub magnitude: f64,   // default 1.0
    pub label: String,    // default "SR+"
}
```

```python
@dataclass(frozen=True)
class Reinforcer:
    time: float
    magnitude: float = 1.0
    label: str = "SR+"
```

- `magnitude` encodes the amount (pellets, ml, points).
- Negative magnitude + `label="SR-"` models aversive control.
- Every schedule in this library emits reinforcers with magnitude
  `1.0` and `label="SR+"` unless an experimental wrapper overrides
  them. Both ports use the same defaults.

### `Outcome`

```rust
#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    pub reinforced: bool,
    pub reinforcer: Option<Reinforcer>,
    pub meta: BTreeMap<String, MetaValue>,
}
```

```python
@dataclass(frozen=True)
class Outcome:
    reinforced: bool = False
    reinforcer: Reinforcer | None = None
    meta: dict[str, object] = field(default_factory=dict)
```

**Invariants enforced at construction** (both ports):
- `reinforced == true` iff `reinforcer.is_some()`.

Rust exposes only safe constructors that enforce the invariant:

```rust
impl Outcome {
    pub fn unreinforced() -> Self { ... }
    pub fn unreinforced_with_meta(meta: BTreeMap<String, MetaValue>) -> Self { ... }
    pub fn reinforced(r: Reinforcer) -> Self { ... }
    pub fn reinforced_with_meta(r: Reinforcer, meta: BTreeMap<String, MetaValue>) -> Self { ... }
}
```

`meta` is used by compound schedules to surface which component fired:

- `Concurrent`: sets `cod_suppressed: bool` and `operandum: str` when a
  reinforcer is gated away by COD.
- `Multiple` / `Chained`: sets `current_component: str` to the currently
  active component's stimulus label, and
  `chain_transition: bool` at non-terminal link transitions in
  `Chained`.
- `Tandem`: sets `current_component: int` (the active link index).
- `Alternative`: sets `alternative_winner: "first" | "second"`.

For cross-language fixtures, `meta` values are restricted to JSON
primitives (bool, int, string). Both ports honour this.

## Error taxonomy

### Rust

```rust
#[derive(Debug, thiserror::Error)]
pub enum ContingencyError {
    #[error("schedule configuration error: {0}")]
    Config(String),

    #[error("schedule state error: {0}")]
    State(String),

    #[error("hardware error: {0}")]
    Hardware(String),
}

pub type Result<T> = std::result::Result<T, ContingencyError>;
```

The `Schedule` trait's `step()` returns `Result<Outcome>`; the Rust
crate prefers `Err` over panics for all state violations.

### Python (canonical)

```
ContingencyError (base)
├── ScheduleConfigError (also ValueError)
├── ScheduleStateError (also RuntimeError)
└── HardwareError
    └── NotConnectedError (also RuntimeError)
```

| Python class | Rust variant | Raised when |
|---|---|---|
| `ScheduleConfigError` | `ContingencyError::Config` | Bad constructor params (negative ratio, unknown combinator) |
| `ScheduleStateError` | `ContingencyError::State` | `step()` contract violated (non-monotonic time, event/now mismatch) |
| `HardwareError` | `ContingencyError::Hardware` | HAL I/O, transport, config, missing dependency |
| `NotConnectedError` | `ContingencyError::Hardware` (with prefix message) | HAL read/write before `connect()` or after `disconnect()` |

`NotConnectedError` is intentionally subsumed into `Hardware(..)` in
Rust; callers needing to distinguish the two cases inspect the
message prefix. See `correspondence.md` Gap G7.

## Time tolerance

A single constant governs floating-point comparisons. Rust exposes
it as a module-level constant in `src/constants.rs`:

```rust
pub const TIME_TOL: f64 = 1e-9;
```

Python defines it as `_TIME_TOL = 1e-9` in the `interfaces` module.

It is applied identically by both ports:

- In monotonic-time checks: `now < last_now - TIME_TOL` fails.
- In event/now matching: `|event.time - now| > TIME_TOL` fails.
- In elapsed-time checks: `now - anchor + TIME_TOL >= interval` fires.
- In sliding-window eviction (DRH): `w[0] < cutoff - TIME_TOL` evicts.
- In LimitedHold expiry: `now > arm_time + hold + TIME_TOL` expires.

The constant is load-bearing for the conformance corpus.

## Threading model

The Python implementation is **not** thread-safe. Callers are expected
to serialise access per-schedule. The Rust crate can trivially be
`Send` but is not `Sync` without explicit synchronisation — the
`&mut self` convention of the trait lets the borrow checker enforce
single-threaded access.

# Differential reinforcement — DRO / DRL / DRH

:jp: [日本語版](../../ja/algorithms/dro-drl-drh.md)

## References

Ferster, C. B., & Skinner, B. F. (1957). *Schedules of reinforcement*.
Appleton-Century-Crofts.

Reynolds, G. S. (1961). Behavioral contrast. *JEAB*, 4(1), 57-71.
https://doi.org/10.1901/jeab.1961.4-57

Reynolds, G. S. (1964). Accurate and rapid reconditioning of
spaced-responding by differential reinforcement of other behavior.
*JEAB*, 7(3), 223-224. https://doi.org/10.1901/jeab.1964.7-223

Zeiler, M. D. (1977). Schedules of reinforcement: The controlling
variables. In W. K. Honig & J. E. R. Staddon (Eds.), *Handbook of
operant behavior* (pp. 201-232). Prentice-Hall.

---

## DRO — Differential Reinforcement of Other behavior

Reinforces the **absence** of a response over an interval. Two
variants are supported.

### Resetting variant (default)

Every response resets the DRO timer to `now`. A reinforcer is
delivered on the first **tick** whose `now` satisfies
`now - anchor >= interval` with no intervening response. After
reinforcement, the timer restarts at `now`.

A response that arrives exactly on or after the interval boundary
does **not** reinforce — any response resets the timer. Use the
momentary variant for boundary-based reinforcement.

### Momentary variant

The timer runs continuously, independent of responses. At each
interval boundary (`now >= anchor + interval`) a reinforcer is
delivered iff **no** response occurred in `[anchor, now)` — a
half-open window where a response at exactly `now` belongs to the
next window. The anchor advances to `now` on every boundary
regardless of outcome.

### State variables

| Name | Type | Purpose |
|---|---|---|
| `interval` | float (const) | Window length (`> 0`). |
| `type` | "resetting" \| "momentary" | Variant. |
| `anchor` | float? | Timer / window-start; `None` before first step. |
| `has_response_in_window` | bool | Momentary variant only: has a response occurred since the current window opened. |
| `last_now` | float? | Monotonic-time check. |

### Step pseudocode

```
fn step(now, event):
    // monotonic + event validation
    last_now = now
    if anchor is None:
        anchor = now
        if event is not None:
            has_response_in_window = true
        return Outcome::unreinforced()
    if type == "resetting":
        if event is not None:
            anchor = now                // reset timer; no reinforcement
            return Outcome::unreinforced()
        if now - anchor + TIME_TOL >= interval:
            anchor = now
            return Outcome::reinforced(Reinforcer { time: now })
        return Outcome::unreinforced()
    else:  // momentary
        reinforced = false
        if now - anchor + TIME_TOL >= interval:
            if not has_response_in_window:
                reinforced = true
            anchor = now
            has_response_in_window = false
        if event is not None:
            has_response_in_window = true
        if reinforced:
            return Outcome::reinforced(Reinforcer { time: now })
        return Outcome::unreinforced()
```

### Reset semantics

- `anchor = None`, `has_response_in_window = false`, `last_now = None`.

### Determinism

Deterministic. No RNG.

---

## DRL — Differential Reinforcement of Low rate

Reinforces a response whose inter-response time (IRT) is at least
`interval`.

### Definition

A response at `now` is reinforced iff:

- it is the **first** response since construction / reset, **or**
- the previous response occurred at least `interval` before `now`
  (`now - last_response_time >= interval` with TOL slack).

**Every** response (reinforced or not) updates `last_response_time`.
Ticks with `event=None` never reinforce and never modify state
beyond the monotonic-time check.

### State

| Name | Type |
|---|---|
| `interval` | float (const) |
| `last_response_time` | float? |
| `last_now` | float? |

### Step pseudocode

```
fn step(now, event):
    // monotonic + event validation
    last_now = now
    if event is None:
        return Outcome::unreinforced()
    prev = last_response_time
    last_response_time = now         // always update
    if prev is None or now - prev + TIME_TOL >= interval:
        return Outcome::reinforced(Reinforcer { time: now })
    return Outcome::unreinforced()
```

### Reset

`last_response_time = None`, `last_now = None`.

---

## DRH — Differential Reinforcement of High rate

Reinforces a response whenever at least `response_count` responses
have occurred within the last `time_window` time units (a sliding
window).

### Definition

Maintain a FIFO of recent response timestamps. On every step (tick
or response), evict timestamps strictly older than `now -
time_window` (with TOL slack). On a response, append `now` to the
right; if the window then holds `>= response_count`, reinforce.

The window is **not** emptied on reinforcement — a sustained
high-rate train will keep producing reinforcers on each qualifying
response.

### State

| Name | Type |
|---|---|
| `response_count` | int (const, `>= 1`) |
| `time_window` | float (const, `> 0`) |
| `window` | deque[float] |
| `last_now` | float? |

### Step pseudocode

```
fn step(now, event):
    // monotonic + event validation
    last_now = now
    evict_old(now)                        // remove w[0] < cutoff - TOL
    if event is None:
        return Outcome::unreinforced()
    window.push_back(now)
    if len(window) >= response_count:
        return Outcome::reinforced(Reinforcer { time: now })
    return Outcome::unreinforced()

fn evict_old(now):
    cutoff = now - time_window
    while window and window[0] < cutoff - TIME_TOL:
        window.pop_front()
```

### Reset

`window.clear()`, `last_now = None`.

### Edge cases

- `response_count < 1` or `time_window <= 0`: `Config`.
- Boundary: a timestamp exactly at `now - time_window` stays in the
  window (inclusive, with TOL slack).
- DRH as used by the DSL bridge is always configured with
  `response_count = 2`: the DSL's `DRHNs` expression carries only a
  time value interpreted as the maximum IRT.

## Determinism

All three are deterministic. No RNG.

## Divergence from Swift original

None of these schedules are in the legacy Swift package. All three
are new in `contingency-py`.

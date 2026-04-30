# Multiple / Chained / Tandem — sequence compounds

:jp: [日本語版](../../ja/algorithms/multiple-chained-tandem.md)

## References

Ferster, C. B., & Skinner, B. F. (1957). *Schedules of reinforcement*.
Appleton-Century-Crofts.

Kelleher, R. T., & Gollub, L. R. (1962). A review of positive
conditioned reinforcement. *JEAB*, 5(4 Suppl), 543-597.
https://doi.org/10.1901/jeab.1962.5-s543

Reynolds, G. S. (1961). Behavioral contrast. *JEAB*, 4(1), 57-71.
https://doi.org/10.1901/jeab.1961.4-57

## Shared definition

All three compose `N >= 2` component schedules and expose the same
`Schedule` protocol. They differ in what happens when the active
component reinforces.

**Inactive components are not stepped.** Only the active component
receives `step()` calls. Time-based inner schedules therefore anchor
at their **first step after activation**, exactly as in a real
operant chamber where S^D and clock both restart on link entry.

## `Multiple` (`mult`)

Cyclic rotation through components, each with its own S^D.
Reinforcement on the active component delivers a primary reinforcer
and advances the active index (wrapping to 0 after the last).

### State

| Name | Type |
|---|---|
| `components` | list[Schedule] |
| `stimuli` | list[str] (unique) |
| `active` | int (active index) |
| `last_now` | float? |

### Step pseudocode

```
fn step(now, event):
    // monotonic + event validation
    inner = components[active].step(now, event)
    stim = stimuli[active]
    meta = dict(inner.meta)
    meta["current_component"] = stim
    if inner.reinforced:
        active = (active + 1) mod N
        return Outcome::reinforced_with_meta(inner.reinforcer, meta)
    return Outcome::unreinforced_with_meta(meta)
```

### Reset

- `active = 0`
- Reset every component.
- `last_now = None`

## `Chained` (`chain`)

Sequential chain through components with distinct S^Ds. Non-terminal
completions are **conditioned** reinforcement: S^D changes but **no**
primary `Reinforcer` is delivered. Only the terminal (last) link
delivers primary reinforcement; after that, `active` cycles back to 0.

### Step pseudocode

```
fn step(now, event):
    inner = components[active].step(now, event)
    if inner.reinforced:
        if active == N - 1:        // terminal
            active = 0
            meta = dict(inner.meta)
            meta["current_component"] = stimuli[active]
            return Outcome::reinforced_with_meta(inner.reinforcer, meta)
        // non-terminal: conditioned reinforcement
        active += 1
        meta = dict(inner.meta)
        meta["current_component"] = stimuli[active]
        meta["chain_transition"] = true
        return Outcome::unreinforced_with_meta(meta)
    meta = dict(inner.meta)
    meta["current_component"] = stimuli[active]
    return Outcome::unreinforced_with_meta(meta)
```

### Reset

Same as `Multiple`.

## `Tandem` (`tand`)

Structurally identical to `Chained` but without distinctive S^Ds.
The subject receives no external cue on link transitions. `meta`
carries an **integer** `current_component` index rather than a
stimulus label.

### Step pseudocode

Identical to `Chained` except `meta["current_component"] = active`
(the integer index).

## Edge cases (all three)

- `< 2` components: `Config`.
- Stimulus labels must be unique and of the correct length (Multiple /
  Chained).
- `None` events still advance the wrapper's time bookkeeping but the
  active component decides whether to reinforce.
- `meta["chain_transition"]` is set on non-terminal completions in
  `Chained` / `Tandem` — useful for sessions recording link
  transitions.

## Determinism

Inherits component determinism. No RNG in the compound itself.

## Divergence from Swift original

No sequence compounds in the legacy Swift package. Multiple, Chained,
Tandem are new in `contingency-py`.

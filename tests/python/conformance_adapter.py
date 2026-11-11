"""Conformance adapter for the PyO3 module ``contingency_core``.

Reads one fixture JSON document on stdin, builds its schedule with
``contingency_core.from_spec``, steps it through every ``(now, event)``
pair and writes the observed document on stdout. It is the Rust-engine
counterpart of contingency-py's ``replay-python`` adapter and is meant
to be driven by contingency-py's checker::

    contingency-conformance run --stochastic strict -- \
        <venv>/bin/python tests/python/conformance_adapter.py

``<venv>`` is a virtual environment where ``maturin develop --features
python`` installed ``contingency_core``.
"""

from __future__ import annotations

import json
import sys
from typing import Any

import contingency_core as cc

FLOAT_NDIGITS = 9


def _round(x: float) -> float:
    return round(x, FLOAT_NDIGITS)


def observe(outcome: cc.Outcome) -> dict[str, Any]:
    """Serialise one Outcome the same way contingency-py's adapter does."""
    out: dict[str, Any] = {"reinforced": outcome.reinforced}
    if outcome.reinforcer is not None:
        out["reinforcer"] = {
            "time": _round(outcome.reinforcer.time),
            "magnitude": _round(outcome.reinforcer.magnitude),
            "label": outcome.reinforcer.label,
        }
    meta: dict[str, Any] = {}
    for key, value in outcome.meta.items():
        if isinstance(value, (bool, int, str)):
            meta[key] = value
        elif isinstance(value, float):
            meta[key] = _round(value)
    if meta:
        out["meta"] = meta
    return out


def replay(fixture: dict[str, Any]) -> dict[str, Any]:
    """Run a fixture document through the Rust engine."""
    schedule = cc.from_spec(fixture["schedule"])
    steps: list[dict[str, Any]] = []
    for step in fixture["steps"]:
        ev = step["event"]
        event = (
            cc.ResponseEvent(ev["time"], ev.get("operandum", "main"))
            if ev is not None
            else None
        )
        steps.append(observe(schedule.step(step["now"], event)))
    return {"implementation": "contingency-rs (contingency_core)", "steps": steps}


def main() -> int:
    observed = replay(json.loads(sys.stdin.read()))
    json.dump(observed, sys.stdout)
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())

"""Tests for the PyO3 module ``contingency_core``.

Run inside a virtual environment where the module was installed with
``maturin develop --features python``::

    python -m pytest tests/python

The conformance corpus is read from ``CONTINGENCY_CONFORMANCE_DIR`` or,
when unset, from the sibling checkout ``../contingency-py/conformance/``
(the same directory ``tests/conformance.rs`` reads).
"""

from __future__ import annotations

import json
import os
from pathlib import Path
from typing import Any

import pytest

import contingency_core as cc

HERE = Path(__file__).resolve().parent


def _corpus_dir() -> Path:
    env = os.environ.get("CONTINGENCY_CONFORMANCE_DIR")
    if env:
        return Path(env)
    return HERE.parents[1].parent / "contingency-py" / "conformance"


def _fixtures() -> list[Path]:
    root = _corpus_dir()
    if not root.is_dir():
        return []
    return sorted(p for p in root.rglob("*.json") if p.name != "index.json")


def _types(node: Any) -> set[str]:
    found: set[str] = set()
    if isinstance(node, dict):
        if isinstance(node.get("type"), str) and "params" in node:
            found.add(node["type"])
        for value in node.values():
            found |= _types(value)
    elif isinstance(node, list):
        for value in node:
            found |= _types(value)
    return found


# ---------------------------------------------------------------------------
# from_spec
# ---------------------------------------------------------------------------


def test_from_spec_builds_fr_from_dict() -> None:
    s = cc.from_spec({"type": "FR", "params": {"n": 3}})
    got = [s.step(float(t), cc.ResponseEvent(float(t))).reinforced for t in range(1, 7)]
    assert got == [False, False, True, False, False, True]


def test_from_spec_accepts_json_string_and_classmethod() -> None:
    s = cc.Schedule.from_spec(json.dumps({"type": "CRF", "params": {}}))
    assert s.step(1.0, cc.ResponseEvent(1.0)).reinforced


def test_from_spec_concurrent_routes_by_operandum() -> None:
    s = cc.from_spec(
        {
            "type": "Concurrent",
            "params": {
                "components": {
                    "left": {"type": "FR", "params": {"n": 1}},
                    "right": {"type": "EXT", "params": {}},
                },
                "cod": 0.0,
            },
        }
    )
    assert not s.step(1.0, cc.ResponseEvent(1.0, "right")).reinforced
    out = s.step(2.0, cc.ResponseEvent(2.0, "left"))
    assert out == cc.Outcome(True, cc.Reinforcer(2.0))


@pytest.mark.parametrize(
    "spec",
    [
        {"type": "NoSuchSchedule", "params": {}},
        {"type": "FR", "params": {"n": 0}},
        {"params": {"n": 1}},
        "not json",
        [1, 2],
    ],
)
def test_from_spec_rejects_bad_spec_with_value_error(spec: Any) -> None:
    with pytest.raises(ValueError):
        cc.from_spec(spec)


def test_spec_types_lists_every_type() -> None:
    assert len(cc.SPEC_TYPES) == 37
    assert len(set(cc.SPEC_TYPES)) == 37
    assert "Concurrent" in cc.SPEC_TYPES


def test_corpus_covers_every_spec_type_through_python() -> None:
    fixtures = _fixtures()
    if not fixtures:
        pytest.skip(f"conformance corpus not found at {_corpus_dir()}")
    covered: set[str] = set()
    for path in fixtures:
        schedule = json.loads(path.read_text(encoding="utf-8"))["schedule"]
        cc.from_spec(schedule)
        covered |= _types(schedule)
    assert covered == set(cc.SPEC_TYPES)


# ---------------------------------------------------------------------------
# Value objects: same names and shapes as contingency-py's entities
# ---------------------------------------------------------------------------


def test_reinforcer_constructor_defaults_and_equality() -> None:
    r = cc.Reinforcer(2.0)
    assert (r.time, r.magnitude, r.label) == (2.0, 1.0, "SR+")
    assert r == cc.Reinforcer(time=2.0, magnitude=1.0, label="SR+")
    assert r != cc.Reinforcer(2.0, -1.0, "SR-")


def test_outcome_constructor_defaults_and_validation() -> None:
    empty = cc.Outcome()
    assert (empty.reinforced, empty.reinforcer, empty.meta) == (False, None, {})
    hit = cc.Outcome(reinforced=True, reinforcer=cc.Reinforcer(1.0), meta={"component": 0})
    assert hit.reinforcer == cc.Reinforcer(1.0)
    assert hit.meta == {"component": 0}
    assert hit == cc.Outcome(True, cc.Reinforcer(1.0), {"component": 0})
    with pytest.raises(ValueError):
        cc.Outcome(reinforced=True)
    with pytest.raises(ValueError):
        cc.Outcome(reinforced=False, reinforcer=cc.Reinforcer(1.0))


def test_outcome_meta_keeps_python_value_types() -> None:
    meta = {"flag": True, "count": 3, "ratio": 0.5, "name": "a", "order": ["x", "y"]}
    out = cc.Outcome(meta=meta)
    assert out.meta == meta
    assert isinstance(out.meta["flag"], bool)
    assert isinstance(out.meta["count"], int) and not isinstance(out.meta["count"], bool)
    with pytest.raises(TypeError):
        cc.Outcome(meta={"bad": object()})


def test_step_returns_reinforcer_with_python_shape() -> None:
    s = cc.from_spec({"type": "FR", "params": {"n": 1}})
    out = s.step(4.0, cc.ResponseEvent(4.0))
    assert out == cc.Outcome(True, cc.Reinforcer(4.0, 1.0, "SR+"))


def test_response_event_keywords_and_equality() -> None:
    ev = cc.ResponseEvent(time=1.0, operandum="left")
    assert ev == cc.ResponseEvent(1.0, "left")
    assert cc.ResponseEvent(1.0).operandum == "main"

"""Tests for ``contingency_core.expand_respondent``.

Replays every respondent Tier A case under
``CONTINGENCY_RESPONDENT_CONFORMANCE_DIR`` or, when unset, the sibling
checkout ``../contingency-py/conformance/respondent/`` (the same directory
``tests/respondent_conformance.rs`` reads). Run inside a virtual
environment where the module was installed with
``maturin develop --features python``.
"""

from __future__ import annotations

import json
import math
import os
from pathlib import Path
from typing import Any

import pytest

import contingency_core as cc

HERE = Path(__file__).resolve().parent


def _respondent_dir() -> Path:
    env = os.environ.get("CONTINGENCY_RESPONDENT_CONFORMANCE_DIR")
    if env:
        return Path(env)
    return HERE.parents[1].parent / "contingency-py" / "conformance" / "respondent"


def _cases() -> list[Path]:
    root = _respondent_dir()
    return sorted(root.glob("*.json")) if root.is_dir() else []


def _same(expected: Any, observed: Any) -> bool:
    if isinstance(expected, bool) or isinstance(observed, bool):
        return expected is observed
    if isinstance(expected, (int, float)) and isinstance(observed, (int, float)):
        return math.isclose(expected, observed, rel_tol=0.0, abs_tol=1e-9)
    if isinstance(expected, list) and isinstance(observed, list):
        return len(expected) == len(observed) and all(
            _same(a, b) for a, b in zip(expected, observed)
        )
    if isinstance(expected, dict) and isinstance(observed, dict):
        return expected.keys() == observed.keys() and all(
            _same(v, observed[k]) for k, v in expected.items()
        )
    return bool(expected == observed)


def test_corpus_is_present() -> None:
    if not _cases():
        pytest.skip(f"respondent corpus not found at {_respondent_dir()}")


@pytest.mark.parametrize("path", _cases(), ids=lambda p: p.stem)
def test_case_replays(path: Path) -> None:
    case = json.loads(path.read_text(encoding="utf-8"))
    try:
        observed: dict[str, Any] = {
            "trials": cc.expand_respondent(case["spec"], case["options"])
        }
    except cc.UnresolvedError as exc:
        observed = {"error": "unresolved", "missing": exc.missing}
    assert _same(case["expected"], observed), observed


def test_unresolved_error_is_a_value_error() -> None:
    with pytest.raises(ValueError) as info:
        cc.expand_respondent({"type": "Extinction", "cs": "tone"})
    assert isinstance(info.value, cc.UnresolvedError)
    assert info.value.missing == "trials"


def test_spec_and_options_accept_json_strings() -> None:
    trials = cc.expand_respondent('{"type": "USOnly", "us": "shock"}', '{"trials": 2}')
    assert [t["us"] for t in trials] == ["shock", "shock"]
    assert trials[0]["time"]["us_on"] == 0.0


def test_non_tier_a_type_is_rejected() -> None:
    with pytest.raises(ValueError) as info:
        cc.expand_respondent({"type": "Blocking"}, {"trials": 1})
    assert not isinstance(info.value, cc.UnresolvedError)

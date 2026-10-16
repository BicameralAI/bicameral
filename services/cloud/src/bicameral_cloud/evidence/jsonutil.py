"""Small typed JSON narrowing helpers.

Collectors parse untrusted CLI output; these helpers narrow ``json``'s dynamically
typed values into concrete Python types without leaking ``Any`` into the rest of the
package (the repo runs mypy in strict mode).
"""

from __future__ import annotations

import json
from typing import cast


class JsonShapeError(ValueError):
    """Raised when parsed JSON does not match the expected shape."""


def loads(text: str) -> object:
    """Parse JSON text into a plainly typed ``object``."""
    return cast(object, json.loads(text))


def as_object(value: object, *, context: str) -> dict[str, object]:
    if not isinstance(value, dict):
        raise JsonShapeError(f"{context}: expected a JSON object")
    return cast(dict[str, object], value)


def as_array(value: object, *, context: str) -> list[object]:
    if not isinstance(value, list):
        raise JsonShapeError(f"{context}: expected a JSON array")
    return cast(list[object], value)


def as_str(value: object, *, context: str) -> str:
    if not isinstance(value, str):
        raise JsonShapeError(f"{context}: expected a string")
    return value


def get_str(mapping: dict[str, object], key: str, default: str = "") -> str:
    value = mapping.get(key, default)
    return value if isinstance(value, str) else default


def get_bool(mapping: dict[str, object], key: str, default: bool = False) -> bool:
    value = mapping.get(key, default)
    return value if isinstance(value, bool) else default


def get_object(mapping: dict[str, object], key: str) -> dict[str, object]:
    value = mapping.get(key)
    return value if isinstance(value, dict) else {}


def get_array(mapping: dict[str, object], key: str) -> list[object]:
    value = mapping.get(key)
    return value if isinstance(value, list) else []

"""RFC 8785 — JSON Canonicalization Scheme.

A2A v1.0 signs Agent Cards as JWS (RFC 7515) over a JCS-canonicalised payload,
so canonicalisation has to be the real thing: a signature computed over
"sorted-key compact JSON" verifies against nothing else in the ecosystem.

Scope note: floats are rejected rather than approximated. RFC 8785 mandates
ECMAScript `Number::toString` semantics for numbers, which is a meaningful
amount of machinery; nothing in an Agent Card or a support payload needs a
float, so refusing them is safer than emitting subtly non-canonical bytes.
"""
from __future__ import annotations

import json
import re

# RFC 8785 §3.2.2.2 — the two-character escapes, everything else below 0x20
# uses \u00xx form. Note that "/" is NOT escaped.
_ESCAPES = {
    0x08: "\\b",
    0x09: "\\t",
    0x0A: "\\n",
    0x0C: "\\f",
    0x0D: "\\r",
    0x22: '\\"',
    0x5C: "\\\\",
}


def _string(s: str) -> str:
    out = ['"']
    for ch in s:
        cp = ord(ch)
        if cp in _ESCAPES:
            out.append(_ESCAPES[cp])
        elif cp < 0x20:
            out.append(f"\\u{cp:04x}")
        else:
            out.append(ch)
    out.append('"')
    return "".join(out)


def _number(n: int) -> str:
    if isinstance(n, bool) or not isinstance(n, int):
        raise TypeError(f"JCS here supports int only, got {type(n).__name__}")
    return str(n)


def _sort_key(k: str):
    """RFC 8785 §3.2.3 — sort by UTF-16 code units, not by code point."""
    return k.encode("utf-16-be")


def _ser(v) -> str:
    if v is None:
        return "null"
    if v is True:
        return "true"
    if v is False:
        return "false"
    if isinstance(v, str):
        return _string(v)
    if isinstance(v, int):
        return _number(v)
    if isinstance(v, (list, tuple)):
        return "[" + ",".join(_ser(x) for x in v) + "]"
    if isinstance(v, dict):
        items = sorted(v.items(), key=lambda kv: _sort_key(kv[0]))
        return "{" + ",".join(f"{_string(k)}:{_ser(x)}" for k, x in items) + "}"
    raise TypeError(f"not JSON-canonicalisable: {type(v).__name__}")


def canonicalize(value) -> bytes:
    """Canonical UTF-8 bytes for `value` per RFC 8785."""
    return _ser(value).encode("utf-8")


def loads_canonical(raw: bytes | str):
    """Parse then re-canonicalise — used to verify a wire payload round-trips."""
    return canonicalize(json.loads(raw))

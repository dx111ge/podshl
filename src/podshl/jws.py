"""RFC 7515 JWS, detached payload, EdDSA (Ed25519) — as A2A v1.0 signs Agent Cards.

Detached because the payload is the card itself: the signature travels *inside*
the object it signs, so the payload is canonicalised (JCS) from the card with
its own `signatures` field removed.
"""
from __future__ import annotations

import base64

from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric.ed25519 import (
    Ed25519PrivateKey,
    Ed25519PublicKey,
)

from .jcs import canonicalize


def b64u(data: bytes) -> str:
    return base64.urlsafe_b64encode(data).rstrip(b"=").decode()


def b64u_decode(s: str) -> bytes:
    return base64.urlsafe_b64decode(s + "=" * (-len(s) % 4))


def public_jwk(key: Ed25519PrivateKey) -> dict:
    raw = key.public_key().public_bytes(
        serialization.Encoding.Raw, serialization.PublicFormat.Raw
    )
    return {"kty": "OKP", "crv": "Ed25519", "x": b64u(raw)}


def jwk_to_public(jwk: dict) -> Ed25519PublicKey:
    if jwk.get("kty") != "OKP" or jwk.get("crv") != "Ed25519":
        raise ValueError(f"unsupported JWK: {jwk.get('kty')}/{jwk.get('crv')}")
    return Ed25519PublicKey.from_public_bytes(b64u_decode(jwk["x"]))


def sign_detached(key: Ed25519PrivateKey, payload: dict, *, kid: str, extra: dict | None = None) -> dict:
    """Detached JWS over JCS(payload). Returns the A2A-shaped signature object."""
    protected = {"alg": "EdDSA", "kid": kid, **(extra or {})}
    p_b64 = b64u(canonicalize(protected))
    signing_input = f"{p_b64}.{b64u(canonicalize(payload))}".encode()
    return {"protected": p_b64, "signature": b64u(key.sign(signing_input))}


def verify_detached(jwk: dict, payload: dict, sig: dict) -> tuple[bool, dict]:
    """Returns (ok, protected_header). Never raises on bad input."""
    try:
        protected_raw = b64u_decode(sig["protected"])
        import json as _json

        protected = _json.loads(protected_raw)
        if protected.get("alg") != "EdDSA":
            return False, protected
        signing_input = f"{sig['protected']}.{b64u(canonicalize(payload))}".encode()
        jwk_to_public(jwk).verify(b64u_decode(sig["signature"]), signing_input)
        return True, protected
    except Exception:
        return False, {}


class KeyMissing(RuntimeError):
    """A signing key that is not there and may not be minted.

    Minting a key is a decision, not a convenience. A log signed under a key
    nobody pinned is a fork of the log: every head it issues verifies under a
    key no monitor holds, and a monitor comparing two heads cannot tell that
    from an attack. So a missing key is refused unless the caller says, in so
    many words, that creating one is intended.
    """


def load_or_create_key(path, *, create: bool = True) -> Ed25519PrivateKey:
    from pathlib import Path

    p = Path(path)
    if p.exists():
        return serialization.load_pem_private_key(p.read_bytes(), password=None)
    if not create:
        raise KeyMissing(
            f"no signing key at {p}, and creating one is not enabled. A key minted "
            f"silently on a fresh host would fork the log under an unpinned key; set "
            f"PODSHL_LOG_KEY_CREATE=1 only where that is what you mean, or restore "
            f"the key from where it was kept.")
    p.parent.mkdir(parents=True, exist_ok=True)
    key = Ed25519PrivateKey.generate()
    p.write_bytes(
        key.private_bytes(
            serialization.Encoding.PEM,
            serialization.PrivateFormat.PKCS8,
            serialization.NoEncryption(),
        )
    )
    p.chmod(0o600)
    return key

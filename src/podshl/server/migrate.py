"""Schema changes as reviewable SQL files, hash-pinned.

An applied migration cannot be edited afterwards: the runner records the SHA-256
of each file and refuses to continue if one it has already applied has changed.
Editing a migration that has run somewhere is how two databases with the same
version number end up with different schemas, and nothing later can detect it.

Alembic without SQLAlchemy models would contribute a directory layout and a
dependency; this is ninety lines.
"""
from __future__ import annotations

import hashlib
import sys
from pathlib import Path

from .db import connect

SQL_DIR = Path(__file__).parent / "sql"

BOOTSTRAP = """
CREATE TABLE IF NOT EXISTS schema_migration (
    version    text PRIMARY KEY,
    applied_at timestamptz NOT NULL DEFAULT now(),
    sha256     bytea NOT NULL
)
"""


def files() -> list[Path]:
    return sorted(SQL_DIR.glob("*.sql"))


def apply_all(verbose: bool = True) -> list[str]:
    applied: list[str] = []
    with connect() as conn:
        with conn.cursor() as cur:
            cur.execute(BOOTSTRAP)
            cur.execute("SELECT version, sha256 FROM schema_migration")
            seen = {r["version"]: bytes(r["sha256"]) for r in cur.fetchall()}
        conn.commit()

        for path in files():
            version = path.stem
            body = path.read_bytes()
            digest = hashlib.sha256(body).digest()

            if version in seen:
                if seen[version] != digest:
                    raise SystemExit(
                        f"{version} was applied and has since been edited. A migration that "
                        f"changes after it runs leaves two databases with the same version "
                        f"number and different schemas, and nothing later can tell. Write a "
                        f"new file instead."
                    )
                continue

            with conn.cursor() as cur:
                cur.execute(body.decode())
                cur.execute(
                    "INSERT INTO schema_migration (version, sha256) VALUES (%s, %s)",
                    (version, digest),
                )
            conn.commit()
            applied.append(version)
            if verbose:
                print(f"  applied {version}")

    if verbose:
        print(f"{len(applied)} applied, {len(seen)} already present")
    return applied


if __name__ == "__main__":
    try:
        apply_all()
    except Exception as e:  # noqa: BLE001 - the message is the product here
        print(f"migration failed: {e}", file=sys.stderr)
        raise SystemExit(1)

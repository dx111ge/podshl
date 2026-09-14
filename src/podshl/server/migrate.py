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


def digest(body: bytes) -> bytes:
    """The pin, over content rather than over bytes.

    Line endings are not part of what a migration *says*, and treating them as
    part of it cost an outage. The deployed tree was cut with `git archive` on
    Windows, where a checkout rewrites text files with CRLF; `.gitattributes`
    pinned `.sh`, `.yaml`, `Dockerfile` and the licences to LF and said nothing
    about `.sql`. So production recorded the CRLF hash of `0001_core`, a later
    deployment from Linux presented the LF one, and the runner correctly
    reported that an applied migration had been edited -- of a file whose git
    history has exactly one commit and which nobody had touched.

    Normalising is not a weakening. The guard exists so two databases cannot
    carry the same version number over different *schemas*, and `\r\n` for
    `\n` is the one difference that cannot change a schema. Everything else
    still refuses.
    """
    return hashlib.sha256(body.replace(b"\r\n", b"\n")).digest()


def _same_migration(recorded: bytes, body: bytes) -> bool:
    """Did this file produce `recorded`, under any line-ending convention?

    A record written before `digest` normalised is the SHA-256 of whatever bytes
    that deployment held -- LF on Linux, CRLF out of a Windows checkout. Both are
    accepted as the same migration, and the caller then rewrites the record to
    the normalised form so the question is asked once.
    """
    if recorded == digest(body):
        return True
    lf = body.replace(b"\r\n", b"\n")
    return recorded in (hashlib.sha256(lf).digest(),
                        hashlib.sha256(lf.replace(b"\n", b"\r\n")).digest())


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
            pin = digest(body)

            if version in seen:
                if seen[version] != pin:
                    if not _same_migration(seen[version], body):
                        raise SystemExit(
                            f"{version} was applied and has since been edited. A migration "
                            f"that changes after it runs leaves two databases with the same "
                            f"version number and different schemas, and nothing later can "
                            f"tell. Write a new file instead."
                        )
                    # Same SQL, recorded under the other line-ending convention.
                    # Converge the record rather than asking again every start.
                    with conn.cursor() as cur:
                        cur.execute("UPDATE schema_migration SET sha256 = %s WHERE version = %s",
                                    (pin, version))
                    conn.commit()
                    if verbose:
                        print(f"  repinned {version} (line endings only)")
                continue

            with conn.cursor() as cur:
                cur.execute(body.decode())
                cur.execute(
                    "INSERT INTO schema_migration (version, sha256) VALUES (%s, %s)",
                    (version, pin),
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

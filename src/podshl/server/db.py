"""A connection pool and a transaction, and nothing clever.

`tx()` exists because "the attestation row and the log entry announcing it are
one fact" has to be expressible as one `with`. A crash between them would leave
either a silent attestation or a lie in the log, and the log is the only thing
this server asks anyone to trust.

Raw SQL, no ORM. The parts of this schema that matter are exactly the parts an
ORM hides — a partial unique index that *is* the k-threshold, a CHECK that *is*
the English obligation, a `FOR UPDATE SKIP LOCKED` claim loop. Writing those
through a layer means writing the SQL anyway and then writing a second thing to
generate it. `jcs.py` is 81 lines of hand-written RFC 8785 for the same reason.
"""
from __future__ import annotations

from contextlib import contextmanager

import psycopg
from psycopg.rows import dict_row

from .config import DSN

_pool = None


def pool():
    global _pool
    if _pool is None:
        from psycopg_pool import ConnectionPool
        _pool = ConnectionPool(DSN, min_size=1, max_size=8, kwargs={"row_factory": dict_row})
    return _pool


@contextmanager
def tx():
    """One transaction. Commits on a clean exit, rolls back on anything else."""
    with pool().connection() as conn:
        with conn.transaction():
            yield conn


@contextmanager
def read():
    """A read-only transaction, enforced by the database rather than by a flag.

    `SET TRANSACTION READ ONLY` means an accidental INSERT on a query path is an
    error from Postgres, not a convention someone can forget. The distinction
    between a query and a report is the design's central one — a query touches
    no store at all — so it is worth making the database hold it.
    """
    with pool().connection() as conn:
        with conn.transaction():
            with conn.cursor() as cur:
                cur.execute("SET TRANSACTION READ ONLY")
            yield conn


def connect() -> psycopg.Connection:
    """A standalone connection, for the migration runner and for scripts that
    must not depend on a pool being up."""
    return psycopg.connect(DSN, row_factory=dict_row)

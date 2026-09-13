"""The k-threshold, counting the right quantity.

What was here before counted **submissions**: `_counts[sig] += 1` and
`len(_obs[product])`. One user pressing the button five times crossed a
threshold named for anonymity, and no amount of tuning k fixes that, because
the quantity being compared to k is the wrong quantity. The client had been
sending a per-vendor per-epoch pseudonym all along and no service read it.

`seen_key = HMAC(HKDF(epoch_salt, cluster_id), pseudonym)` — salted per cluster
and per epoch, so nothing joins on either axis, and once the epoch's salt is
gone it cannot be tested against any pseudonym by anyone, including us.

**The salt is not in the database.** A column would be in every base backup and
every WAL archive, so "discarded when the epoch rolls" would be true of the live
row and false of the archive — the promise honest only until the first restore.
"""
from __future__ import annotations

import hashlib
import hmac
import os
from datetime import datetime, timezone
from pathlib import Path

from .config import K_REPORTERS, SALT_DIR


def current_epoch(now: datetime | None = None) -> int:
    now = now or datetime.now(timezone.utc)
    return now.year * 100 + now.month


def _salt_path(epoch: int) -> Path:
    return SALT_DIR / f"{epoch}"


def salt_for(epoch: int) -> bytes:
    """The epoch's salt, created on first use. 0600, and it never leaves here."""
    SALT_DIR.mkdir(parents=True, exist_ok=True)
    path = _salt_path(epoch)
    if path.exists():
        return path.read_bytes()
    salt = os.urandom(32)
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, "wb") as fh:
        fh.write(salt)
    return salt


def cluster_key(salt: bytes, cluster_id: int) -> bytes:
    """HKDF-Expand, so one epoch salt yields an unrelated key per cluster.

    `SERVER.md` says "salted per cluster and per epoch". One salt plus a
    per-cluster expansion is cryptographically the same statement and does not
    require a salt row per cluster — of which there would be millions, each an
    object that has to be destroyed on time.
    """
    info = b"podshl-cluster\x00" + cluster_id.to_bytes(8, "big")
    return hmac.new(salt, info, hashlib.sha256).digest()


def seen_key(epoch: int, cluster_id: int, pseudonym: str) -> bytes:
    """16 bytes: far past collision relevance at these counts, and a smaller
    unique index on the hottest write path."""
    key = cluster_key(salt_for(epoch), cluster_id)
    return hmac.new(key, pseudonym.encode(), hashlib.sha256).digest()[:16]


def open_epoch(conn, epoch: int | None = None) -> int:
    epoch = epoch or current_epoch()
    salt = salt_for(epoch)
    with conn.cursor() as cur:
        cur.execute(
            "INSERT INTO epoch (epoch, salt_id, salt_sha256) VALUES (%s, %s, %s) "
            "ON CONFLICT (epoch) DO NOTHING",
            (epoch, str(epoch), hashlib.sha256(salt).digest()),
        )
    return epoch


def roll(conn, epoch: int) -> None:
    """Close an epoch and destroy its salt.

    This is the operation the whole anonymity claim rests on: afterwards a
    stored `seen_key` cannot be tested against any pseudonym — by anyone,
    including us, including under an order. It happens in one transaction with
    recording the destruction, so there is never a window in which the row says
    closed and the file is still there.
    """
    path = _salt_path(epoch)
    if path.exists():
        # Overwrite before unlinking: an unlink alone leaves the bytes.
        size = path.stat().st_size
        with open(path, "r+b") as fh:
            fh.write(os.urandom(size))
            fh.flush()
            os.fsync(fh.fileno())
        path.unlink()
    with conn.cursor() as cur:
        cur.execute("UPDATE epoch SET closed_at = now(), destroyed_at = now() WHERE epoch = %s",
                    (epoch,))


def roll_past(conn, now: datetime | None = None) -> list[int]:
    """Roll every epoch before the current month. Returns the epochs rolled.

    `roll` existed, was tested, and nothing in production called it — so every
    month's salt stayed on disk and the sentence this module opens with was
    true of the test suite only. The ingest worker calls this on every cycle.
    It also destroys a salt file with no epoch row behind it: the file is the
    thing that makes a pseudonym testable, whatever the table says.
    """
    current = current_epoch(now)
    rolled: list[int] = []
    with conn.cursor() as cur:
        cur.execute("SELECT epoch FROM epoch WHERE destroyed_at IS NULL AND epoch < %s "
                    "ORDER BY epoch", (current,))
        due = {r["epoch"] for r in cur.fetchall()}
    if SALT_DIR.is_dir():
        for p in SALT_DIR.iterdir():
            if p.name.isdigit() and int(p.name) < current:
                due.add(int(p.name))
    for epoch in sorted(due):
        roll(conn, epoch)
        rolled.append(epoch)
    return rolled


def record_observation(conn, cluster_id: int, pseudonym: str, *, epoch: int | None = None,
                       model_class: str = "unknown", ux_severity: str | None = None,
                       outcome: str | None = None, observed: dict | None = None,
                       stated: dict | None = None,
                       description: str | None = None,
                       description_consent: dict | None = None) -> bool:
    """Record one report. Returns whether it was new for this pseudonym.

    A second report from the same pseudonym in the same epoch is not a second
    observation — the unique index says so, and this returns False rather than
    pretending otherwise. The counter reads "47 reported", never "47 occurred".
    """
    import json

    epoch = open_epoch(conn, epoch)
    key = seen_key(epoch, cluster_id, pseudonym)
    with conn.cursor() as cur:
        cur.execute(
            "INSERT INTO observation (cluster_id, epoch, model_class, ux_severity, outcome, "
            "  observed, stated, seen_key, description, description_consent) "
            "VALUES (%s, %s, %s, %s, %s, %s, %s, %s, %s, %s) "
            "ON CONFLICT (cluster_id, epoch, seen_key) DO NOTHING RETURNING id",
            (cluster_id, epoch, model_class, ux_severity, outcome,
             json.dumps(observed or {}), json.dumps(stated or {}), key, description,
             json.dumps(description_consent) if description_consent else None),
        )
        inserted = cur.fetchone() is not None

        # `reports_total` counts submissions and says so. The reporter counts
        # only move when the pseudonym was new.
        cur.execute("UPDATE cluster SET reports_total = reports_total + 1, last_epoch = %s "
                    "WHERE id = %s", (epoch, cluster_id))
        if inserted:
            cur.execute(
                "UPDATE cluster SET reporters_this_epoch = ("
                "  SELECT count(DISTINCT seen_key) FROM observation "
                "  WHERE cluster_id = %s AND epoch = %s) WHERE id = %s",
                (cluster_id, epoch, cluster_id),
            )
            cur.execute(
                "UPDATE cluster SET peak_epoch_reporters = GREATEST(peak_epoch_reporters, "
                "  reporters_this_epoch) WHERE id = %s", (cluster_id,))
    return inserted


def reporters(conn, cluster_id: int, epoch: int | None = None) -> int:
    """Distinct pseudonyms. The only quantity the threshold is applied to."""
    with conn.cursor() as cur:
        if epoch is None:
            cur.execute("SELECT peak_epoch_reporters AS n FROM cluster WHERE id = %s",
                        (cluster_id,))
        else:
            cur.execute("SELECT count(DISTINCT seen_key) AS n FROM observation "
                        "WHERE cluster_id = %s AND epoch = %s", (cluster_id, epoch))
        row = cur.fetchone()
    return row["n"] if row else 0


def surfaceable(conn, cluster_id: int) -> bool:
    """Below k distinct pseudonyms, a cluster is not shown to anyone — not to
    the vendor, not to the operator. A rare configuration is identifying."""
    return reporters(conn, cluster_id) >= K_REPORTERS

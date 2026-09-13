"""Every knob that differs between the demo and production, in one place.

Read here and nowhere else. A setting fetched from the environment at the point
of use is a setting nobody can enumerate, and this server's whole argument is
that what it holds can be stated exactly.
"""
from __future__ import annotations

import os
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
VAR = ROOT / "var"

# The cluster mise puts in var/. A unix socket, and `listen_addresses` empty, so
# the development database is not reachable over the network at all.
DSN = os.environ.get("PODSHL_DSN", f"host={VAR} port=5433 user=podshl dbname=podshl")

# Where the epoch salts live. Deliberately NOT in the database: a column would
# be in every base backup and every WAL archive, so "discarded when the epoch
# rolls" would be true of the live row and false of the archive — the promise
# honest only until the first restore.
SALT_DIR = Path(os.environ.get("PODSHL_SALT_DIR", VAR / "salt"))

# The log's signing key. One key, one log; rotation is itself a log entry.
LOG_KEY = Path(os.environ.get("PODSHL_LOG_KEY", VAR / "log.ed25519"))

# Whether a missing log key may be minted on first use. Off unless said so: a
# production host that comes up without its key must refuse to sign rather than
# quietly start a second log under a key nobody pinned — that is a fork, and a
# monitor comparing heads has no way to tell it from an attack. The development
# container sets this because its key lives in a bind mount that starts empty.
LOG_KEY_CREATE = os.environ.get("PODSHL_LOG_KEY_CREATE") == "1"

# The operator's own listener. Every route on it requires this bearer token,
# compared in constant time; unset, the listener answers 503 to everything, so
# an operator view with no credential configured is unreachable rather than
# open. Loopback binding is the first wall, this is the second — one process
# on the same host reaching the port is not the same as the operator.
OPS_TOKEN = os.environ.get("PODSHL_OPS_TOKEN") or None

# `Host` values the operator listener answers to. A browser on the operator's
# machine sends one of these; a DNS name that resolves to loopback — the shape a
# rebinding attack takes — sends something else and is refused before the token
# is looked at. Extended, never replaced, by `PODSHL_OPS_HOSTS` (comma-separated).
OPS_HOSTS = frozenset({"127.0.0.1:8726", "localhost:8726"} | {
    h.strip().lower() for h in os.environ.get("PODSHL_OPS_HOSTS", "").split(",") if h.strip()})

# Below this many *distinct pseudonyms* nothing is surfaced to anyone. The
# quantity matters more than the number: counting submissions cannot tell five
# people from one person reporting five times.
K_REPORTERS = int(os.environ.get("PODSHL_K", "5"))

# A year-month, like the client's. A vendor's view of a client expires on its own.
EPOCH_DAYS = 30

# Development only, and off unless deliberately switched on. The crawler follows
# URLs an attacker chose, so refusing loopback and private addresses is what
# stops it becoming a request-forgery engine aimed at our own network. The local
# counterparty lives on 127.0.0.1, so the demo needs an exception — and an
# exception that defaults to on, or that lives only in a comment, is not one.
# `/` reports when this is enabled, so it cannot be true in production quietly.
ALLOW_LOOPBACK = os.environ.get("PODSHL_ALLOW_LOOPBACK") == "1"

# Anchor liveness. One failed check is nothing; these are the grades.
STALE_AFTER_DAYS = 14
UNKNOWN_AFTER_DAYS = 90

# The operator's own identity, for the imprint every public service here owes.
#
# Deliberately unset by default, and deliberately *not* derived from anything.
# A hostname, a `Host` header or a placeholder name would each produce a page
# that looks like an imprint and is not one, and an unmet legal duty must not
# render as fine. `/imprint` answers 503 until all three are configured and says
# which are missing.
IMPRINT_NAME = os.environ.get("PODSHL_IMPRINT_NAME")
IMPRINT_ADDRESS = os.environ.get("PODSHL_IMPRINT_ADDRESS")
IMPRINT_EMAIL = os.environ.get("PODSHL_IMPRINT_EMAIL")
IMPRINT_REGISTER = os.environ.get("PODSHL_IMPRINT_REGISTER")   # HRB, USt-IdNr — optional
IMPRINT_COMPLETE = bool(IMPRINT_NAME and IMPRINT_ADDRESS and IMPRINT_EMAIL)

# `security.txt` is served only with a contact *and* an expiry, because RFC 9116
# requires the second and a security contact nobody answers is worse than none.
SECURITY_CONTACT = os.environ.get("PODSHL_SECURITY_CONTACT")
SECURITY_EXPIRES = os.environ.get("PODSHL_SECURITY_EXPIRES")

# Where a notice-and-action complaint reaches a person. Shown on `/notice`.
NOTICE_CONTACT = os.environ.get("PODSHL_NOTICE_CONTACT")

# Where this project's own source lives. Unset until there is one to point at —
# a page claiming to be open source with a link that 404s is worse than a page
# that says the repository is not up yet.
SOURCE_URL = os.environ.get("PODSHL_SOURCE_URL")


def spell_out(address: str | None) -> str | None:
    """An email address split so that neither the page nor the API ever carries
    the literal form.

    An imprint has to be published; it does not have to be published *to a
    harvester*. Address-scrapers read HTML and JSON and look for `local@domain`,
    and almost none of them reassemble parts or execute a page's script. So the
    served form is `name (at) host (dot) tld` — readable by a person with no
    JavaScript, which is what "leicht erkennbar und unmittelbar erreichbar" (easily
    recognisable and directly reachable, § 5 DDG)
    actually requires, and useless to a regular expression.

    Deliberately not an image and not JavaScript-only: an imprint a person
    cannot read without running code is not an imprint.
    """
    if not address or "@" not in address:
        return address
    local, _, domain = address.partition("@")
    return f"{local} (at) {domain.replace('.', ' (dot) ')}"

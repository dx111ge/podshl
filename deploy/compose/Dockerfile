# The operator, for a real host. Not docker/Dockerfile: that image is the
# development side — Rust, Chromium, the counterparty, a database inside it, the
# source bind-mounted — and every one of those is something a public machine
# should not carry.
#
# What is in here is what the three processes import and read: the Python
# packages, `src/`, and `spec/` (the vocabulary and the schema are served and
# checked against at runtime). Nothing else from the checkout.
FROM python:3.12-slim-trixie

ENV PYTHONUNBUFFERED=1 \
    PYTHONDONTWRITEBYTECODE=1 \
    PYTHONPATH=/opt/podshl/src \
    PIP_NO_CACHE_DIR=1 \
    PIP_DISABLE_PIP_VERSION_CHECK=1

COPY docker/requirements-server.txt /tmp/requirements-server.txt
RUN pip install -r /tmp/requirements-server.txt && rm /tmp/requirements-server.txt

# A fixed uid, so the state volume's ownership means the same thing after a
# rebuild. The directory exists in the image with that owner and 0700, which is
# what Docker copies onto a fresh named volume mounted over it.
RUN groupadd --system --gid 10001 podshl \
 && useradd --system --uid 10001 --gid podshl --home /var/lib/podshl --shell /usr/sbin/nologin podshl \
 && install -d -o podshl -g podshl -m 0700 /var/lib/podshl /var/lib/podshl/salt

WORKDIR /opt/podshl
COPY --chown=root:root src/ /opt/podshl/src/
COPY --chown=root:root spec/ /opt/podshl/spec/

# Where the development defaults would put these is a checkout's var/, which
# does not exist here. Said explicitly so nothing lands in the image layer.
ENV PODSHL_SALT_DIR=/var/lib/podshl/salt \
    PODSHL_LOG_KEY=/var/lib/podshl/log.ed25519

USER podshl

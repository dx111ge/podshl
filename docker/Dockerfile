# The Linux side of PODSHL, in one image: the store, the operator server, and
# the counterparty it is tested against.
#
# One container rather than several on purpose. `run_testcases.py` reaches the
# vendor, the plain website, the index and the OSS project over loopback, and
# `PODSHL_ALLOW_LOOPBACK` exists precisely so the crawler may follow them there.
# Split across containers, "loopback" stops meaning what those cases mean by it.
FROM postgres:18-trixie

# uv, pinned. The image installs the Python that `mise.toml` pins rather than
# Debian's 3.13, so the container and a local checkout run the same interpreter.
COPY --from=ghcr.io/astral-sh/uv:0.12.9 /uv /usr/local/bin/uv

ENV UV_PYTHON_INSTALL_DIR=/opt/python \
    VIRTUAL_ENV=/opt/venv \
    PATH=/opt/venv/bin:$PATH \
    PYTHONPATH=/app/src \
    PYTHONUNBUFFERED=1

RUN uv python install 3.12

COPY docker/requirements-server.txt /tmp/requirements-server.txt
RUN uv venv --python 3.12 /opt/venv \
 && uv pip install --no-cache -r /tmp/requirements-server.txt

# The cluster does not live in var/ here. var/ is a bind mount from the host,
# and initdb refuses a directory whose ownership it cannot control — so PGDATA
# is a named volume and only the keys, salts and logs stay on the mount where
# the host can read them.
ENV PGDATA=/pgdata \
    PGPORT=5433

# `listen_addresses` stays empty, as it is on a developer's machine: the store
# is reachable over a unix socket inside this container and from nowhere else.
# The socket is inside PGDATA — the shared volume — so a second container can
# see that one is already serving and does not start a rival postmaster over
# the same data directory.
ENV PODSHL_DSN="host=/pgdata port=5433 user=podshl dbname=podshl"

# Rust, and the system libraries Tauri and keyring link against.
#
# The client's cases are not a separate suite: `run_testcases.py` shells out to
# `cargo` for 83 of its 185 cases, and those Rust tests in turn need the
# counterparty answering on loopback. Both halves have to be in one image, or
# no single command can answer for the whole list — which is the property the
# suite exists to have.
#
# `git` and `fakeroot` are for the release: the task stamps the commit into
# PLATFORM.md, and the deb bundler needs fakeroot.
#
# `nodejs` is a *test* dependency and not a build step — the window still has
# none, which is the point of it being one file. It is here because the window
# is a JavaScript program and only a JavaScript parser can tell you it parses.
# W3 exists because a syntax error there kills the whole interface while every
# other check, all of which read the file as text, keeps passing.
#
# Xvfb, imagemagick and chromium are so the window and the operator's pages can
# be opened and photographed with no desktop involved. The pages need a real
# browser specifically: their CSP names a hash per inline script, and a wrong
# hash blocks the script silently — the page renders with its layout drawn and
# nothing filled in, which no check that reads the file as text can see. Every panel here has a contract test and for a long time
# nobody had actually looked at one, because looking needed a free screen and
# somebody's attention. It should need neither.
RUN apt-get update && apt-get install -y --no-install-recommends \
      build-essential curl ca-certificates pkg-config file \
      xvfb x11-utils imagemagick nodejs chromium \
      libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev \
      libayatana-appindicator3-dev libxdo-dev \
      libsecret-1-dev \
      git fakeroot \
 && rm -rf /var/lib/apt/lists/*

ENV RUSTUP_HOME=/opt/rustup \
    CARGO_HOME=/opt/cargo \
    PATH=/opt/cargo/bin:$PATH
RUN curl -fsSL https://sh.rustup.rs \
  | sh -s -- -y --no-modify-path --profile minimal --default-toolchain 1.94.0
# `minimal` leaves out rustfmt and clippy, and `scripts/ci/ci.sh` gates on both.
# When it only reported them it once printed "0 places differ" for a tree with
# 588 of them, because `cargo fmt` errored, the grep matched nothing and
# `|| true` turned the error into a comfortable number. A check that cannot fail
# is the thing this project keeps taking out.
RUN rustup component add rustfmt clippy

# The bundler, and mise. `mise run release` is the release contract — it writes
# PLATFORM.md and SHA256SUMS alongside the binary — so the container runs that
# task rather than a second copy of it that could drift from it.
RUN cargo install tauri-cli --locked --version '^2'
RUN curl -fsSL https://mise.run | MISE_INSTALL_PATH=/usr/local/bin/mise sh

# The image already carries python, uv and postgres; mise must use those rather
# than provision its own. postgres in particular has no Linux binary in the
# plugin and would compile from source.
ENV MISE_DISABLE_TOOLS=python,uv,postgres \
    MISE_TRUSTED_CONFIG_PATHS=/app \
    MISE_YES=1

# Not client-rs/target. That path is inside the bind mount, where it would both
# crawl over the host filesystem and collide with the host's own build — the
# target triples differ, and cargo would rebuild the world on every switch.
ENV CARGO_TARGET_DIR=/cargo-target

WORKDIR /app
COPY docker/entrypoint.sh /usr/local/bin/podshl-entrypoint
RUN chmod +x /usr/local/bin/podshl-entrypoint

EXPOSE 8721 8722 8723 8725 8726 8727

ENTRYPOINT ["/usr/local/bin/podshl-entrypoint"]
CMD ["all"]

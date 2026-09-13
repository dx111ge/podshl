# Running the operator on a host

**With Docker Compose, see [`compose/`](compose/README.md)** — the deployment
chosen for the first host, verified end to end on 2026-09-13. The systemd units
below describe the same thing without containers.

Three processes and a database. The files here are what a small EU VPS needs
(`SERVER.md`, *Hosting abroad moves nothing*, for why EU; a free PaaS sleeps and
loses its disk, and this server's log key has to survive); nothing in them is
specific to a provider.

| Unit | Listens | Public? |
|---|---|---|
| `podshl-server.service` | `127.0.0.1:8725` | **Yes, via the reverse proxy only** — TLS terminates there |
| `podshl-ops.service` | `127.0.0.1:8726` | **Never.** Reach it with `ssh -L 8726:127.0.0.1:8726 <host>`. Every request must carry `PODSHL_OPS_TOKEN` as a bearer token; with none configured the listener answers `503` to everything |
| `podshl-ingest.service` | nothing | Outbound only: fetches what projects publish |

## Install

```bash
useradd --system --home /var/lib/podshl --shell /usr/sbin/nologin podshl
install -d -o podshl -g podshl -m 0700 /var/lib/podshl /var/lib/podshl/salt
# the code and a venv with docker/requirements-server.txt in /opt/podshl
install -d -m 0750 -o root -g podshl /etc/podshl
install -m 0640 -o root -g podshl /opt/podshl/.env.example /etc/podshl/env   # then edit it
cp /opt/podshl/deploy/systemd/*.service /etc/systemd/system/
sudo -u podshl env $(grep -v '^#' /etc/podshl/env | xargs) \
  /opt/podshl/venv/bin/python -m podshl.server.migrate
systemctl enable --now podshl-server podshl-ops podshl-ingest
```

`/etc/podshl/env` takes the imprint and contact settings `.env.example` lists,
and these, which the development defaults would otherwise put in a checkout's
`var/`:

```
PYTHONPATH=/opt/podshl/src
PODSHL_DSN=host=/run/postgresql dbname=podshl user=podshl
PODSHL_SALT_DIR=/var/lib/podshl/salt
PODSHL_LOG_KEY=/var/lib/podshl/log.ed25519
PODSHL_OPS_TOKEN=<long random string>     # without it the ops view answers 503 to everything
```

**Never** `PODSHL_ALLOW_LOOPBACK` in production: it exists so the development
crawler can reach its own counterparty, and `GET /` reports when it is on.

**The log key is not created for you.** With `PODSHL_LOG_KEY` pointing at
nothing the server refuses to sign a head, rather than minting a key on first
start and quietly opening a second log that no monitor pinned — which is a
fork. Generating one is an explicit opt-in, `PODSHL_LOG_KEY_CREATE=1`, set for
exactly the one start that is meant to create it and then removed; the backup
below is the other half of that decision.

## The reverse proxy

The server does no TLS and no rate limiting of its own; both are the proxy's
job, and a deployment without a proxy in front of `:8725` has neither. What
the proxy must do, as a Caddyfile — nginx expresses the same things:

```
podshl.example.org {
    encode zstd gzip

    # Sizes: a report with 16 KiB of free text is the largest honest body.
    request_body {
        max_size 256KB
    }

    # The routes a stranger can write to. The server counts people, not
    # requests, so this is what stands between it and a flood.
    @writes path /report /claim/* /notice
    rate_limit @writes {
        zone writes {
            key    {remote_host}
            events 30
            window 1m
        }
    }

    # What /privacy says is logged, and nothing more — the same block as
    # compose/Caddyfile: the first path segment only (a host or a log index
    # names a project), no request headers (X-Podshl-Claim is a credential),
    # the address cut to its network, seven days.
    log {
        output file /var/log/caddy/podshl.log {
            roll_size 10MiB
            roll_keep 5
            roll_keep_for 168h
        }
        format filter {
            request>remote_ip ip_mask 16 32
            request>client_ip ip_mask 16 32
            request>uri regexp ^(/[^/?]*).*$ $1
            request>headers delete
        }
    }

    reverse_proxy 127.0.0.1:8725
}
```

Nothing proxies `:8726`. It binds loopback, needs `PODSHL_OPS_TOKEN`, and is
reached over `ssh -L`; a proxy block for it is a mistake, however
well-authenticated.

`rate_limit` is a Caddy plugin rather than a stock directive; with stock nginx
the same is `client_max_body_size 256k`, a `limit_req_zone` keyed on
`$binary_remote_addr` applied to those three locations, and a `map` on
`$request_uri` that blanks the host segment before `access_log` sees it.

## The two things that must survive the machine

* **The log's signing key.** Lose it and the log cannot sign another head; the
  honest way on is a new log with a new key, announced — and every monitor
  pinned to the old one sees a fork. Back it up offline, once.
* **The database, with one thing understood about it.** Most of it is public
  mirrors, counters and the log, and a backup of those is not a liability. The
  `observation` table is different: it holds what people sent — the coarsened
  readings verbatim, the answers they picked from a list, and free text where
  they agreed to it — with no timestamp finer than the month. A backup of the
  database is a backup of that too, and it lives as long as the backup does.
  Keep it under the same care as the key. The epoch salts are *meant* to be
  destroyed when the epoch rolls and belong in no backup.

**Rotating the log key is not automated.** One key, one log; when a key has to
change, the change is itself a log entry — a `log_policy` entry, which is a
kind the log already accepts, naming the new key so a monitor that pinned the
old head can see the change stated rather than see a fork. Nothing writes that
entry for you today: it is done by hand, once, with the server stopped, and
announced.

The ingest worker is safe to restart at any time: a cycle runs in one
transaction per source, a failed cycle is retried, and a source is claimed with
a lease so two workers never fetch the same one.

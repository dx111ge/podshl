# The operator with Docker Compose

The same deployment as [`../README.md`](../README.md) — a public server behind
TLS, the ops view on loopback only, the ingest worker, Postgres — as five
containers on one Linux host. Chosen 2026-09-13 for a STRATO VPS; nothing here
is specific to STRATO.

| Service | Reaches the network | Notes |
|---|---|---|
| `caddy` | **80, 443** | TLS (Let's Encrypt), 256 KB body limit, 30 writes a minute per address on `/report`, `/claim/*`, `/notice`, an access log with addresses masked and project names blanked |
| `server` | no port published | `:8725` inside; out to the internet for claim checks and GLEIF |
| `ops` | `127.0.0.1:8726` on the host | `ssh -L 8726:127.0.0.1:8726 <host>`; bearer token from `ops.env`, read by this container only |
| `ingest` | outbound only | the crawler refuses private and loopback addresses before it connects, so the `db` name resolves and is still refused |
| `db` | nothing | on an internal network with no route out; scram passwords |
| `migrate` | nothing | runs once per `up`, the others wait for it |

Every operator container runs as uid 10001 on a read-only root filesystem with
all capabilities dropped. Measured on a first start: about 170 MB of memory for
the whole stack at idle.

## What was verified, 2026-09-13

On a development machine, under another project name and other ports:
images built; migrations applied on a fresh volume; **with no key, `/log/sth`
and `/index` fail with `KeyMissing`** rather than start a second log; the key
created once with `PODSHL_LOG_KEY_CREATE=1`, `0600` and owned by the service
user; `/`, `/index`, `/imprint` served through Caddy; `loopback_fetching_enabled`
false; `server` and `db` not published; ops `401` without the token and `200`
with it; a 300 KB body `413`; the 31st write in a minute `429`; `/dashboard/<host>`
and `/mirror/<host>/…` logged as `/dashboard/-` and `/mirror/-/…`, addresses
masked, no referrer. `backup.sh`, then `down -v`, then `restore.sh`: the same
log id, the database back with all 17 migrations present, and a second restore
refused. Not verified: Let's Encrypt, which needs a public name.

## On the host

**1. The machine.** A VPS with Debian 13 or Ubuntu 24.04, 2 GB of memory or more
(STRATO **VPS S Linux**: 1 vCore, 2 GB, 60 GB NVMe, location Germany). The stack
idles at about 170 MB; the 2 GB are for building the images on the host, where
compiling Caddy with its plugin is the largest step. STRATO includes no
backups — `backup.sh` is the backup. Log in with an SSH key, turn
password logins off, and install Docker Engine from Docker's own repository
(docs.docker.com/engine/install) — the distribution's package is usually old.

Firewall: 22, 80 and 443 in. Note that Docker writes its own iptables rules and
a published port is reachable whatever `ufw` says — which is why the only
published ports here are Caddy's two and a loopback one.

**2. The code.** The repository is not public yet, so copy the tree rather than
clone it:

```bash
# on your machine, in the checkout
git archive --format=tar HEAD | ssh <host> 'sudo mkdir -p /opt/podshl && sudo tar -x -C /opt/podshl'
```

**3. The settings.**

```bash
cd /opt/podshl/deploy/compose
sudo cp env.example .env && sudo cp ops.env.example ops.env
sudo chmod 600 .env ops.env
openssl rand -base64 33   # twice: POSTGRES_PASSWORD in .env, PODSHL_OPS_TOKEN in ops.env
sudo nano .env            # the imprint, and PODSHL_SITE once a name points here
```

Leave `PODSHL_SITE` empty until the DNS record exists: Caddy then serves plain
HTTP on port 80, which is enough to check the machine and not enough to be
public.

**4. The key — once, and only on this first start.**

```bash
sudo docker compose up -d --build --wait
sudo docker compose run --rm --no-deps -e PODSHL_LOG_KEY_CREATE=1 server \
  python -c "from podshl.server import sth; sth.key(); print('log id', sth.log_id())"
sudo docker compose restart server ingest ops
curl -s http://<ip>/log/sth
```

Never set `PODSHL_LOG_KEY_CREATE` in `.env`. A later start that has lost its key
must refuse to sign — that is the difference between an outage and a forked log.

**5. Back it up, now, before anything is public.**

```bash
sudo ./backup.sh /root/podshl-backups
```

Copy that directory off the machine. `log_key.json` in it is the public key a
client installer is built with.

**6. The name.** At the registrar (STRATO: *Domains → DNS → A/AAAA record*),
point the chosen name at the server's address, set `PODSHL_SITE` to that name,
and `sudo docker compose up -d caddy`. The certificate is fetched on the first
request; `curl -I https://<name>/` should answer `200`.

**7. The client.** On Windows:

```powershell
pwsh scripts\build\build_windows_installer.ps1 -ServerUrl https://<name> -LogKey <backup>\log_key.json
```

`-IndexUrl` names the responsiveness index (`:8723` in development), which is a
demo with a reset endpoint and is **not** part of this deployment; the published
path does not use it.

## At home, for the beta

Decided 2026-09-13: the beta runs on the operator's own Ubuntu machine at home,
beside other services, behind a FritzBox, reached as `sdota.de` through DynDNS. A VPS is the next step once people outside
the operator's circle use it — `backup.sh` there, `restore.sh` on the VPS, DNS
changed, and no client installer has to be rebuilt, because clients know the
name and the key and neither moves.

What is different from a VPS, in the order it is done:

**a. The wall between the containers and the home network — before the first
`up`.** Without it, verified on that machine: a container reached other services
on the LAN, the router's web interface, and ports on the host itself. With it, verified:
all three time out; the internet, DNS and the containers' own database still
work; Caddy still serves; applying twice changes nothing; `remove` restores the
previous state.

```bash
sudo cp /opt/podshl/deploy/compose/podshl-firewall.service /etc/systemd/system/
sudo systemctl enable --now podshl-firewall
sudo /opt/podshl/deploy/compose/firewall.sh status
```

It covers the two compose networks (172.30.0.0/16) and nothing else on the
machine. What it cannot do is contain an escape from a container to the host;
that is what the non-root, read-only, capability-free containers are for, and
it is the risk accepted by sharing the machine with other services for a beta.

**b. Ports 80 and 443 free on the host.** `sudo ss -ltnp '( sport = :80 or sport = :443 )'`
must list nothing before `up`.

**c. The FritzBox.** *Internet → Freigaben → Portfreigaben*: this machine, TCP 80
and TCP 443 (UDP 443 as well for HTTP/3). Nothing else — never 8726, never SSH
for this.

**d. The name.** The beta uses `sdota.de` and `www.sdota.de`, which the DynDNS
update already points at the home address (`PODSHL_SITE=sdota.de, www.sdota.de`).
A subdomain would work the same way as a **CNAME to the DynDNS name**, with any
**AAAA record deleted** that points elsewhere — a visitor or Let's Encrypt
arriving over IPv6 would otherwise reach the registrar's placeholder. DNS changes
can take up to 24 hours. Until the name resolves to the home address, keep
`PODSHL_SITE` empty and check on the LAN IP.

**e. Backups to another device.** Not to this machine: a backup on the disk it
backs up is gone with that disk. Copy the directory `backup.sh` prints to a
different computer (`scp -r`), and keep `log_key.json` from it for the client
installer.

**f. Encrypt the disk the backups land on — for production, not for a first
beta.** Deliberately in that order, because saying it earlier gets it done
badly or not at all. A backup set is `log.ed25519`, which is the log's signing
key, and `podshl.dump`, which holds the `observation` table — what people
actually sent. Together they are the two most sensitive artefacts this project
produces, and after step *e* they live on a machine chosen for having space
rather than for being looked after.

What this is and is not:

* It is **at-rest only** — theft of the box, and the disk being returned, sold
  or scrapped years later with a signing key still on it. Disposal is the one
  that actually happens.
* It is **not** about the disk being removable. A USB disk bolted to a desk and
  an internal one have the same threat model; what matters is who can reach the
  platters.
* It is **not** a substitute for keeping the key off the operator's own disk.
  That is step *e* and it is about surviving a failure, not about secrecy.

Whole-disk is enough — LUKS, BitLocker, FileVault, whatever the backup host
already has. The thing to avoid is encrypting one copy and not another: the
exposure is the least-protected copy, so count them first. And whatever is
decided, **wipe the disk when it is retired**, which is the step that is
forgotten precisely because it happens years after anybody thought about this.

A key whose confidentiality is gone cannot be repaired by rotating it quietly:
clients pin `log_key.json` at build time, so replacing it means a new client
build reaching everybody who has one.

## Routine

| | |
|---|---|
| Update | `deploy/compose/update.sh <user@host>` — backs up, `git archive`s **HEAD** (not the working tree, so a deployment is always a commit somebody can name) over `/opt/podshl`, then `sudo docker compose up -d --build`. Migrations run first; the key volume is untouched. It refuses to deploy if the backup fails, because a migration without a way back is not a deployment |
| Staging first | `update.sh <user@host> --staging` — a second operator at `/opt/podshl-staging`, loopback `:8735`, no Caddy and no backup, because its database is meant to be replaceable. Run it there first whenever a migration is involved: it exists because a CRLF byte in one took the live operator down on 2026-09-13 |
| Which code is live | `curl -s https://<host>/ -H 'Accept: application/json'` — `version` is what `git describe --tags --always` said when the image was built, passed in by `update.sh` as a build argument. It is also in the footer of every page. If it names something older than the deployment just printed, the build argument did not arrive and the image was reused |
| Back up | `backup.sh` from cron, daily, copied off the machine. Salts are excluded on purpose |
| Operator view | `ssh -L 8726:127.0.0.1:8726 <host>`, then `http://127.0.0.1:8726/` with the token |
| Walk the client | `scripts/build/build_client.sh <operator>` then `scripts/walk/walk_client.py` — drives the real binary through the flow the window walks, follows the decision tree's questions, and names the step where it stops |
| Client log | `~/.local/state/podshl/client.log` — one line per operator call, anonymised. `PODSHL_LOG=0` turns it off |
| Logs | `sudo docker compose logs -f server ingest`; the access log is in the `caddy-logs` volume |
| Restore | on empty volumes only: `restore.sh <backup>` refuses a volume that holds a key or a database with tables |

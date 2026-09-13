"""Five real failures arriving from users' machines, and one that stays hidden.

Every report goes through `POST /report` on the running server — the same route
a client posts to — so what lands on the dashboard is counted, epoch-salted and
k-anonymised by the shipped code rather than written into the tables.

The last scenario is the point of the exercise as much as the other five: three
reporters is below the floor of five, so it is *counted and not shown*. A
dashboard that displayed it would be publishing a constellation rare enough to
identify the person who reported it.

**engram's version is a reading now, not an answer.** These reports used to
carry `engram.release` in `stated` — a version somebody picked from a list — so
the dashboard filed it under facts people typed, and an outcome that turned on
it said nothing about engram's rule. The manifest asks engram itself
(`program_version`), so it travels in `observed` — exactly, `1.2.2`, because it is
engram's own version and the last component is the fix that shipped. "I built it myself" is not a version and has no reading; that
reporter simply has none.

Two of the people in the Ollama scenario send the lines Ollama logged, the way
the client sends them: anonymised on their machine — the address, the user
name, the time replaced — shown to them, and attached only with their consent.
Those are what appears on the dashboard under "in their own words".
"""
from __future__ import annotations

import json
import urllib.request

SERVER = "http://127.0.0.1:8725"
SUBJECT = "engram.localhost"

#: What the client's anonymiser leaves of an Ollama log after a person cut it
#: down to the lines around the failure.
OLLAMA_LOG = (
    "ollama.log: <time> level=INFO source=routes.go:1256 msg=\"Listening on <ip>:11434 (version 0.3.14)\"\n"
    "<time> level=INFO source=images.go:753 msg=\"total blobs: 0\"\n"
    "[GIN] <time> | 404 |     412.3µs |       <ip> | POST     \"/api/chat\"\n"
    "<time> level=ERROR msg=\"model 'gemma4:e4b' not found, try pulling it first\""
)

SCENARIOS = [
    dict(
        note="Intel Mac — there is no build at all",
        reporters=6,
        observed={"os.name": "macos", "os.arch": "x86_64", "os.kernel": "23.6"},
        stated={"engram.symptom": "the binary will not start at all"},
        outcome="resolved", ux_severity="high", model_class="none",
    ),
    dict(
        note="ARM64 Linux — the x86_64 archive was downloaded",
        reporters=7,
        observed={"os.name": "linux", "os.arch": "aarch64", "os.kernel": "6.8"},
        stated={"engram.symptom": "the binary will not start at all"},
        outcome="resolved", ux_severity="high", model_class="none",
    ),
    dict(
        note="Search went empty after the embedding model changed",
        reporters=5,
        observed={"os.name": "linux", "os.arch": "x86_64", "os.kernel": "6.8",
                  "gpu.name": "NVIDIA GeForce RTX 4070", "gpu.vram_total_mib": "12288",
                  "engram.version": "1.1.4"},
        stated={"engram.symptom": "search returns nothing, and it used to work"},
        outcome="resolved", ux_severity="medium", model_class="local_large",
    ),
    dict(
        note="Ingest slow on a machine with no GPU — nothing is broken",
        reporters=6,
        observed={"os.name": "windows", "os.arch": "x86_64", "engram.version": "1.2.2"},
        stated={"engram.symptom": "document ingest is extremely slow"},
        outcome="unresolved", ux_severity="medium", model_class="local_small",
    ),
    dict(
        note="No model endpoint — chat and debate are the only things that fail",
        reporters=6,
        observed={"os.name": "windows", "os.arch": "x86_64", "os.kernel": "10.0",
                  "engram.version": "1.2.2", "ollama.container_version": "0.3.14"},
        stated={"engram.symptom": "the chat or debate never answers",
                "ollama.host": "OLLAMA_HOST is not set and I am not running Ollama",
                "ollama.log": None},
        outcome="resolved", ux_severity="high", model_class="none",
        words=OLLAMA_LOG, words_from=2,
    ),
    dict(
        note="Below the floor on purpose: three reporters, never surfaced",
        reporters=3,
        observed={"os.name": "macos", "os.arch": "aarch64", "os.kernel": "24.1"},
        stated={"engram.symptom": "something else"},
        outcome="escalated", ux_severity="low", model_class="cloud",
    ),
]


def post(body: dict) -> dict:
    req = urllib.request.Request(
        f"{SERVER}/report", data=json.dumps(body).encode(),
        headers={"Content-Type": "application/json"}, method="POST")
    with urllib.request.urlopen(req, timeout=10) as r:
        return json.load(r)


for i, s in enumerate(SCENARIOS):
    last = None
    for n in range(s["reporters"]):
        body = {
            "pseudonym": f"engram-demo-{i}-{n}",
            "subject": SUBJECT,
            "observed": s["observed"],
            "stated": s["stated"],
            "outcome": s["outcome"],
            "ux_severity": s["ux_severity"],
            "model_class": s["model_class"],
        }
        if s.get("words") and n < s.get("words_from", 0):
            body["description"] = s["words"]
            body["description_consent"] = {
                "granted": True, "granted_at": "2026-09",
                "destination": f"the maintainers of {SUBJECT}, through the operator"}
        last = post(body)
    print(f"{'shown ' if last['surfaced'] else 'HIDDEN'} "
          f"{last['reporters']} reporters — {s['note']}")
    if not last["surfaced"]:
        print(f"         {last['why']}")

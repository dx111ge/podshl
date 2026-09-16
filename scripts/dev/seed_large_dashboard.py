"""A project with a lot of problems, for looking at the maintainer's dashboard.

Every fixture the dashboard has been looked at with had one to five clusters,
and the page read well at that size. A project people actually use has hundreds
of configurations above the floor. This builds one in the development database
through the shipped path — published files over HTTP, the real ingest, and
`report()` for every report — so the page is looked at with the shape of data
it will get rather than the shape a fixture author imagined.

Deterministic, so two runs look alike. Run it inside the container:

    docker compose exec -e PYTHONPATH=/app/src -e PODSHL_ALLOW_LOOPBACK=1 \
        podshl python scripts/dev/seed_large_dashboard.py

It prints the host and a dashboard token. Development database only: it mints
a claim directly, the way the suite does.
"""
from __future__ import annotations

import random
import sys

sys.path.insert(0, "/app/src")

from podshl.server import testcases as t  # noqa: E402
from podshl.server.app import report  # noqa: E402

SYMPTOMS = ["will-not-start", "crashes-on-open", "sync-stalls", "login-loop",
            "render-black", "export-fails", "plugin-missing", "search-slow",
            "memory-grows", "update-fails", "audio-crackle", "fonts-broken",
            "print-blank", "notifications-silent"]
OSES = ["windows", "linux", "macos"]
ARCHES = ["x86_64", "aarch64"]
GPUS = ["nvidia", "amd", "intel", "none"]
VERSIONS = [f"2.{i}" for i in range(10)]

EXCERPTS = [
    "time=<time> level=ERROR msg=\"sync worker stopped\" err=\"context deadline exceeded\" peer=<ip>:443",
    "thread 'main' panicked at src/render/surface.rs:212:9:\ncalled `Result::unwrap()` on an `Err` value: "
    "SurfaceLost\nnote: run with `RUST_BACKTRACE=1` environment variable to display a backtrace",
    "[<time>] export: writing C:\\Users\\<user>\\Documents\\report.pdf\n[<time>] export: font subset failed "
    "for 'Inter Display' (glyph 0x2011)\n[<time>] export: aborted, 0 bytes written",
    "Traceback (most recent call last):\n  File \"plugins/loader.py\", line 88, in load\n    spec.loader.exec_module(mod)"
    "\nModuleNotFoundError: No module named 'app_ext_calendar'",
    "E/<time> updater: signature check failed for 2.7.1 (expected key id <identifier>)\n"
    "E/<time> updater: rolling back to 2.6.4",
]


def manifest() -> tuple[dict[str, bytes], dict]:
    rng = random.Random(7)
    lines = [b"endpoint: {base}\ncommit: r1\nstatus: active\nlangs: [en]\n",
             b"problem_classes: [" + ", ".join(f"app.{s}" for s in SYMPTOMS).encode() + b"]\n",
             b"collect:\n",
             b"  - id: os.name\n    kind: machine\n    describes: Operating system\n"
             b"    why: the builds differ\n    read: { op: os_fact, name: os }\n",
             b"  - id: os.arch\n    kind: machine\n    describes: Architecture\n"
             b"    why: the builds differ\n    read: { op: os_fact, name: arch }\n",
             b"  - id: app.symptom\n    kind: human\n    describes: What goes wrong\n"
             b"    why: it decides the class\n    prompt: What goes wrong?\n    choices: ["
             + ", ".join(SYMPTOMS).encode() + b"]\n",
             b"  - id: gpu.vendor\n    kind: human\n    describes: Graphics card maker\n"
             b"    why: rendering paths differ\n    prompt: Which graphics card?\n    choices: ["
             + ", ".join(GPUS).encode() + b"]\n",
             b"solutions:\n"]
    files: dict[str, bytes] = {}
    rules: dict = {}

    def solution(sid: str, symptom: str, when: str) -> None:
        files[f"/.podshl/solutions/{sid}.md"] = (
            f"---\nid: {sid}\nanswers:\n  problem_class: app.{symptom}\n  when:\n{when}"
            f"severity: {rng.choice(['low', 'medium', 'high'])}\n"
            f"proposes:\n  - action: report_only\n    params: {{}}\n---\nDo the thing.\n").encode()
        lines.append(f"  - solutions/{sid}.md\n".encode())

    for i, symptom in enumerate(SYMPTOMS):
        if i < 10:
            # The shape that builds: one answer, decided by what the person
            # says goes wrong. It spans every machine, which is where an answer
            # that helps on one architecture and not another comes from.
            solution(symptom, symptom, f"    app.symptom: {symptom}\n")
            for os_name in OSES:
                rules[(symptom, os_name)] = i
        elif i == 10:
            # The person names the symptom and the machine's OS picks the fix.
            # Refused until `SV109`; it builds now, with a skipped question
            # falling back to the answer for that OS.
            for os_name in OSES:
                solution(f"{symptom}-{os_name}", symptom,
                         f"    app.symptom: {symptom}\n    os.name: {os_name}\n")
        elif i == 11:
            # The shape that still does not: two symptoms under one OS in one
            # class, so the class does not say which and a skip has nowhere to
            # go. Its tree is refused at ingest, which the maintainer has to see.
            for os_name in OSES:
                solution(f"{symptom}-{os_name}", symptom,
                         f"    app.symptom: {symptom}\n    os.name: {os_name}\n")
                solution(f"{symptom}-other-{os_name}", symptom,
                         f"    app.symptom: {SYMPTOMS[12]}\n    os.name: {os_name}\n")
        # The last two symptoms have no answer at all.
    files["/.podshl/agent.yaml"] = b"".join(lines)
    return files, rules


def main() -> None:
    import os

    from podshl.server import db

    rng = random.Random(11)
    files, rules = manifest()
    # A readable name for screenshots and recordings, when one is given. It has
    # to be new: two anchors under one host would make the dashboard ambiguous.
    wanted = os.environ.get("HOST")
    if wanted:
        with db.read() as conn:
            with conn.cursor() as cur:
                cur.execute("SELECT 1 FROM anchor WHERE host = %s", (wanted,))
                if cur.fetchone():
                    sys.exit(f"{wanted} is already in this database; choose another HOST")
    host, token, stop = t._served_project(files, "large-token", host=wanted)
    stop()

    configs = set()
    while len(configs) < 340:
        configs.add((rng.choice(SYMPTOMS), rng.choice(OSES), rng.choice(ARCHES),
                     rng.choice(GPUS), rng.choice(VERSIONS)))

    n_reports = 0
    for c, (symptom, os_name, arch, gpu, version) in enumerate(sorted(configs)):
        # A quarter stay below the floor, as the long tail does.
        people = rng.randint(1, 4) if rng.random() < 0.25 else rng.randint(5, 16)
        # The hidden truth the dashboard should help find: some answers fail on
        # ARM, some on old versions, and some configurations simply go both ways.
        klass = rules.get((symptom, os_name), -1)
        fails = (klass % 3 == 1 and arch == "aarch64") or (klass % 5 == 2 and version < "2.4")
        mixed = rng.random() < 0.12
        quiet = rng.random() < 0.15
        for p in range(people):
            if quiet:
                outcome = None
            elif mixed:
                outcome = "resolved" if p % 2 else "unresolved"
            else:
                outcome = "unresolved" if fails else "resolved"
            body = {"pseudonym": f"large-{host}-{c}-{p}", "subject": host, "model_class":
                    rng.choice(["none", "local_small", "hosted_large"]),
                    "observed": {"os.name": os_name, "os.arch": arch},
                    "stated": {"app.symptom": symptom, "gpu.vendor": gpu, "app.version": version}}
            if outcome:
                body["outcome"] = outcome
            if rng.random() < 0.08:
                body["description"] = rng.choice(EXCERPTS)
                body["description_consent"] = {"granted": True, "destination": host,
                                               "granted_at": "2026-09"}
            out = report(body)
            assert isinstance(out, dict) and out["accepted"], out
            n_reports += 1

    print(f"host  {host}")
    print(f"token {token}")
    print(f"{len(configs)} configurations, {n_reports} reports")
    print(f"open  http://127.0.0.1:8725/dashboard#{host}")


if __name__ == "__main__":
    main()

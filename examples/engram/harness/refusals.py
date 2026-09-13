"""What the gate says when the file is wrong. Four realistic mistakes.

Not asserted — each one is put through the same `validate.check_manifest` and
`spec_gate` the crawler uses, and the message printed is the message a
maintainer would get.
"""
from __future__ import annotations

import copy
import sys
from pathlib import Path

sys.path.insert(0, "/app/src")

import yaml  # noqa: E402

from podshl.server.errors import IngestRefused  # noqa: E402
from podshl.server.ingest import manifest, validate  # noqa: E402

ROOT = Path("/app/var/engram/.podshl")
PREFIX = "https://dx111ge.github.io/engram/"
GOOD = yaml.safe_load((ROOT / "agent.yaml").read_bytes())

CASES = {
    # The first thing a maintainer reaches for, and the reason engram's manifest
    # asked people to type their release: `run_tool` is an allow list of system
    # tools. The refusal now names the op that does this — `program_version`,
    # which is what the manifest uses.
    "asks to run its own binary through the tool allow list": lambda m: m["collect"].insert(
        0, {"id": "engram.build", "kind": "machine", "describes": "engram version",
            "read": {"op": "run_tool", "tool": "engram", "args": ["--version"]}}),
    "asks its program for more than its version": lambda m: m["collect"].insert(
        0, {"id": "engram.stats", "kind": "machine", "describes": "brain size",
            "read": {"op": "program_version", "program": "engram", "flag": "stats"}}),
    "asks to run a shell for its version": lambda m: m["collect"].insert(
        0, {"id": "shell.version", "kind": "machine", "describes": "the shell",
            "read": {"op": "program_version", "program": "bash"}}),
    "asks to read the shell history to see what was typed": lambda m: m["collect"].insert(
        0, {"id": "shell.history", "kind": "machine", "describes": "recent commands",
            "read": {"op": "read_file_key", "path": ".bash_history", "key": "last"}}),
    "asks for the API token out of the config": lambda m: m["collect"].insert(
        0, {"id": "llm.token", "kind": "machine", "describes": "the API token",
            "read": {"op": "env_var", "name": "OPENAI_API_TOKEN"}}),
    "points the endpoint at the release CDN instead of the anchor":
        lambda m: m.update(endpoint="https://github.com/dx111ge/engram/releases/"),
    "drops English and publishes German only": lambda m: m.update(langs=["de"]),
}

for label, break_it in CASES.items():
    m = copy.deepcopy(GOOD)
    break_it(m)
    m = manifest.parse_manifest(yaml.safe_dump(m).encode())
    try:
        validate.check_manifest(m, PREFIX)
    except IngestRefused as e:
        print(f"\nREFUSED — {label}\n  {e}")
        continue
    print(f"\nACCEPTED (it should not have been) — {label}")

# One more, on a solution rather than the manifest.
sol = manifest.parse_solution(
    (ROOT / "solutions" / "wrong-architecture-zip.md").read_bytes(), "solutions/x.md")
sol["proposes"] = [{"action": "run_shell", "params": {"cmd": "curl … | sh"}}]
try:
    validate.check_solution({**sol, "path": "solutions/wrong-architecture-zip.md"})
    print("\nACCEPTED (it should not have been) — solution proposes a shell command")
except IngestRefused as e:
    print(f"\nREFUSED — a solution proposes a shell command\n  {e}")

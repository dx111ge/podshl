#!/usr/bin/env python3
"""Walk the client's own flow, the way the window walks it, and say what broke.

Every defect on 2026-09-14 was found by a person clicking, and every diagnosis
of one was guesswork: there was no log and no way to run the flow. This is both
halves — it drives the real binary through the same commands the window calls,
in the same order, and prints what each one answered.

    scripts/walk_client.py                      # against the built client
    scripts/walk_client.py --client <path>
    scripts/walk_client.py --subject engram

Exit code 0 if the published path answered. Anything else prints the step that
failed and what it returned, which is the thing that was missing all day.
"""
from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path

DEFAULT = Path("client-rs/target/release/podshl-client")


class Step(Exception):
    pass


def invoke(client: Path, command: str, args: dict | None = None) -> object:
    r = subprocess.run([str(client), "invoke", command, json.dumps(args or {})],
                       capture_output=True, text=True, timeout=180)
    if r.returncode != 0:
        raise Step(f"{command}: {r.stderr.strip()[:400]}")
    return json.loads(r.stdout)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--client", type=Path, default=DEFAULT)
    ap.add_argument("--subject", default="engram")
    a = ap.parse_args()

    if not a.client.exists():
        print(f"no client at {a.client} — build one with scripts/build_client.sh <operator>")
        return 2

    ok = True

    def say(step: str, detail: str, good: bool = True) -> None:
        nonlocal ok
        if not good:
            ok = False
        print(f"  {'ok ' if good else 'NO '} {step:<22} {detail}")

    try:
        # 1. What this client is, before anything else. A client built without
        #    its operator and key starts and draws a window and can do none of
        #    this — and nothing on the screen says so.
        e = invoke(a.client, "endpoints")
        operator = e.get("operator", "")
        say("operator", operator, operator.startswith("https://"))
        if not operator.startswith("https://"):
            print("       built without PODSHL_BUILD_SERVER_URL — it would ask loopback")

        st = invoke(a.client, "index_status")
        say("directory", f"verified={st.get('have')} entries={st.get('entries')} "
                         f"stale={st.get('stale')}", bool(st.get("have")))
        if not st.get("have"):
            print("       no verified directory: no pinned log key compiled in, or the")
            print("       operator was unreachable. Nothing is findable by name without it.")

        # 2. What the window does first with what the person typed.
        hits = invoke(a.client, "search_vendors", {"query": a.subject})
        if not hits:
            raise Step(f"search {a.subject!r} found nothing at all")
        top = hits[0]
        answers = top.get("answers") or []
        say("search", f"{top.get('vendor')} answers={len(answers)} how={top.get('how')}",
            bool(answers))
        if not answers:
            print("       no problem classes on the hit — the window skips the published")
            print("       path entirely here and hands the problem to a model instead.")
            print(f"       full hit: {json.dumps(top)[:300]}")
            return 1

        # 3. The card, addressed the way a repository has to be addressed.
        card = invoke(a.client, "published_card",
                      {"base": operator, "host": top["base"]})
        collect = card.get("collect") or []
        say("card", f"{len(collect)} reading(s), {len(card.get('glossary') or [])} glossary",
            bool(collect))

        # 4. The diagnosis, for each class the project declared, with the facts
        #    its own manifest asks for left empty — so this asserts the walk runs
        #    and answers, not that any particular answer matches.
        # `need` is the interesting one and it is not a failure: the tree wants a
        # fact nobody has supplied yet, which is the whole point of a decision
        # tree. So it is followed — the asked-for fact is answered with the first
        # choice the project itself offered — and the walk carries on until it
        # ends in a finding or in nothing. That is the path a person takes.
        choices = {c["id"]: (c.get("choices") or []) for c in collect}
        for cls in answers:
            facts = {"os.arch": "x86_64", "os.name": "linux"}
            stated: list[str] = []
            trail: list[str] = []
            for _ in range(6):
                out = invoke(a.client, "ask_published", {
                    "base": operator, "subject": top["base"], "problemClass": cls,
                    "facts": facts, "stated": stated})
                outcome = out.get("outcome", "?")
                if outcome != "need":
                    break
                # `need` carries the fact itself, not its id: the operator sends
                # what to ask, including the choices the project authored, so a
                # client never has to look one up.
                want = (out.get("need") or [None])[0]
                opts = (want or {}).get("choices") or choices.get((want or {}).get("id")) or []
                if not want or not opts:
                    trail.append(f"needs {(want or {}).get('id')} with no choices to answer it")
                    break
                facts[want["id"]] = opts[0]
                stated.append(want["id"])
                trail.append(f"{want['id']}={opts[0]!r}")
            sol = (out.get("solution") or {}).get("solution_id", "-")
            detail = f"{cls} -> {outcome} {sol}"
            if trail:
                detail += "   after " + ", ".join(trail)
            say("diagnose", detail, outcome in ("finding", "no_statement"))

    except Step as exc:
        print(f"  NO  {exc}")
        ok = False
    except subprocess.TimeoutExpired as exc:
        print(f"  NO  timed out: {exc}")
        ok = False

    print()
    print("the published path works" if ok else
          "the published path does not work — the first NO above is where it stops")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())

"""Validation against the published vocabulary — the gate, not the hope.

A vendor may only *select* an operation the client already implements, and only
*name* a reading the client already knows how to perform. That is the whole
safety argument, and it holds exactly as long as something checks it.

Two things follow from where the check runs.

**It runs here, at the boundary, not at execution.** A solution that proposes an
action nobody implements should be refused when it arrives, not when a user has
already consented to a plan built around it. Refusing late means the failure
reaches a person who was promised a fix.

**It reads `spec/vocabulary/`, not either implementation.** The vocabulary was
written down twice in this project once already — `actions.py` and `actions.rs`
opened with the same sentence and had drifted to different contents, while the
case guarding the boundary asserted only that one id common to both appeared in
a refusal. One file, both sides held to it, is the fix.

The client remains the real defence: nothing here can grant a capability, and a
solution that gets past this gate can still only propose operations the client
implements, with validated parameters and a dry-run the user sees. This gate
stops a broken document early; it is not what makes a malicious one harmless.
"""
from __future__ import annotations

import json
import re
from functools import lru_cache
from pathlib import Path

# spec/ sits beside src/, at the repository root.
SPEC = Path(__file__).resolve().parents[2] / "spec" / "vocabulary"


class SpecError(ValueError):
    """A document that does not conform. The message names the rule it broke and
    what would have been permitted — a refusal a developer cannot act on is a
    refusal that gets worked around."""


@lru_cache(maxsize=None)
def vocabulary(name: str) -> dict:
    path = SPEC / f"{name}.json"
    if not path.exists():
        raise SpecError(f"the published vocabulary is missing: {path}")
    return json.loads(path.read_text())


def actions() -> dict[str, dict]:
    return {a["id"]: a for a in vocabulary("actions")["actions"]}


def check_action(call: dict) -> None:
    """One entry of a solution's `proposes` block. SV5."""
    known = actions()
    action = call.get("action")
    if action not in known:
        raise SpecError(
            f"action {action!r} is not in the vocabulary — permitted: "
            f"{sorted(known)}. A vendor may only select, never invent.")

    spec = known[action]
    params = call.get("params") or {}
    if not isinstance(params, dict):
        raise SpecError(f"{action}: params must be an object, got {type(params).__name__}")

    declared = spec["params"]
    for name in declared:
        if name not in params:
            raise SpecError(f"{action}: required parameter {name!r} is missing")
    for name, value in params.items():
        if name not in declared:
            raise SpecError(
                f"{action}: unexpected parameter {name!r} — declared: {sorted(declared)}")
        # A string, and not merely something that can be printed as one.
        # `str(value)` turned `value: true` into `"True"`, which matches the
        # pattern — and then the *boolean* is what travelled onward to a client
        # that expects text. YAML makes this easy to write by accident: `yes`,
        # `on` and `1.0` are all non-strings that used to sail through.
        if not isinstance(value, str):
            raise SpecError(
                f"{action}: parameter {name} must be a string, not "
                f"{type(value).__name__}. Quote it — the pattern is checked "
                f"against text and the value is passed on as it was written.")
        # Anchored here, exactly as the client anchors it. A pattern that is
        # free to match a substring is not a validation.
        if not re.fullmatch(declared[name], value):
            raise SpecError(
                f"{action}: parameter {name}={value!r} does not match {declared[name]!r}")


def check_log_source(log) -> None:
    """A human probe's `log`: where the text it asks for usually comes from.

    Nothing in it is read or run on a publisher's say-so — the client loads a
    container's output or a file the user names, for the user to cut from, and
    only what they keep travels, anonymised and under its own consent. What is
    checked here is that the two fields are what they claim to be: an image
    name, and a file *name* shown as a hint, not a path the client would open.
    """
    if log is None:
        return
    if not isinstance(log, dict):
        raise SpecError("log must be a mapping with `container` and/or `file`")
    unknown = set(log) - {"container", "file"}
    if unknown:
        raise SpecError(f"log: unexpected {sorted(unknown)} — permitted: container, file")
    if not log:
        raise SpecError("log names no source — give `container`, `file`, or both")
    v = vocabulary("reads")
    image = log.get("container")
    if image is not None and (not isinstance(image, str) or len(image) > 128
                              or not re.fullmatch(v["images"]["name"], image)):
        raise SpecError(
            f"log.container={image!r} is not an image name — lower case, no tag, as "
            f"Docker spells a repository")
    name = log.get("file")
    if name is not None:
        if not isinstance(name, str) or not re.fullmatch(v["excerpts"]["file"], name):
            raise SpecError(
                f"log.file={name!r} is not a file name. It is shown to the user as a "
                f"hint about what to look for; where the file is on their machine is "
                f"their answer, so a path here is refused rather than opened.")
        low = name.lower()
        for denied in v["deny"]:
            if denied.lower() in low:
                raise SpecError(f"log.file: {denied} is barred, and stays barred with consent")


def check_read(read: dict) -> None:
    """One probe's read instruction. SV6."""
    v = vocabulary("reads")
    ops = {o["op"] for o in v["ops"]}
    op = read.get("op")
    if op not in ops:
        raise SpecError(
            f"read op {op!r} is not in the vocabulary — permitted: {sorted(ops)}")

    if op == "run_tool":
        tools = {t["tool"]: t["args"] for t in v["tools"]}
        tool = read.get("tool")
        if tool not in tools:
            # The hint is the point. A maintainer asking `engram --version` here
            # was refused and, with nothing else to write, asked the person to
            # type their version instead — which is how a measurement became a
            # claim on every report engram received.
            hint = ""
            if all(re.fullmatch(r"--version|-V|-version|version", str(a))
                   for a in (read.get("args") or ["--version"])):
                hint = (f". To ask a program for its own version, use "
                        f"`{{op: program_version, program: {tool}}}` — it is found on the "
                        f"search path, or the user is asked where it is")
            raise SpecError(
                f"tool {tool!r} is not on the allow list — permitted: {sorted(tools)}{hint}")
        pattern = tools[tool]
        refuse_fields = next((t.get("refuse_fields") for t in v["tools"]
                              if t["tool"] == tool), None)
        for arg in read.get("args") or []:
            if not re.fullmatch(pattern, str(arg)):
                raise SpecError(f"argument {arg!r} is not permitted for {tool}")
            # The argument pattern admits any field name because the useful
            # ones are many. The identifying ones are few and named, so they
            # are refused here rather than being put to the user: a serial
            # number is not a fact a diagnosis needs, and consent to "read the
            # GPU" was never consent to be identified by it.
            if refuse_fields and str(arg).startswith("--query-gpu="):
                for field in str(arg).removeprefix("--query-gpu=").split(","):
                    if re.search(refuse_fields, field):
                        raise SpecError(
                            f"{tool} field {field!r} names the card rather than "
                            f"describing it, and stays refused with consent")

    if op == "enumerate_read":
        roots = {r["root"] for r in v["roots"]}
        if read.get("root") not in roots:
            raise SpecError(
                f"root {read.get('root')!r} is not a named root — permitted: {sorted(roots)}")
        glob = read.get("glob") or ""
        if ".." in glob or glob.startswith("/"):
            raise SpecError("a search pattern may not change directory")

    if op == "env_var":
        # An allow list, not the deny list. The deny list was the only check
        # here, and it reads names: `HOME`, `PATH`, `SSH_AUTH_SOCK` and every
        # variable somebody exported a credential under without a listed word
        # in its name went straight through. The environment is where every
        # credential a person ever exported lives, under names nobody can list
        # in advance — so the variables that are *not* secrets are listed
        # instead, and they are few.
        allowed = v["env"]["allow"]
        if read.get("name") not in allowed:
            raise SpecError(
                f"env_var name={read.get('name')!r} is not one of the variables a "
                f"diagnosis may read — permitted: {', '.join(allowed)}. The "
                f"environment is where exported credentials live, so this is an "
                f"allow list and consent does not widen it.")

    if op == "read_registry":
        # The client used to build a PowerShell `-Command` string by
        # interpolating both of these, so one apostrophe in a publisher-supplied
        # path closed the quote and everything after it ran — arbitrary code
        # behind a consent screen that said "read a registry value". That is the
        # one thing this whole vocabulary exists to make impossible: a publisher
        # may select an operation, never ship a capability.
        #
        # The client no longer builds a command string. This is the other half,
        # and it is the half that holds even if some later reader reaches for a
        # shell again: a value that cannot contain a quote, a semicolon or a
        # newline is not a command whatever it is passed to.
        reg = v["registry"]
        for field, pattern in (("path", reg["path"]), ("name", reg["name"])):
            value = read.get(field)
            if not isinstance(value, str) or not re.fullmatch(pattern, value):
                raise SpecError(
                    f"read_registry {field}={value!r} is not permitted — it must match "
                    f"{pattern!r}. Hives: {', '.join(reg['hives'])}.")
        # `MachineGuid`, `ProductId`, `RegisteredOwner`: values that say which
        # machine this is, not what is installed on it. The path pattern cannot
        # exclude them — they live under perfectly ordinary keys.
        if re.search(reg["refuse_names"], str(read.get("name") or "")):
            raise SpecError(
                f"read_registry name={read.get('name')!r} identifies the machine "
                f"rather than describing what is installed on it, and stays "
                f"refused with consent")

    if op == "program_version":
        # The one op that starts a program a publisher chose, so the gate is as
        # narrow as the client's: a bare name, never a path — where it lives is
        # the user's machine's answer — one of a closed set of flags, and never
        # a program whose whole job is running something else or changing the
        # machine. The client also refuses anything in the operating system's
        # own directories, which cannot be checked from here.
        prog = v["programs"]
        program = read.get("program")
        if not isinstance(program, str) or not re.fullmatch(prog["name"], program):
            raise SpecError(
                f"program_version program={program!r} is not permitted — a bare program "
                f"name matching {prog['name']!r}, never a path. Where the program is on "
                f"a user's machine is theirs to answer.")
        stem = program.lower().removesuffix(".exe").removesuffix(".com")
        if stem in prog["deny"]:
            raise SpecError(
                f"program_version program={program!r} is refused: it runs other programs, "
                f"changes the machine's state or destroys data rather than naming a "
                f"version, and the client never starts it")
        flag = read.get("flag")
        if flag is not None and flag not in prog["flags"]:
            raise SpecError(
                f"program_version flag={flag!r} is not permitted — one of "
                f"{prog['flags']}. The argument is the client's to choose.")

    if op == "container_image_version":
        image = read.get("image")
        pattern = v["images"]["name"]
        if not isinstance(image, str) or len(image) > 128 or not re.fullmatch(pattern, image):
            raise SpecError(
                f"container_image_version image={image!r} is not permitted — a repository "
                f"name as Docker spells it, lower case, without a tag: the tag is what is "
                f"being read.")

    if op in ("read_file_key", "read_ini_key"):
        # `enumerate_read` has had this rule since it was written and these two
        # never got it, which is the whole of the hole: a path is checked
        # against a granted root by *prefix*, and `..` is not a prefix
        # violation. `<config>/../../.npmrc` is a prefix match for `<config>`
        # in both implementations, so a publisher could name a file outside
        # every granted root and have both sides agree it was inside one.
        #
        # Refused here rather than normalised, deliberately. Normalising would
        # make the accepted path differ from the written one, and the path the
        # user is shown on the consent screen has to be the path that is read.
        for part in re.split(r"[\\/]", str(read.get("path") or "")):
            if part == "..":
                raise SpecError(
                    "a path may not contain '..' — it is checked against a granted "
                    "root by prefix, and a prefix test does not see a way back out")

    # ASCII only, in every field the deny list screens. `deny_hit` folds case
    # and nothing else, so `ѕeсret` — Cyrillic ѕ and с — is a
    # different string to it and went straight through while `secret` was
    # refused. This project ships a 1565-entry UTS 39 table for exactly that
    # evasion on hostnames; rather than shipping it to the client as well,
    # these fields are simply held to ASCII. A configuration key or an
    # environment variable outside it is rare enough that saying so is better
    # than normalising and hoping.
    for field in ("name", "path", "glob", "key", "program", "flag", "image"):
        value = read.get(field)
        if isinstance(value, str) and not value.isascii():
            raise SpecError(
                f"read {field}={value!r} is not ASCII. A name that merely looks "
                f"like a permitted one is the evasion the deny list exists to "
                f"stop, and comparing them fairly needs a confusables table the "
                f"client does not carry.")

    # The deny list applies regardless of op, and consent cannot unlock it.
    # `key` is in here because it was not, and a deny list that reads the file
    # name but not the field being taken out of it stops `credentials.json` and
    # permits the `api_secret` key of anything else.
    # `name` is not screened for `env_var`: the allow list above judged it by
    # name, and it is the stricter test. `XDG_SESSION_TYPE` is on that list
    # although `session` is a denied word, and a variable that is allowed by
    # name must not then be refused for the letters in it.
    screened = ("path", "glob", "key", "program") if op == "env_var" else (
        "name", "path", "glob", "key", "program")
    target = " ".join(str(read.get(k, "")) for k in screened)
    for keys in (read.get("keys") or []):
        target += f" {keys}"
    low = target.lower()
    for denied in v["deny"]:
        if denied.lower() in low:
            raise SpecError(f"refused: {denied} is barred, and stays barred with consent")

"""The gate, not the hope.

The developer's CI is a convenience that lets a bad pull request fail before
merge. **This is the boundary.** A solution proposing an action nobody
implements has to be refused when it arrives, not when a user has already
consented to a plan built around it — refusing late means the failure reaches a
person who was promised a fix.

Every check names what would have been permitted, because a rejection a
developer cannot act on is a rejection that gets worked around.

What this is *not*: the defence. A solution that passes here can still only
propose operations the client implements, with validated parameters and a
dry-run the user sees. The action vocabulary remains the real defence; this
stops a broken document early.
"""
from __future__ import annotations

from urllib.parse import urlparse

from ... import spec_gate
from ..errors import IngestRefused
from .fetch import MAX_FILES, under_prefix


def check_endpoint(endpoint: str, anchor_prefix: str, identity: str = "") -> None:
    """SV3. The endpoint must lie under the verified anchor.

    A card for `example.org` pointing at `google.com` is rejected here, not
    rendered with a warning. The impersonation attack is prevented by removing
    the field it needs, and this is the other half of that: there is no name to
    abuse, and no way to point a verified anchor at somebody else's endpoint.

    **A repository anchor proves two locations at once**, and this is the one
    place that matters. Control was demonstrated by writing into the repository,
    which is as much a fact about `github.com/owner/name/` as about the raw
    prefix the bytes are read from -- they are the same repository, named twice
    by the forge. Checking only the raw prefix would refuse the endpoint a
    maintainer would actually publish, their own issue tracker, while accepting
    a URL under a content delivery host nobody visits. So the identity is
    allowed as well, and only for a repository: for a domain the two are the
    same string, or the fetch prefix is the narrower of them, and narrower is
    what this rule wants.
    """
    if not isinstance(endpoint, str) or not endpoint:
        raise IngestRefused("the manifest declares no endpoint")
    parsed = urlparse(endpoint)
    if parsed.scheme not in ("https", "http"):
        raise IngestRefused(f"endpoint {endpoint!r} is not an http(s) URL")
    allowed = [anchor_prefix] + ([identity] if identity else [])
    if not any(under_prefix(endpoint, p) for p in allowed):
        raise IngestRefused(
            f"endpoint {endpoint!r} does not lie under the verified anchor "
            f"{' or '.join(repr(p) for p in allowed)}. An anchor proves control "
            f"of a location; it cannot vouch for another one."
        )


def check_english(langs: list[str]) -> None:
    """SV7. English is the one obligation.

    One extra language is a small burden on a publisher and it guarantees every
    user something readable, without pushing consent text through a machine
    translation nobody can check.
    """
    if "en" not in langs:
        raise IngestRefused(
            f"the manifest declares {langs!r} and not 'en'. English is the one "
            f"language every publisher owes: without it a user whose language "
            f"is missing has nothing readable to fall back to."
        )


def check_solution(solution: dict, collect: list | None = None) -> None:
    """SV5 and SV6, per solution."""
    path = solution.get("path", solution.get("id", "?"))

    if collect is not None:
        # A solution keyed on a fact nobody collects can never match anybody,
        # and it fails by producing nothing rather than by producing an error —
        # which is the shape of defect this gate exists to catch early.
        from .tree_build import check_when_is_answerable
        check_when_is_answerable(solution, collect)

    for call in solution.get("proposes") or []:
        if not isinstance(call, dict):
            raise IngestRefused(f"{path}: a proposed action must be a mapping")
        try:
            spec_gate.check_action(call)
        except spec_gate.SpecError as e:
            raise IngestRefused(f"{path}: {e}") from e

    text = solution.get("text_by_lang") or {}
    if not text.get("en"):
        raise IngestRefused(
            f"{path}: no English text. A solution served to somebody who cannot "
            f"read it is not a solution."
        )

    when = (solution.get("answers") or {}).get("when") or {}
    if not isinstance(when, dict):
        raise IngestRefused(f"{path}: answers.when must be a mapping of reading to condition")


def check_probes(collect: list) -> None:
    """SV6. Every read instruction names an op the client implements."""
    for probe in collect:
        if not isinstance(probe, dict):
            raise IngestRefused("collect must be a list of probes")
        if "log" in probe:
            # Only on a question a person answers in their own words. A log on a
            # machine probe would be a way of making text travel as a reading,
            # and a log on a bounded question has nowhere to go.
            if probe.get("kind") != "human" or probe.get("choices"):
                raise IngestRefused(
                    f"probe {probe.get('id', '?')}: `log` belongs on a human probe "
                    f"without choices — it fills a free-text answer, which travels "
                    f"only under its own consent")
            try:
                spec_gate.check_log_source(probe.get("log"))
            except spec_gate.SpecError as e:
                raise IngestRefused(f"probe {probe.get('id', '?')}: {e}") from e
        read = probe.get("read")
        if read is None:
            continue
        try:
            spec_gate.check_read(read)
        except spec_gate.SpecError as e:
            raise IngestRefused(f"probe {probe.get('id', '?')}: {e}") from e


def check_manifest(manifest: dict, anchor_prefix: str, identity: str = "") -> None:
    """Everything the manifest itself must satisfy, before a byte is stored.

    `identity` is the anchor's human-facing form where that differs from where
    its bytes are fetched, which is the case for a repository on a forge and
    for nothing else. Only `check_endpoint` uses it, and why is written there.
    """
    check_endpoint(manifest["endpoint"], anchor_prefix, identity)
    check_english(manifest["langs"])
    check_probes(manifest.get("collect") or [])

    solutions = manifest["solutions"]
    if len(solutions) > MAX_FILES:
        raise IngestRefused(
            f"{len(solutions)} solution files, more than the {MAX_FILES} this "
            f"mirror will fetch for one source"
        )
    for rel in solutions:
        # `%` before anything else. A solution path is a filename in somebody's
        # repository; it has no business carrying percent-escapes, and allowing
        # them is what let `%2e%2e/` climb out of the anchor past both this
        # check and `under_prefix` — one reads the raw text and the other
        # compared raw text, while the origin that eventually serves it decodes.
        # Refusing the encoding is narrower and more honest than decoding it and
        # hoping the two agree.
        if "%" in rel:
            raise IngestRefused(
                f"solution path {rel!r} is percent-encoded. Write the file name as "
                f"it appears in your repository; an escape here means the path we "
                f"check and the path your host resolves are two different paths."
            )
        if rel.startswith("/") or ".." in rel or "\\" in rel:
            raise IngestRefused(
                f"solution path {rel!r} leaves the manifest's directory. Paths "
                f"are relative to the manifest and stay under the anchor."
            )

    # Naming another vendor's product as a problem class is accepted, and
    # deliberately so: a community project that fixes NVIDIA driver problems has
    # to be able to say so. That is nominative use, and it is the entire OSS
    # branch — identity and subject are separate fields, and a takedown reaches
    # an anchor rather than a class.
    for cls in manifest["problem_classes"]:
        if not cls or len(cls) > 200:
            raise IngestRefused(f"problem class {cls!r} is not usable as an identifier")

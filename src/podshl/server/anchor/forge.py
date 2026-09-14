"""Turning a repository a person recognises into a place files can be fetched.

A repository anchor has two URLs, and they are genuinely different things. The
one a person recognises is `https://github.com/dx111ge/engram/` -- it is where
they got the software and it is what they will compare against their own memory.
The one the challenge and the published files come from is
`https://raw.githubusercontent.com/dx111ge/engram/HEAD/`, because a forge serves
file *contents* from somewhere other than its web pages.

**This is a table of URL shapes, not a forge API.** Nothing here asks a forge a
question, holds a token or has an account; `SERVER.md`'s "there is no forge API
involved" still holds. What the table does is connect an identity a human can
check to a location control can be proved at, and it can only ever be wrong in
one direction: a shape that does not match yields nothing, and a claim is
refused. It cannot make an unproved claim true.

**Every shape here was fetched before it was written down**, on 2026-09-14, and
each answered `200` with `text/plain` and the file's own bytes rather than a page
with the file inside it -- which is the property that matters, because the
challenge is compared to a token and an HTML wrapper would never match:

| Shape | Measured against | `HEAD` |
|---|---|---|
| `github` | `raw.githubusercontent.com/dx111ge/engram` | yes |
| `gitlab` | `gitlab.com/gitlab-org/gitlab-runner` | yes |
| `gitea` | `codeberg.org/forgejo/forgejo` **and** a self-hosted Gitea 1.26.1 | yes |
| `sourcehut` | `git.sr.ht/~sircmpwn/hare` | yes |

`HEAD` resolving the default branch on all four is what makes this possible at
all: no main-or-master guess, and nothing to re-ask when a project renames its
branch.

**Bitbucket is deliberately absent.** It was tried, the answer was `404`, and the
repository used for the attempt turned out not to exist -- so nothing was
measured and a documented shape is a guess with a URL in it. It goes in when
somebody has run it.

**A host nobody has heard of is fine**, and is the case this exists for: the
self-hosted Gitea and Forgejo instances the audience actually runs. There the
claimant names the shape, because no table can enumerate those hosts. Naming the
wrong one costs them the claim and gains them nothing -- the raw URL simply does
not answer.
"""
from __future__ import annotations

import re

#: What a forge allows in an owner or repository name. Deliberately narrower
#: than any forge's own rule: everything here has to survive being pasted into a
#: URL, compared as a string and shown to a person who is deciding whether they
#: recognise it. A name that needs escaping to be any of those is refused rather
#: than handled.
SEGMENT = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,99}$")

#: sourcehut writes a user as `~name`, and that tilde is part of the identity
#: rather than decoration -- `git.sr.ht/sircmpwn/hare` is not a URL. Allowed in
#: the owner and nowhere else.
OWNER = re.compile(r"^~?[A-Za-z0-9][A-Za-z0-9._-]{0,99}$")

#: Shape name -> template for the raw prefix. `{host}`, `{owner}` and `{repo}`
#: are filled from the identity, already lowercased. GitHub is the one that does
#: not use `{host}`, because it is the one that serves file contents from a
#: different name than its pages.
SHAPES: dict[str, str] = {
    "github": "https://raw.githubusercontent.com/{owner}/{repo}/HEAD/",
    "gitlab": "https://{host}/{owner}/{repo}/-/raw/HEAD/",
    # Gitea and Forgejo are one shape; Forgejo is a fork of Gitea and both serve
    # `/raw/HEAD/`. Measured on Codeberg and on a self-hosted Gitea 1.26.1, which
    # is the pair worth having: one public instance and one of the kind this is
    # for.
    "gitea": "https://{host}/{owner}/{repo}/raw/HEAD/",
    "sourcehut": "https://{host}/{owner}/{repo}/blob/HEAD/",
}

#: Hosts whose shape is not the claimant's to declare. Where we know, we know;
#: letting a caller say that `github.com` is a Gitea would let them point an
#: identity on one forge at a raw URL pattern belonging to another.
KNOWN_HOSTS: dict[str, str] = {
    "github.com": "github",
    "gitlab.com": "gitlab",
    "codeberg.org": "gitea",
    "git.sr.ht": "sourcehut",
}

#: The other side of a forge -- where it serves bytes rather than pages. An
#: identity must never be one of these: `raw.githubusercontent.com/owner/repo`
#: is not a repository anybody controls, it is a path on a CDN, and treating it
#: as an identity would let somebody anchor a name under GitHub's own host.
RAW_HOSTS = frozenset({"raw.githubusercontent.com", "raw.githack.com"})


def _segments_ok(owner: str, repo: str) -> bool:
    if not OWNER.match(owner) or not SEGMENT.match(repo):
        return False
    # `.` and `..` match the pattern and are path traversal wearing a name.
    return owner not in (".", "..") and repo not in (".", "..")


def parse(url: str, shape: str | None = None) -> tuple[str, str] | None:
    """`(identity, probe_prefix)` for a repository URL, or `None`.

    `None` means "this is not a repository this can anchor", which the caller
    turns into a refusal naming what would work. It never means "assume it is
    fine".

    `shape` names which forge software serves the host, and is **required for a
    host this does not know and refused for one it does**. Required, because no
    table can enumerate the self-hosted Gitea and Forgejo instances this exists
    for; refused where the host is known, because otherwise a caller could say
    that `github.com` is a Gitea and aim an identity on one forge at another
    forge's raw pattern. Declaring the wrong shape for your own host costs you
    the claim and gains you nothing: the raw URL does not answer.

    Lowercased, because a forge treats `dx111ge/Engram` and `dx111ge/engram` as
    the same repository and cannot hold both. Keeping the typed case would let
    one repository be claimed twice under two identities no forge can tell
    apart, which is a confusable pair we would have created ourselves.
    """
    # The same narrow loopback carve-out `0006` made for `anchor.value`, for the
    # same reason it gave: without it the suite cannot stand up a forge of its
    # own, and a repository anchor could only ever be tested against somebody
    # else's server -- which is to say, not tested. Loopback is not reachable by
    # anybody who is not already on the machine, and the crawler refuses it
    # unless `PODSHL_ALLOW_LOOPBACK` says otherwise.
    scheme = None
    for candidate in ("https://", "http://127.0.0.1:", "http://127.0.0.1/"):
        if url.startswith(candidate):
            scheme = "https://" if candidate == "https://" else "http://"
            break
    if not isinstance(url, str) or scheme is None:
        return None
    rest = url[len(scheme):]
    # A query or fragment on an identity is either a deep link into the forge's
    # own interface or an attempt to make two identities out of one repository.
    if "?" in rest or "#" in rest or "@" in rest:
        return None
    parts = [p for p in rest.split("/") if p != ""]
    if len(parts) != 3:
        return None
    host, owner, repo = parts
    host = host.lower()
    if host.startswith("www."):
        host = host[4:]
    # A port stays, because a self-hosted instance often has one -- the Gitea
    # this shape was measured against answers on 3141 -- and refusing one would
    # exclude exactly the case the shapes exist for. `:443` is stripped, since
    # keeping it would make two identities out of one endpoint.
    if host.endswith(":443"):
        host = host[: -len(":443")]
    bare, _, port = host.partition(":")
    if port and not (port.isdigit() and 1 <= int(port) <= 65535):
        return None
    if bare in RAW_HOSTS:
        # A raw host is a CDN path rather than a place anybody controls, and an
        # identity there would anchor a name under the forge's own name.
        return None
    host, lookup = host, bare

    known = KNOWN_HOSTS.get(lookup)
    if known is not None:
        if shape is not None and shape != known:
            return None
        shape = known
    elif shape is None:
        return None
    template = SHAPES.get(shape)
    if template is None:
        return None

    if repo.lower().endswith(".git"):
        repo = repo[: -len(".git")]
    if not _segments_ok(owner, repo):
        return None
    owner, repo = owner.lower(), repo.lower()
    identity = f"{scheme}{host}/{owner}/{repo}/"
    probe = template.format(host=host, owner=owner, repo=repo)
    if scheme != "https://":
        probe = probe.replace("https://", scheme, 1)
    return identity, probe


def supported() -> str:
    """What a refusal can name, so it says what would work instead."""
    hosts = ", ".join(sorted(KNOWN_HOSTS))
    shapes = ", ".join(sorted(SHAPES))
    return (f"{hosts} need no forge named; any other host needs one of "
            f"forge={shapes}")


def fetch_root(anchor: dict) -> str:
    """Where this anchor's challenge and files are actually fetched from.

    For a domain that is the identity itself; for a repository it is the forge's
    raw prefix, because a forge serves file contents from somewhere other than
    its web pages. Everything that fetches on an anchor's behalf goes through
    here, so the two never drift apart -- a probe reading one place while
    containment is checked against another would let a claim be proved by a file
    the mirror never touches.
    """
    return anchor.get("probe_prefix") or anchor["value"]


def host_only(identity: str) -> str:
    """The bare hostname out of an identity, for the `host` column.

    `anchor_host_is_punycode` admits no colon, and an identity may carry a port
    because self-hosted instances do. `host` stopped being the identity when
    repositories arrived -- it is shared by every repository on a forge -- so
    dropping the port from it costs nothing that anything still reads.
    """
    # Scheme-agnostic on purpose. Stripping only `https://` left the loopback
    # form intact, and the split then read `http` out of `http://127.0.0.1:.../`
    # as the host name -- which the database happily stored, because `http` is a
    # valid punycode label.
    rest = identity.split("://", 1)[-1]
    return rest.split("/", 1)[0].split(":", 1)[0].lower()

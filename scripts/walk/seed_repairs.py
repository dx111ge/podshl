"""Seed a state directory with repair records, for walking the panel.

The panel is drawn from `repairs.json` and shows what `repair::review` flags.
Seeding rather than recording live keeps the walk to the thing being walked:
`issue_state` is written with a fresh `checked_at`, so the window asks GitHub
nothing and a closed issue is a closed issue whatever the network is doing.

    python scripts/walk/seed_repairs.py <state-dir>

Writes the directory, a file to be "fixed", the copy `begin` would have kept,
and the records. Prints what it made.
"""
import hashlib
import json
import os
import shutil
import sys
import time


def main(root):
    root = os.path.abspath(root)
    shutil.rmtree(root, ignore_errors=True)
    os.makedirs(os.path.join(root, "backups", "walk-file"), exist_ok=True)

    # The file a fix was made in, and the copy from before the fix.
    target = os.path.join(root, "sway-like.conf")
    backup = os.path.join(root, "backups", "walk-file", "sway-like.conf")
    with open(backup, "w", encoding="utf-8", newline="\n") as f:
        f.write("# before the fix\nscale = 1\n")
    fixed = "# before the fix\nscale = 1\nfloat = podshl-client   # the fix\n"
    with open(target, "w", encoding="utf-8", newline="\n") as f:
        f.write(fixed)
    digest = hashlib.sha256(fixed.encode()).hexdigest()
    # ...and then something changed it again, which is what brings the panel up.
    #
    # Deliberately *not* the same text as the copy. It was, and that made the
    # walk's last step vacuous: "the file is back" is no evidence when the file
    # it was restored over already said the same thing.
    with open(target, "w", encoding="utf-8", newline="\n") as f:
        f.write("# before the fix\nscale = 2\n# and something else rewrote this\n")

    now = int(time.time())
    records = [
        {
            "id": "walk-file",
            "at": now - 3600,
            "action": "external:file",
            "params": {},
            "target": target,
            "backup": backup,
            "reversible": True,
            "subject": "ein anderes Werkzeug",
            "upstream": {"issue": "https://github.com/cli/cli/issues/14000"},
            "state": "applied",
            "note": "walk: the file case, with a copy and an upstream issue",
            "kind": "file",
            "file_sha256": digest,
            "watch_issue": False,
            # Asked already, and today: the window asks GitHub nothing.
            "issue_state": {"checked_at": now, "state": "closed", "is_pull": False},
        },
    ]
    # `{"records": [...]}`, not a bare list: `repair::read` looks for the key
    # and `load` swallows the difference, so a wrong shape here is a silent
    # empty ledger rather than an error.
    with open(os.path.join(root, "repairs.json"), "w", encoding="utf-8", newline="\n") as f:
        json.dump({"records": records}, f, indent=2)

    print(f"state dir : {root}")
    print(f"file      : {target}")
    print(f"backup    : {backup}")
    print(f"records   : {len(records)}")


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    main(sys.argv[1])

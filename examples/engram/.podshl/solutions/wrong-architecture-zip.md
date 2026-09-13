---
id: wrong-architecture-zip
answers:
  problem_class: engram.install.wrong-architecture-zip
  when:
    os.name: "linux"
    os.arch: "aarch64"
severity: medium
proposes:
  - action: report_only
    params: {}
    because: >-
      The fix is downloading a different archive. This client will not fetch and
      unpack a binary on somebody's machine, and it should not
---
This is an ARM64 Linux machine, and the archive most people land on is
`engram-linux-x86_64.zip`.

Downloaded on this machine that binary does not run, and what you see depends
on the shell: `cannot execute binary file: Exec format error`, or a bare
`No such file or directory` for a file you can plainly see.

Take `engram-linux-aarch64.zip` instead. Nothing else changes — the `.brain`
file is the same format on both, so an existing brain copies straight across.

    unzip engram-linux-aarch64.zip
    ./engram serve my.brain

If it still will not start after that, the next most likely thing is the
executable bit being lost by whatever unpacked it: `chmod +x engram`.

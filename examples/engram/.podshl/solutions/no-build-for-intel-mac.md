---
id: no-build-for-intel-mac
answers:
  problem_class: engram.install.no-build-for-this-platform
  when:
    os.name: "macos"
    os.arch: "x86_64"
severity: high
proposes:
  - action: report_only
    params: {}
    because: >-
      There is nothing to configure and nothing to undo. No archive exists for
      this combination, so any action here would be a guess dressed up as a fix
---
There is no engram build for an Intel Mac.

The release page carries four archives — `windows-x86_64`, `linux-x86_64`,
`linux-aarch64` and `macos-aarch64` — and `macos-aarch64` is Apple Silicon
only. On an Intel Mac none of the four will run, and the failure looks like the
binary doing nothing rather than like a missing build, which is why this is
worth saying out loud.

Two things that do work today:

* Run engram on another machine and point at it. It is an HTTP server; the web
  UI, the REST API and the MCP endpoint are all reachable over the network, so
  the Mac does not have to be the machine holding the `.brain` file.
* Run the Linux x86_64 archive inside a Linux VM on the Mac.

Rosetta will not help. It translates x86_64 for Apple Silicon, which is the
opposite direction.

If an Intel Mac build matters to you, say so on the issue tracker — that is a
build-matrix decision, and nobody knows it is wanted until somebody asks.

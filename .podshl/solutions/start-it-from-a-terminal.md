---
id: start-it-from-a-terminal
answers:
  problem_class: podshl.start.nothing-happens
  # No conditions: this is the answer that holds before anything has been read,
  # and every more specific one below overrides it. A class whose questions and
  # readings all led somewhere else would leave somebody with nothing.
  when: {}
severity: medium
proposes:
  - action: report_only
    because: >-
      Reading a program's own error message is something you do in your own
      terminal; this client does not start other programs to see what they say
---
**A program that will not start has nowhere to put its error.** A desktop icon
discards standard error, so the one line that says what went wrong is written
and thrown away, and clicking it appears to do nothing at all.

Start it from a terminal and the line is right there:

    ./podshl-client

On Windows, open PowerShell and run the installed program by its path under
`%LOCALAPPDATA%\PODSHL`; on macOS, `/Applications/PODSHL.app/Contents/MacOS/PODSHL`.

Common answers that line gives:

* `error while loading shared libraries: libwebkit2gtk-4.1.so` — the bare Linux
  binary needs `webkit2gtk-4.1` and `gtk3` installed. The `.deb` declares them
  and would have installed them for you.
* `Gdk-Message: Error 71 (Protocol error) dispatching to Wayland display` — the
  Wayland case, which has its own answer here.
* Nothing at all, and it simply exits — that is worth reporting, with the
  version and the system, because it is not a failure we have seen.

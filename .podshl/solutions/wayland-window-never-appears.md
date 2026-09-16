---
id: wayland-window-never-appears
answers:
  problem_class: podshl.start.nothing-happens
  when:
    session.type: wayland
severity: high
proposes:
  - action: report_only
    because: >-
      The fix is either a newer download or one environment variable you set
      yourself, and both are yours to do — this client does not replace itself
      and does not edit your shell profile
---
**Nothing appears, and there is no error on screen.** Under Wayland the window
toolkit this client uses, WebKitGTK, takes an accelerated path that fails on
some drivers — and it does not fail by drawing a black window. The program exits
before any window exists, with a single line on standard error, and a desktop
icon throws that line away. Clicking it looks exactly like nothing happening.

Measured on Omarchy 4.0.2 — Hyprland 0.56.2, WebKitGTK 2.52.6, NVIDIA
610.57.04 — where it exits with `Gdk-Message: Error 71 (Protocol error)
dispatching to Wayland display.`

**Since 0.1.2 the client turns that path off for itself**, in its own process,
before the window is created. Nothing outside this program is affected and you
do not have to do anything. So if nothing happens when you start it, one of two
things is true:

* **You are running something older than 0.1.2.** Download the current release
  and the problem goes with it.
* **You asked for the accelerated path back.** Setting
  `WEBKIT_DISABLE_DMABUF_RENDERER` to `0` yourself tells the client to leave
  your choice alone, and this is the failure that default exists for. Unset it,
  or set it to `1`.

To see the line the icon discards, start it from a terminal:

    ./podshl-client

That line is also the thing worth sending if neither of the above is it — this
is the one failure where there is no diagnosis left to run, because the window
that would run it never opened.

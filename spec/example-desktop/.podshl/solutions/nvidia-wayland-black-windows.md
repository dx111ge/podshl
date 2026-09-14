---
id: nvidia-wayland-black-windows
answers:
  problem_class: nvidia.driver.wayland-black-windows
  when:
    session.type: "wayland"
severity: high
proposes:
  - action: report_only
    params: {}
    because: >-
      The fix is an environment variable for one application, and setting one
      for you would mean deciding where it belongs — a shell profile, a
      desktop file, a systemd unit — which is your machine's layout, not ours
---
Your windows are black because the application is handing the compositor a
frame before the GPU has finished drawing it, and under Wayland nothing puts
those back in order for you.

This is not a bug in the application and not a bug in this desktop. Until
driver 555 the NVIDIA proprietary stack had no explicit synchronisation on
Wayland, so a buffer could be shown before its fence signalled. On X11 the same
setup works, which is why this looks like it started when you switched.

The reliable workaround for Electron and Chromium applications is to disable
the DMA-BUF renderer for that application:

    WEBKIT_DISABLE_DMABUF_RENDERER=1

For a GTK application, forcing it through XWayland has the same effect:

    GDK_BACKEND=x11

Both are per-application on purpose. Setting either globally will make things
worse elsewhere, because everything that *does* work under Wayland will stop
using it.

Driver 555 or newer together with a compositor that supports explicit sync was
supposed to end this, and for the black windows it largely did.

**It is not the whole story, and this answer used to say it was.** This rule
matched only `gpu.driver_version: "< 555"` until somebody ran a WebKitGTK
application on 610.57.04 under Hyprland 0.56.2 and it did not draw a black
window — it exited before any window existed, with
`Gdk-Message: Error 71 (Protocol error) dispatching to Wayland display.` and
nothing else. The same variable fixes it. Whether that is this bug surviving or
a second one wearing its clothes, nobody here has established, and the honest
consequence is that **we do not know where the boundary is** — so this answer no
longer draws one. It is offered to everyone on Wayland, and people who did not
need it will say so, which is information we do not have yet.

If you are on 555 or later and seeing either symptom, that is worth reporting —
to us, because at that point it is ours.

A note for anyone publishing their own rules from this example: the version
range that was here read well and was never measured. A `when:` that is too
narrow does not fail loudly. It simply never reaches the people who have the
problem, and you find out from nobody.

Nothing is changed for you here deliberately. This agent can set a key in a
configuration file, and an environment variable for one application is not
that: where it belongs depends on how you launch the thing, and guessing wrong
would leave a setting somewhere you will not find again in six months.

---
id: nvidia-wayland-cursor-flicker
answers:
  problem_class: nvidia.driver.wayland-cursor-flicker
  when:
    session.type: "wayland"
    nvidia.drm_modeset: "N"
severity: medium
proposes:
  - action: report_only
    params: {}
    because: >-
      This needs a kernel module parameter and a rebuilt initramfs, which is a
      privileged change to how the machine boots — well outside anything this
      agent should do behind a dialog
---
The cursor flickers, tears or disappears because kernel modesetting is off for
the NVIDIA driver, so the compositor is not driving the display through the
path it expects.

`nvidia_drm.modeset=1` is what turns that on. Most distributions set it for
you now; yours has not, or an upgrade dropped it.

Where it goes depends on your distribution — a file in `/etc/modprobe.d/`, a
kernel command line entry, or a distribution-specific tool — and the initramfs
has to be rebuilt afterwards or nothing changes. Your distribution's
documentation is the right source for which of those it is; ours would be a
guess about your machine.

You can check the current state with:

    cat /sys/module/nvidia_drm/parameters/modeset

If that prints `N`, this is you. If the file does not exist at all, the module
is not loaded, which is a different problem and worth opening an issue about.

Deliberately no automated action. This changes how the machine boots, needs
root, and gets it wrong on a machine that will not come back up — the kind of
change that has to be made by someone who can see the whole system, reading
their own distribution's instructions.

# A plain Linux with a real user session, for walking `repairs install-hook`.
#
# **The gap this closes.** On Linux that is not Omarchy, `install-hook` sets up
# a systemd *user* timer. That path had only ever been walked in a container
# with no user bus, where it fails cleanly and exits 1 — which proves the
# refusal and nothing else. Whether the units it writes are ones systemd
# accepts, whether the timer appears, and whether a review that found something
# (exit 3) reads as a failed unit, needs a session: `systemd` as pid 1,
# `systemd-logind`, and `user@<uid>.service` running.
#
# Deliberately plain Debian rather than this project's build image. The
# question is what happens on somebody else's machine, and an image carrying
# our build dependencies is not that machine. Only the runtime libraries the
# client links are installed, which is what a person installing the `.deb`
# would get.
#
#   docker build -f docker/systemd-walk.Dockerfile -t podshl-systemd-walk:local .
#   docker run -d --name podshl-systemd-walk --privileged --cgroupns=host \
#       -v /sys/fs/cgroup:/sys/fs/cgroup:rw \
#       -v "$PWD/var/walk/podshl-client:/usr/local/bin/podshl-client:ro" \
#       podshl-systemd-walk:local
#
# `--privileged` is what systemd in a container needs; this image exists to be
# thrown away after the walk and is not part of the suite.
FROM debian:trixie

ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
      systemd systemd-sysv dbus dbus-user-session \
      libwebkit2gtk-4.1-0 libgtk-3-0 librsvg2-2 \
      libnotify-bin ca-certificates procps sudo \
 && rm -rf /var/lib/apt/lists/*

# An ordinary person, not root: the timer is a *user* timer, and root's session
# is not the one it would live in.
RUN useradd --create-home --shell /bin/bash walker

# Nothing in this image should start the client itself. The walk does that.
STOPSIGNAL SIGRTMIN+3
CMD ["/sbin/init"]

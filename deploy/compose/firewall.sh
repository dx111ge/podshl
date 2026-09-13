#!/usr/bin/env bash
# Keeps the operator's containers out of the network they run in.
#
#   sudo ./firewall.sh apply     install the rules (idempotent)
#   sudo ./firewall.sh remove    take them out again
#   sudo ./firewall.sh status    show them
#
# For a host that is not alone on its network — a machine at home, beside a
# router, a Git server and other people's devices. The crawler already refuses
# private addresses before it connects (`anchor/challenge.py`); this is the wall
# behind that check, for the day something in a container is not our code any
# more. With it, every container on the two compose networks may reach:
#
#   * each other (172.30.0.0/16 — the ranges compose.yaml fixes)
#   * the public internet
#   * nothing private: not the LAN, not the router, not link-local or CGNAT
#   * nothing on this host itself: not SSH, not a Git server, not the Docker API
#
# Replies to connections made *to* the containers (Caddy's visitors, the ssh
# tunnel to the ops view) are untouched: established traffic is let through
# first. DNS goes to public resolvers (compose.yaml `dns:`), because the router
# is on the LAN and is refused like everything else there.
#
# IPv4 only, because the compose networks are IPv4 only.
set -euo pipefail

OURS=172.30.0.0/16
PRIVATE=(10.0.0.0/8 172.16.0.0/12 192.168.0.0/16 169.254.0.0/16 100.64.0.0/10 224.0.0.0/4 0.0.0.0/8)

need_root() { [ "$(id -u)" = 0 ] || { echo "firewall: run as root" >&2; exit 1; }; }

apply() {
  need_root
  iptables -nL DOCKER-USER >/dev/null 2>&1 || { echo "firewall: no DOCKER-USER chain — is Docker running?" >&2; exit 1; }

  # Forwarded traffic: container to anywhere that is not the host.
  iptables -N PODSHL-OUT 2>/dev/null || iptables -F PODSHL-OUT
  iptables -A PODSHL-OUT -m conntrack --ctstate ESTABLISHED,RELATED -j RETURN
  iptables -A PODSHL-OUT -d "$OURS" -j RETURN
  for net in "${PRIVATE[@]}"; do
    iptables -A PODSHL-OUT -d "$net" -j DROP
  done
  iptables -A PODSHL-OUT -j RETURN
  iptables -C DOCKER-USER -s "$OURS" -j PODSHL-OUT 2>/dev/null \
    || iptables -I DOCKER-USER 1 -s "$OURS" -j PODSHL-OUT

  # Traffic to the host itself goes through INPUT, not DOCKER-USER, whatever
  # address of the host it is aimed at.
  iptables -N PODSHL-HOST 2>/dev/null || iptables -F PODSHL-HOST
  iptables -A PODSHL-HOST -m conntrack --ctstate ESTABLISHED,RELATED -j RETURN
  iptables -A PODSHL-HOST -j DROP
  iptables -C INPUT -s "$OURS" -j PODSHL-HOST 2>/dev/null \
    || iptables -I INPUT 1 -s "$OURS" -j PODSHL-HOST

  echo "firewall: applied for $OURS"
}

remove() {
  need_root
  while iptables -D DOCKER-USER -s "$OURS" -j PODSHL-OUT 2>/dev/null; do :; done
  while iptables -D INPUT -s "$OURS" -j PODSHL-HOST 2>/dev/null; do :; done
  iptables -F PODSHL-OUT 2>/dev/null && iptables -X PODSHL-OUT || true
  iptables -F PODSHL-HOST 2>/dev/null && iptables -X PODSHL-HOST || true
  echo "firewall: removed"
}

status() {
  iptables -S DOCKER-USER | grep PODSHL || echo "DOCKER-USER: no jump"
  iptables -S INPUT | grep PODSHL || echo "INPUT: no jump"
  iptables -S PODSHL-OUT 2>/dev/null || true
  iptables -S PODSHL-HOST 2>/dev/null || true
}

case "${1:-status}" in
  apply) apply ;;
  remove) remove ;;
  status) status ;;
  *) echo "usage: firewall.sh apply|remove|status" >&2; exit 2 ;;
esac

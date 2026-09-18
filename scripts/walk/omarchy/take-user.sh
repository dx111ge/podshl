#!/bin/bash
# One continuous take of the client on Omarchy, driven by pointer and keyboard.
#
# There is no DevTools protocol on WebKitGTK, so unlike the Windows recorder
# this cannot click a selector or scroll to an element. Every coordinate below
# was read off a screenshot of the real window during a dry run, and the whole
# point of doing it as one script is that the take is continuous: a person
# watching should see one session, not a stitch.
#
# Window is floated at 800,343 960x780 by the rule in hyprland.lua. ydotool's
# absolute coordinates are half of screen pixels on this 2560x1440 output.
set -uo pipefail
export XDG_RUNTIME_DIR=/run/user/1000 WAYLAND_DISPLAY=wayland-1
export YDOTOOL_SOCKET=/run/user/1000/.ydotool_socket

SRT=${SRT:-/tmp/take.srt}
: > "$SRT"
N=0
T0=$(date +%s.%N)

now() { echo "$(date +%s.%N) $T0" | awk '{printf "%.3f", $1-$2}'; }
ts()  { awk -v t="$1" 'BEGIN{h=int(t/3600);m=int((t%3600)/60);s=t-h*3600-m*60;printf "%02d:%02d:%06.3f",h,m,s}' | tr '.' ','; }

# sub "text" seconds — writes one subtitle covering the next `seconds`
sub() {
  local text="$1" dur="$2" start end
  start=$(now); end=$(echo "$start $dur" | awk '{print $1+$2}')
  N=$((N+1))
  { echo "$N"; echo "$(ts "$start") --> $(ts "$end")"; echo "$text"; echo; } >> "$SRT"
  sleep "$dur"
}
click() { ydotool mousemove --absolute -x "$1" -y "$2"; sleep 0.4; ydotool click 0xC0; }

sub "A fix somebody made on this machine is still here — and something has moved since." 5
sub "PODSHL says so at the start. It undoes nothing by itself." 4

click 558 545; sleep 0.6
sub "Ask about something that is broken." 1
wtype "Search in engram returns nothing"
sub "" 2
click 708 545; sleep 0.6
wtype "engram"
sub "The project, by name." 2
click 790 545
sub "engram publishes support files in its own repository. Nothing was installed for this." 6

click 526 326
sub "It offers to check where this machine got engram from. You can decline." 6

sub "These are the problems engram says it can answer — their words, not a guess." 6
click 477 437; sleep 0.6
sub "Search stopped finding things it used to find." 3
click 500 500
sub "" 8

sub "Now it asks. Item by item, with the reason, and what it would read." 7
sub "Nothing has been sent yet." 3
click 517 500
sub "" 8

sub "Three things only a person can answer. They can be left blank." 6
click 639 416; sleep 1
click 541 446; sleep 0.6
sub "Yes — and the reindex was never run." 3
click 498 509
sub "" 8

sub "This is everything that would leave the machine, before it leaves." 7
sub "Unanswered questions go as unanswered, rather than being left out." 5
click 491 500
sub "" 10

sub "engram's own answer, from the files engram publishes." 6
sub "The amber line marks what you said, not what was measured." 5
click 498 511
sub "" 6

sub "Whether it worked is the one thing only you can tell them." 5
click 494 500
sub "" 8

sub "And what nothing covers becomes Markdown you take to the project — anonymised, and shown first." 8

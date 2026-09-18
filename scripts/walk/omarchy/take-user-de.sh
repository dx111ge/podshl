#!/bin/bash
# The German cut of the same take. Every coordinate was measured again: the
# German strings are longer, so the panels sit differently and the English
# numbers do not carry over.
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

sub "Eine Korrektur, die hier jemand gemacht hat, ist noch da — und seitdem hat sich etwas bewegt." 6
sub "PODSHL sagt das beim Start. Von selbst macht es nichts rückgängig." 5

click 563 545; sleep 0.6
sub "Frag nach etwas, das kaputt ist." 2
wtype "Die Suche in engram findet nichts mehr"
sub "" 2
click 718 545; sleep 0.6
wtype "engram"
sub "Das Projekt, beim Namen." 2
click 795 545
sub "engram veröffentlicht seine Hilfe im eigenen Repository. Dafür wurde nichts installiert." 7

click 569 326
sub "Es bietet an nachzusehen, woher engram kommt. Du kannst ablehnen." 7

sub "Das sind die Probleme, die engram beantworten kann — ihre Worte, nicht geraten." 7
click 477 449; sleep 0.6
sub "Die Suche findet Dinge nicht mehr, die sie früher gefunden hat." 4
click 507 512
sub "" 9

sub "Jetzt fragt es. Einzeln, mit dem Grund, und was es lesen würde." 7
sub "Gesendet wurde noch nichts." 3
click 527 500
sub "" 9

sub "Drei Angaben, die nur ein Mensch beantworten kann. Sie dürfen leer bleiben." 7
click 639 416; sleep 1
click 541 446; sleep 0.6
sub "Ja — und die Neuindizierung lief nie." 3
click 494 509
sub "" 9

sub "Das ist alles, was das Gerät verlassen würde, bevor es geht." 7
sub "Unbeantwortete Fragen gehen als unbeantwortet mit, statt weggelassen zu werden." 6
click 495 500
sub "" 12

sub "engrams eigene Antwort, aus den Dateien, die engram veröffentlicht." 7
sub "Die orange Zeile markiert, was du gesagt hast — nicht, was gemessen wurde." 6
click 510 512
sub "" 7

sub "Ob es funktioniert hat, kann nur du ihnen sagen." 5
click 495 500
sub "" 9

sub "Und was nichts abdeckt, wird Markdown für das Projekt — anonymisiert, und vorher gezeigt." 9

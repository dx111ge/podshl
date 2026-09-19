#!/bin/bash
# One continuous take of podshl-repairs on a clean Omarchy, in a real terminal
# on the real desktop, with a real Claude.
#
# **Pasted, not typed.** `wtype` lost characters on this layout: a rehearsal
# turned `http://127.0.0.1:8765` into `http//127.0.0.18765`, and every command
# after the download ran against a program never installed, while the
# subtitles went on describing a story that was not happening. So commands go
# through the clipboard, are shown for a moment, then run; and every step that
# matters is checked, and the take stops at the first that did not happen.
#
# **Claude is used the way a person uses it**: `claude`, a request typed in plain
# words, Enter on its question before it writes, `/exit`. The take waits for
# the record the agent hook writes, not for a guessed number of seconds, and
# presses Enter now and then while it waits — the answer to its question
# before an edit, whose first choice is Yes. The trust question in a new folder
# is answered once, at the start, with Down and Enter, because its first
# choice is No. An Enter or a Down with nothing to answer does nothing.
#
# **Recorded with gpu-screen-recorder**, as Omarchy does; the screen is
# recorded whole, and `enc-repairs.sh` crops it, pans to the notice at the
# times written to `$DIR/pan`, and puts the subtitles in a band of their own.
#
# Needs: a clean machine (no podshl-repairs, no ledger, no hooks, no
# podshl-bin), Claude as Omarchy's default agent and logged in, ydotoold.
#
#   take-repairs.sh <dir>                    # the published release
#   INSTALL=… RAW=… PODSHL_RELEASES=… PODSHL_VERSION=… take-repairs.sh <dir>   # a mirror
set -uo pipefail
export XDG_RUNTIME_DIR=/run/user/1000 WAYLAND_DISPLAY=wayland-1
export YDOTOOL_SOCKET=/run/user/1000/.ydotool_socket
export HYPRLAND_INSTANCE_SIGNATURE=$(ls -t /run/user/1000/hypr | head -1)
export PATH=$HOME/.local/bin:$PATH
DIR=${1:?output directory}
INSTALL=${INSTALL:-https://raw.githubusercontent.com/dx111ge/podshl/main/packaging/repairs/install.sh}
RAW=${RAW:-https://raw.githubusercontent.com/dx111ge/podshl/main/packaging/aur/podshl-bin}
R=$HOME/.local/bin/podshl-repairs
LF=$HOME/.config/hypr/looknfeel.lua
rm -rf "$DIR"; mkdir -p "$DIR" ~/podshl-pkg

# Windows titled PODSHL: this take's terminal, and the review a click on the
# notice opens. One left from an earlier take was taken for this one's — the
# first by that title — and every command went to a window nobody watched.
podshl_windows() { hyprctl clients -j | python3 -c '
import json,sys
print(" ".join(c["address"] for c in json.load(sys.stdin) if c["title"]=="PODSHL"))'; }
close_podshl_windows() {
  for a in $(podshl_windows); do
    hyprctl dispatch "hl.dsp.window.close({ window = \"address:$a\" })" >/dev/null
  done
}
[ -z "$(podshl_windows)" ] || { echo "PODSHL windows are open from before: reset first" >&2; exit 1; }
trap close_podshl_windows EXIT

hyprctl dispatch "hl.dsp.focus({ workspace = 9 })" >/dev/null
setsid uwsm-app -- ghostty --class=org.omarchy.terminal --title=PODSHL --font-size=17 \
  --working-directory="$HOME" -e bash --noprofile --norc </dev/null >/dev/null 2>&1 &
sleep 3
read -r ADDR GPID < <(hyprctl clients -j | python3 -c '
import json,sys
c=next(c for c in json.load(sys.stdin) if c["title"]=="PODSHL")
print(c["address"], c["pid"])')
hyprctl dispatch "hl.dsp.window.resize({ window = \"address:$ADDR\", x = 1800, y = 1000 })" >/dev/null
hyprctl dispatch "hl.dsp.window.move({ window = \"address:$ADDR\", x = 1020, y = 40 })" >/dev/null
SHELLPID=$(pgrep -P "$GPID" -x bash | head -1)
[ -n "$SHELLPID" ] || { echo "no shell in the take terminal" >&2; exit 1; }
ydotool mousemove --absolute -x 500 -y 520

# Checks read uncommented lines only: Omarchy's looknfeel.lua carries every
# option as a commented example (`--     gaps_in = 0,`).

# ---------------------------------------------------------------- helpers
paste_() { printf '%s' "$1" | wl-copy >/dev/null 2>&1; sleep 0.2; wtype -M ctrl -M shift v -m shift -m ctrl; }
idle() { sleep 0.8; while pgrep -P "$SHELLPID" >/dev/null; do sleep 0.3; done; sleep 0.6; }
run_() { paste_ "$1"; sleep 1.1; wtype -k Return; }
t() { run_ "$1"; idle; }
cls() { wtype -M ctrl l -m ctrl; sleep 0.5; }
records() { [ -x "$R" ] && "$R" list --json 2>/dev/null || echo '[]'; }
# The newest record whose `by` contains $1 and whose file or package contains $2.
latest() {
  records | python3 -c "
import json,sys
try: rs=json.load(sys.stdin)
except Exception: rs=[]
rs=[r for r in rs if sys.argv[1] in r['subject'] and sys.argv[2] in ((r.get('target') or '')+' '+(r.get('upstream',{}).get('package') or ''))]
print(rs[-1]['id'] if rs else '')" "$1" "$2"
}
count() { records | python3 -c "
import json,sys
print(sum(1 for r in json.load(sys.stdin) if sys.argv[1] in r['subject'] and sys.argv[2] in (r.get('target') or '')))" "$1" "$2"; }
state_of() { records | python3 -c "
import json,sys
print(next((r['state'] for r in json.load(sys.stdin) if r['id']==sys.argv[1]),''))" "$1"; }

# Off camera: what an ordinary Omarchy shell would have (bash without its
# startup files has neither the prompt nor OMARCHY_PATH), plus a mirror's
# release location when rehearsing.
OP=$(bash -ic 'echo "$OMARCHY_PATH"' 2>/dev/null | tail -1)
extra=""
[ -n "${PODSHL_RELEASES:-}" ] && extra="$extra PODSHL_RELEASES=$PODSHL_RELEASES"
[ -n "${PODSHL_VERSION:-}" ] && extra="$extra PODSHL_VERSION=$PODSHL_VERSION"
run_ "export OMARCHY_PATH=$OP$extra PS1='\$ '; clear"; idle
sleep 1

SRT="$DIR/take.srt"; PAN="$DIR/pan"; : > "$SRT"; : > "$PAN"; N=0
T0=$(date +%s.%N)
gpu-screen-recorder -w HDMI-A-1 -f 30 -fm cfr -k h264 -q very_high -cursor yes \
  -o "$DIR/screen.mp4" 2>"$DIR/recorder.log" &
REC=$!
sleep 1.5
now() { echo "$(date +%s.%N) $T0" | awk '{printf "%.3f", $1-$2}'; }
ts()  { awk -v t="$1" 'BEGIN{h=int(t/3600);m=int((t%3600)/60);s=t-h*3600-m*60;printf "%02d:%02d:%06.3f",h,m,s}' | tr '.' ','; }
OPEN=""
endsub() {
  [ -z "$OPEN" ] && return
  N=$((N+1)); { echo "$N"; echo "$(ts "$OPEN_START") --> $(ts "$(now)")"; echo "$OPEN"; echo; } >> "$SRT"
  OPEN=""
}
sub() {
  local start end
  endsub
  start=$(now); end=$(echo "$start $2" | awk '{print $1+$2}')
  if [ -n "$1" ]; then
    N=$((N+1)); { echo "$N"; echo "$(ts "$start") --> $(ts "$end")"; echo "$1"; echo; } >> "$SRT"
  fi
  sleep "$2"
}
subo() { endsub; OPEN_START=$(now); OPEN="$1"; }
stop_rec() { kill -INT "$REC" 2>/dev/null; wait "$REC" 2>/dev/null; }
must() {
  local what=$1; shift
  if ! "$@" >/dev/null 2>&1; then
    endsub; stop_rec
    echo "TAKE STOPPED: $what did not happen" | tee "$DIR/FAILED" >&2
    exit 1
  fi
}

# ask "request" check: open Claude, ask, answer its questions, leave.
ask() {
  local request=$1 check=$2 n=0
  run_ "claude"
  sleep 8
  # The trust question in a folder Claude has not seen offers "No, exit"
  # first: the rehearsal's Enter chose it, Claude left, and the request went
  # to bash. Down to "Yes, I trust this folder", then Enter. Where there is no
  # question, Down in an empty prompt does nothing.
  wtype -k Down; sleep 0.6; wtype -k Return
  sleep 4
  # Typed, as a person does. A pasted request is one Claude may take for
  # quoted text rather than an instruction, and ask back (take 10). The
  # requests are words and a full stop: none of the characters wtype lost.
  wtype -d 45 "$request"; sleep 1.5; wtype -k Return
  until "$check"; do
    sleep 3; n=$((n + 1))
    [ $((n % 3)) -eq 0 ] && wtype -k Return    # its question before an edit
    [ $n -gt 90 ] && must "Claude's change" false
  done
  sleep 6
  paste_ "/exit"; sleep 1; wtype -k Return
  idle
}
BEFORE=0
new_hypr_record() { [ "$(count claude /.config/hypr/)" -gt "$BEFORE" ] && [ "$(state_of "$(latest claude /.config/hypr/)")" = applied ]; }
repo_changed() { grep -q "x = 1" ~/code-demo/settings.py 2>/dev/null; }

# ---------------------------------------------------------------- the take
sub "A fix outlives its reason. You, a script or an agent change something, and a year later nobody knows why it is there." 7
sub "podshl-repairs keeps a record of such changes and looks at them again after every update. This is a clean Omarchy." 7

# 1. install
subo "One line: the newest release, checked against its sums, into ~/.local/bin without root — with its hooks and its manual."
t "curl -fsSL $INSTALL | bash"
must "the install" test -x "$R"
must "the update hook" test -f ~/.config/omarchy/hooks/post-update.d/podshl-repairs
must "the agent hook" grep -q agent-hook ~/.claude/settings.json
sub "" 3
subo "The manual is there too."
run_ "man podshl-repairs"; sleep 6; wtype q; idle

# 2. an agent changes a setting
cls
subo "Now open Claude, Omarchy's default agent here, and ask for a change in your own words."
BEFORE=$(count claude /.config/hypr/)
ask "Make the gaps between my windows a bit smaller." new_hypr_record
R1=$(latest claude /.config/hypr/)
subo "Nobody typed anything for the record."
t "podshl-repairs list"
sub "It is there anyway: by claude, with a copy of the file from before the change." 6

# 3. undo it
subo "You do not like it. Undo."
t "podshl-repairs restore $R1"
must "the undo" bash -c "[ \"\$('$R' list --json | python3 -c 'import json,sys; print(next(r[\"state\"] for r in json.load(sys.stdin) if r[\"id\"]==\"$R1\"))')\" = undone ]"
sub "The file is back, byte for byte." 4

# 4. a change you keep, overwritten
cls
subo "A change you keep, asked for the same way."
BEFORE=$(count claude /.config/hypr/)
ask "Dim the windows I am not using." new_hypr_record
R2=$(latest claude /.config/hypr/)
must "the change being in looknfeel.lua" bash -c "'$R' list --json | python3 -c 'import json,sys; r=next(r for r in json.load(sys.stdin) if r[\"id\"]==\"$R2\"); sys.exit(0 if (r.get(\"target\") or \"\").endswith(\"/looknfeel.lua\") else 1)'"
subo "Weeks later Omarchy puts its own default back: an update, a migration, a refresh."
t "omarchy-refresh-config hypr/looknfeel.lua"
must "Omarchy's reset" bash -c "! grep -qE '^[[:space:]]*dim_inactive' '$LF'"
sub "Your change is gone, and your rounded corners with it. Nothing tells you." 6
cls
subo "After every update, Omarchy runs the hook."
t "bash ~/.config/omarchy/hooks/post-update.d/podshl-repairs"
must "the notice" bash -c "ls ~/.local/state/omarchy/notifications/*.json | xargs grep -l '\"PODSHL\"'"
echo "$(now) right" >> "$PAN"
sub "This time something does. The notice stays until you look at it." 6
ydotool mousemove --absolute -x 1800 -y 45; sleep 1.2
ydotool click 0xC0
sleep 1.8
echo "$(now) centre" >> "$PAN"
sub "One click: which change, and why it wants a look." 7
wtype " "; sleep 1.5
subo "Undo goes back to before Claude touched the file, so your rounded corners return."
t "podshl-repairs restore $R2"
must "the second undo" grep -qE "^[[:space:]]*rounding = 8" "$LF"
sub "" 3

# 5. what a package declares
cls
subo "Packages can say why they leave the ordinary path. PODSHL's own client is built from its PKGBUILD:"
t "cd ~/podshl-pkg"
t "for f in PKGBUILD podshl-client.desktop podshl-bin.repairs.json; do curl -fsSLO $RAW/\$f; done"
t "makepkg -si --noconfirm"
must "the client package" pacman -Q podshl-bin
cls
subo "The AUR step of every omarchy-update:"
t "omarchy-update-aur-pkgs"
sub "It asks the AUR about podshl-bin. The package is not there, and the name is free for anybody to register." 7
t "podshl-repairs list"
P=$(latest "declared by podshl-bin" podshl-bin)
must "the package's declaration" test -n "$P"
subo "Nobody wrote that down on this machine. The package says it:"
t "podshl-repairs show $P"
sub "Its maintainer's reason, attributed by pacman. A later version withdraws it, and the record will say so." 8
t "cd"

# 6. a workaround waiting on upstream
cls
subo "An older workaround, for a Hyprland bug, watched on GitHub."
t "podshl-repairs add --kind package --by me --package hyprland --issue https://github.com/hyprwm/Hyprland/issues/7564 --watch"
t "podshl-repairs review"
must "the closed issue" bash -c "grep -q 'upstream asked about https://github.com/hyprwm/Hyprland/issues/7564: closed' ~/.local/state/podshl/client.log"
sub "Asked only because you said so. The issue is closed: time to see whether the workaround can go. Nothing is removed by itself." 8

# 7. inside a repository
cls
subo "Code in a git repository has a history of its own."
t "mkdir ~/code-demo && cd ~/code-demo && git init -q && echo 'x = 0' > settings.py"
ask "Change x to 1 in settings.py." repo_changed
t "cat settings.py; podshl-repairs list"
sub "Changed, and not recorded: git already has it. The record is for what has no history." 7
t "cd"

# 8. the record
cls
t "podshl-repairs list"
sub "What was changed on this machine, by whom, and why, looked at again after every update." 7
sub "podshl-repairs · man podshl-repairs · github.com/dx111ge/podshl" 5
endsub

stop_rec
echo "$(now)" > "$DIR/duration"
echo "take done: $(du -h "$DIR/screen.mp4" | cut -f1) over $(cat "$DIR/duration")s"

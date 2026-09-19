#!/bin/bash
# One continuous take of podshl-repairs on a clean Omarchy, in a real terminal
# on the real desktop.
#
# Unlike `take-user.sh` there is no window to click through: the record is a
# command-line program, so the take puts commands into a Ghostty and waits for
# the shell to be idle again — no fixed sleeps guessing how long Claude or
# makepkg takes. Two things are pointer work: the notice Omarchy shows after
# the post-update hook, and the terminal its click opens.
#
# **Pasted, not typed.** `wtype` lost characters on this layout: the first
# rehearsal turned `http://127.0.0.1:8765` into `http//127.0.0.18765` and
# `$RAW/PKGBUILD` into `PKGB;SuILD`, and every command after the download ran
# against a program that was never installed — while the subtitles went on
# describing the story as if it had worked. So each command goes through the
# clipboard, is shown for a moment, then runs; and every step that matters is
# checked, and the take stops at the first one that did not happen.
#
# **Recorded with gpu-screen-recorder**, which Omarchy uses itself: `grim` on a
# timer managed 1.5 frames a second at 3840x1080. The screen is recorded
# whole; `enc-repairs.sh` crops the middle to 1920x1080 and pans right while
# the notice (top right) is the thing to look at, at the times written to
# `$DIR/pan` as the take happens, like the subtitles.
#
# Needs: a clean machine (no podshl-repairs, no ledger, no hooks), Claude set
# as Omarchy's default agent and logged in, ydotoold running.
#
#   REL=… RAW=… take-repairs.sh <dir>   # screen.mp4, take.srt, pan in <dir>
set -uo pipefail
export XDG_RUNTIME_DIR=/run/user/1000 WAYLAND_DISPLAY=wayland-1
export YDOTOOL_SOCKET=/run/user/1000/.ydotool_socket
export HYPRLAND_INSTANCE_SIGNATURE=$(ls -t /run/user/1000/hypr | head -1)
export PATH=$HOME/.local/bin:$PATH
DIR=${1:?output directory}
REL=${REL:-https://github.com/dx111ge/podshl/releases/download/v0.1.7}
RAW=${RAW:-https://raw.githubusercontent.com/dx111ge/podshl/main/packaging/aur/podshl-bin}
R=$HOME/.local/bin/podshl-repairs
LF=$HOME/.config/hypr/looknfeel.lua
rm -rf "$DIR"; mkdir -p "$DIR"

# ---------------------------------------------------------------- preparation
# The repository script the AUR case shows and runs, written here so the take
# shows exactly what it runs.
mkdir -p ~/podshl-pkg
cat > ~/podshl-pkg/local-repo.sh <<'EOF'
#!/bin/bash
# A local pacman repository holding podshl-bin, so it is no longer a foreign
# package and no AUR helper looks its name up.
set -e
install -d -m755 /var/lib/podshl/repo
cp podshl-bin-*.pkg.tar.zst /var/lib/podshl/repo/
repo-add -q /var/lib/podshl/repo/podshl.db.tar.gz /var/lib/podshl/repo/podshl-bin-*.pkg.tar.zst
printf '\n[podshl]\nSigLevel = Optional TrustAll\nServer = file:///var/lib/podshl/repo\n' >> /etc/pacman.conf
pacman -Sy --noconfirm >/dev/null
echo "podshl-bin now comes from the local repository [podshl]"
EOF

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
# option as a commented example (`--     gaps_in = 0,`), and a plain grep for
# the option found it in every state — which stopped the second rehearsal at an
# undo that had worked.

# ---------------------------------------------------------------- helpers
paste_() { printf '%s' "$1" | wl-copy >/dev/null 2>&1; sleep 0.2; wtype -M ctrl -M shift v -m shift -m ctrl; }
idle() { sleep 0.8; while pgrep -P "$SHELLPID" >/dev/null; do sleep 0.3; done; sleep 0.6; }
run_() { paste_ "$1"; sleep 1.1; wtype -k Return; }
t() { run_ "$1"; idle; }
cls() { wtype -M ctrl l -m ctrl; sleep 0.5; }

# Off camera: what an ordinary Omarchy shell would have. The take runs bash
# without its startup files, so it has neither Omarchy's prompt setup nor
# OMARCHY_PATH — and without that, `omarchy-refresh-config` said "Not a
# shipped user config" and stopped the third rehearsal. Taken from an
# interactive shell rather than written here.
OP=$(bash -ic 'echo "$OMARCHY_PATH"' 2>/dev/null | tail -1)
run_ "export OMARCHY_PATH=$OP PS1='\$ '; clear"; idle
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
# sub "text" seconds: one subtitle for the next `seconds`, and wait them out.
sub() {
  local start end
  endsub
  start=$(now); end=$(echo "$start $2" | awk '{print $1+$2}')
  if [ -n "$1" ]; then
    N=$((N+1)); { echo "$N"; echo "$(ts "$start") --> $(ts "$end")"; echo "$1"; echo; } >> "$SRT"
  fi
  sleep "$2"
}
# subo "text": held until the next subtitle starts, for a command whose length
# nobody knows in advance.
subo() { endsub; OPEN_START=$(now); OPEN="$1"; }
stop_rec() { kill -INT "$REC" 2>/dev/null; wait "$REC" 2>/dev/null; }
# must "what" test...: the step happened, or the take stops here.
must() {
  local what=$1; shift
  if ! "$@" >/dev/null 2>&1; then
    endsub; stop_rec
    echo "TAKE STOPPED: $what did not happen" | tee "$DIR/FAILED" >&2
    exit 1
  fi
}
latest() {  # the newest record by $1 on $2, from the record itself
  [ -x "$R" ] || return 0
  "$R" list --json 2>/dev/null | python3 -c "
import json,sys
try: rs=json.load(sys.stdin)
except Exception: rs=[]
rs=[r for r in rs if r['subject']==sys.argv[1] and sys.argv[2] in (r.get('target') or '')]
print(rs[-1]['id'] if rs else '')" "$1" "$2"
}

# ---------------------------------------------------------------- the take
sub "A fix outlives its reason. You, a script or an agent change something, and a year later nobody knows why it is there." 7
sub "podshl-repairs keeps a record of such changes, and looks at them again after every update. This is a clean Omarchy." 7

# 1. install
subo "One program from the release, checked against its sums."
t "cd \"\$(mktemp -d)\""
t "curl -fsSLO $REL/podshl-repairs-0.1.7-linux-x86_64"
t "curl -fsSLO $REL/SHA256SUMS"
t "sha256sum -c --ignore-missing SHA256SUMS"
subo "Into your own ~/.local/bin. No root, and no package."
t "install -Dm755 podshl-repairs-0.1.7-linux-x86_64 ~/.local/bin/podshl-repairs"
must "the install" test -x "$R"
t "podshl-repairs review"
sub "Nothing recorded yet." 3
subo "A review after every Omarchy update."
t "podshl-repairs install-hook"
must "the update hook" test -f ~/.config/omarchy/hooks/post-update.d/podshl-repairs
sub "" 1.5
subo "And the agent Omarchy is set to use — Claude here — records what it writes, through its own hooks."
t "podshl-repairs install-agent-hook"
must "the agent hook" grep -q agent-hook ~/.claude/settings.json
sub "Even this change to Claude's settings is in the record, with a copy to go back to." 6
t "cd"

# 2. an agent changes a setting
cls
subo "Ask the agent for a change, the way anybody does."
t "claude -p \"Make the gaps between windows smaller: in ~/.config/hypr/looknfeel.lua add an hl.config block with general.gaps_in = 2 and general.gaps_out = 4. Change nothing else and run no commands. Reply only with done.\" --permission-mode acceptEdits < /dev/null"
R1=$(latest claude looknfeel.lua)
must "Claude's change being recorded" test -n "$R1"
subo "Nobody typed anything for the record."
t "podshl-repairs list"
sub "It is there anyway: by claude, with a copy from before the change." 6

# 3. undo it
subo "You do not like it. Undo."
t "podshl-repairs restore $R1"
must "the undo" bash -c "! grep -qE '^[[:space:]]*gaps_in = 2' '$LF'"
sub "The file is back, byte for byte." 4

# 4. a change you keep, overwritten
cls
subo "A change you keep: dim the windows you are not using."
t "claude -p \"Dim unfocused windows: in ~/.config/hypr/looknfeel.lua add an hl.config block with decoration.dim_inactive = true and decoration.dim_strength = 0.15. Change nothing else and run no commands. Reply only with done.\" --permission-mode acceptEdits < /dev/null"
R2=$(latest claude looknfeel.lua)
must "the second change being recorded" test -n "$R2" -a "$R2" != "$R1"
subo "Weeks later Omarchy puts its own default back: an update, a migration, a refresh."
t "omarchy-refresh-config hypr/looknfeel.lua"
must "Omarchy's reset" bash -c "! grep -qE '^[[:space:]]*dim_inactive = true' '$LF'"
sub "Your change is gone, and your rounded corners with it. Nothing tells you." 6
cls
subo "After every update, Omarchy runs the hook."
t "bash ~/.config/omarchy/hooks/post-update.d/podshl-repairs"
must "the notice" bash -c "ls ~/.local/state/omarchy/notifications/*.json | xargs grep -l '\"PODSHL\"'"
echo "$(now) right" >> "$PAN"
sub "This time something does. The notice stays until you look at it." 6
# The notice, top right: ydotool's coordinates are half the screen's pixels.
ydotool mousemove --absolute -x 1800 -y 45; sleep 1.2
ydotool click 0xC0
sleep 1.8
echo "$(now) centre" >> "$PAN"
sub "One click: which change, and why it wants a look." 7
wtype " "; sleep 1.5
subo "Undo goes back to before Claude touched the file, so your rounded corners return. Ask again for the dimming, and it is recorded again."
t "podshl-repairs restore $R2"
must "the second undo" grep -qE "^[[:space:]]*rounding = 8" "$LF"
sub "" 3

# 5. the AUR case
cls
subo "Now PODSHL's own client, built from its PKGBUILD."
t "cd ~/podshl-pkg"
t "curl -fsSLO $RAW/PKGBUILD && curl -fsSLO $RAW/podshl-client.desktop"
t "makepkg -si --noconfirm"
must "the client package" pacman -Q podshl-bin
cls
subo "The AUR step of every omarchy-update:"
t "omarchy-update-aur-pkgs"
sub "It asks the AUR about podshl-bin. It is not there, and the name is free: whoever registers it, the next update installs theirs." 8
sub "The fix is a local repository, a workaround until the package is in the AUR. So it goes in the record, with its reason." 7
t "podshl-repairs begin --kind file --by me --path /etc/pacman.conf --note \"Local repo for podshl-bin until it is in the AUR\""
P=$(latest me /etc/pacman.conf)
must "the record of the workaround" test -n "$P"
t "cat local-repo.sh"
sub "" 4
t "sudo bash local-repo.sh"
t "podshl-repairs done $P"
must "the workaround" bash -c "[ -z \"\$(pacman -Qm)\" ]"
subo "The same step again:"
t "omarchy-update-aur-pkgs"
sub "Nothing foreign left, so nothing is looked up." 5

# 6. a workaround waiting on upstream
cls
subo "An older workaround, for a Hyprland bug."
t "podshl-repairs add --kind package --by me --package hyprland --issue https://github.com/hyprwm/Hyprland/issues/7564 --watch"
t "podshl-repairs review"
must "the closed issue" bash -c "'$R' review --offline >/dev/null; grep -q 'upstream asked about https://github.com/hyprwm/Hyprland/issues/7564: closed' ~/.local/state/podshl/client.log"
sub "Watched, it asks GitHub, and only because you said so. The issue is closed: time to see whether the workaround can go. It removes nothing by itself." 9

# 7. inside a repository
cls
subo "Code in a repository has a history of its own."
t "mkdir ~/code-demo && cd ~/code-demo && git init -q && echo 'x = 0' > settings.py"
t "claude -p \"In settings.py change x = 0 to x = 1. Change nothing else and run no commands. Reply only with done.\" --permission-mode acceptEdits < /dev/null"
t "cat settings.py; podshl-repairs list"
must "Claude's edit in the repository" grep -q "x = 1" ~/code-demo/settings.py
sub "Changed, and not recorded: git already has it. The record is for what has no history." 7
t "cd"

# 8. the record
cls
t "podshl-repairs list"
sub "What was changed on this machine, by whom, and why, looked at again after every update." 7
sub "podshl-repairs · github.com/dx111ge/podshl · docs/REPAIRS.md" 5
endsub

stop_rec
echo "$(now)" > "$DIR/duration"
hyprctl dispatch "hl.dsp.window.close({ window = \"address:$ADDR\" })" >/dev/null
echo "take done: $(du -h "$DIR/screen.mp4" | cut -f1) over $(cat "$DIR/duration")s"

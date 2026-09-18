#!/bin/bash
# Frame capture for the Omarchy screencasts.
#
# `record_videos.mjs` uses WebView2's `Page.startScreencast`, which does not
# exist on WebKitGTK — there is no DevTools protocol to ask. So frames come
# from `grim` on a timer and ffmpeg times them into a constant-rate H.264, and
# the subtitles are burned in from an .srt written while the driving happens
# rather than injected into the page.
#
#   rec.sh start "<x>,<y> <w>x<h>" <fps> <dir>
#   rec.sh stop <dir>
#   rec.sh encode <dir> <fps> <out.mp4> [subs.srt]
set -uo pipefail
export XDG_RUNTIME_DIR=/run/user/1000 WAYLAND_DISPLAY=wayland-1

cmd=${1:?start|stop|encode}
case "$cmd" in
  start)
    geom=${2:?geometry}; fps=${3:-8}; dir=${4:?dir}
    rm -rf "$dir"; mkdir -p "$dir/frames"
    echo "$geom" > "$dir/geom"
    # **Fully detached.** A background subshell that inherits stdout keeps the
    # ssh session open until it exits, so `rec.sh start` over ssh hung until
    # the capture was killed — and the capture went on filling the disk in the
    # meantime. setsid, and every descriptor closed.
    setsid bash -c '
      dir="$1"; geom="$2"; fps="$3"; n=0
      while [ ! -f "$dir/.stop" ]; do
        grim -g "$geom" "$(printf "%s/frames/%06d.png" "$dir" "$n")" 2>/dev/null
        n=$((n+1))
        sleep "$(awk "BEGIN{print 1/$fps}")"
      done
    ' _ "$dir" "$geom" "$fps" </dev/null >/dev/null 2>&1 &
    echo $! > "$dir/.pid"
    echo "capturing $geom at ${fps}fps into $dir (pid $(cat "$dir/.pid"))"
    ;;
  stop)
    dir=${2:?dir}
    touch "$dir/.stop"; sleep 1
    kill "$(cat "$dir/.pid" 2>/dev/null)" 2>/dev/null
    echo "frames: $(ls "$dir/frames" | wc -l)"
    ;;
  encode)
    dir=${2:?dir}; fps=${3:-8}; out=${4:?out}; srt=${5:-}
    # Even dimensions, or H.264 refuses.
    vf="scale=trunc(iw/2)*2:trunc(ih/2)*2"
    [ -n "$srt" ] && [ -f "$srt" ] && vf="$vf,subtitles=$srt:force_style='FontName=JetBrainsMono Nerd Font,FontSize=16,PrimaryColour=&H00FFFFFF,BackColour=&HA0000000,BorderStyle=3,Outline=1,MarginV=24'"
    ffmpeg -y -loglevel error -framerate "$fps" -pattern_type glob -i "$dir/frames/*.png" \
      -vf "$vf" -c:v libx264 -pix_fmt yuv420p -crf 20 -preset medium "$out"
    echo "wrote $out  ($(du -h "$out" | cut -f1))"
    ;;
  *) echo "start|stop|encode" >&2; exit 2;;
esac

#!/bin/bash
# Encode a take's frames with its subtitles burned in.
#   enc.sh <frames-dir> <fps> <srt> <out.mp4>
#
# The style is deliberately small: ffmpeg's subtitles filter scales ASS point
# sizes against a 288-line reference, so FontSize=16 came out ~43px on a 780
# line capture and covered the buttons the subtitle was talking about.
set -euo pipefail
dir=${1:?frames dir}; fps=${2:?fps}; srt=${3:?srt}; out=${4:?out}; pad=${5:-0}

style='FontName=JetBrainsMono Nerd Font,FontSize=9,PrimaryColour=&H00FFFFFF,BackColour=&HC8000000,BorderStyle=3,Outline=0.8,Shadow=0,MarginV=8'

ffmpeg -y -loglevel error -framerate "$fps" -pattern_type glob -i "$dir/frames/*.png" \
  -vf "scale=trunc(iw/2)*2:trunc(ih/2)*2,tpad=stop_mode=clone:stop_duration=${pad},subtitles=${srt}:force_style='${style}'" \
  -c:v libx264 -pix_fmt yuv420p -crf 20 -preset medium "$out"

echo "wrote $out ($(du -h "$out" | cut -f1), $(ffprobe -v error -show_entries format=duration -of csv=p=0 "$out")s)"

#!/bin/bash
# The screen recording of `take-repairs.sh` into a 1920x1080 H.264 with the
# subtitles burned in.
#
# The screen is 3840x1080; the video shows its middle, where the terminal is,
# and pans to the right half while the notice in the top right corner is the
# thing to look at — between the two times `take-repairs.sh` wrote to `pan`.
# The pan takes 0.6 s each way rather than cutting, so the eye can follow it.
#
#   enc-repairs.sh <dir> <out.mp4>
set -euo pipefail
DIR=${1:?take directory}; OUT=${2:?output file}
read -r R _ < <(grep right "$DIR/pan")
read -r C _ < <(grep centre "$DIR/pan")
x="960+960*clip((t-$R)/0.6\,0\,1)-960*clip((t-$C)/0.6\,0\,1)"
style="FontName=JetBrainsMono Nerd Font,FontSize=18,PrimaryColour=&H00FFFFFF,BackColour=&HA0000000,BorderStyle=3,Outline=1,MarginV=28"
ffmpeg -y -loglevel error -i "$DIR/screen.mp4" \
  -vf "crop=1920:1080:$x:0,subtitles=$DIR/take.srt:force_style='$style'" \
  -c:v libx264 -pix_fmt yuv420p -crf 20 -preset medium -an "$OUT"
echo "wrote $OUT ($(du -h "$OUT" | cut -f1))"

#!/bin/bash
# The screen recording of `take-repairs.sh` into a 1920x1080 H.264, with the
# subtitles in a band of their own under the picture.
#
# **Never over the terminal.** The first published cut burned the subtitles
# into the picture, and wherever the terminal's output reached the bottom — the
# diff `omarchy-refresh-config` prints, the review after the hook — the
# subtitle sat on top of the text it was describing. So the picture is the top
# 1920x900 and the subtitles live in the 180 pixels below it, where there is
# never anything else.
#
# The picture is a 2304x1080 window of the 3840x1080 screen scaled to
# 1920x900: the terminal in the middle, and a pan to the right edge while the
# notice in the top right corner is the thing to look at, between the two
# times `take-repairs.sh` wrote to `pan`. The pan takes 0.6 s each way.
#
#   enc-repairs.sh <dir> <out.mp4>
set -euo pipefail
DIR=${1:?take directory}; OUT=${2:?output file}
read -r R _ < <(grep right "$DIR/pan")
read -r C _ < <(grep centre "$DIR/pan")
# Middle of the screen at 768; the right edge at 1536.
x="768+768*clip((t-$R)/0.6\,0\,1)-768*clip((t-$C)/0.6\,0\,1)"
# libass sizes subtitles against a 288-line script, so 11 is about 41 pixels
# on 1080: two lines fit the band with room above and below.
style="FontName=JetBrainsMono Nerd Font,FontSize=11,PrimaryColour=&H00FFFFFF,BorderStyle=1,Outline=0,Shadow=0,MarginV=14,MarginL=20,MarginR=20"
ffmpeg -y -loglevel error -i "$DIR/screen.mp4" \
  -vf "crop=2304:1080:$x:0,scale=1920:900,pad=1920:1080:0:0:color=0x0b0e1a,subtitles=$DIR/take.srt:force_style='$style'" \
  -c:v libx264 -pix_fmt yuv420p -crf 20 -preset medium -an "$OUT"
echo "wrote $OUT ($(du -h "$OUT" | cut -f1))"

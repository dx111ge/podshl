# Recording the client on Omarchy

`scripts/walk/record_videos.mjs` cannot run here. It drives the window over
WebView2's DevTools protocol and captures with `Page.startScreencast`, and on
WebKitGTK there is no DevTools protocol to speak to — its inspector server
binds a port and answers no HTTP. So there is no way to click a selector, read
the DOM, or ask the page for frames.

These three do it the other way round, on the real desktop:

| | |
|---|---|
| `rec.sh` | `grim` on a timer into a frame directory, and `ffmpeg` afterwards. Fully detached, because a background loop that inherits stdout keeps an ssh session open until it exits — and goes on filling the disk while it does |
| `take-user.sh` | the drive: pointer and keyboard, one continuous pass, writing an `.srt` as it goes so the narration is timed to what actually happened |
| `enc.sh` | frames plus subtitles into H.264 |

## What it needs

`ydotoold` running (`systemctl --user start ydotool`), `wtype`, `grim`,
`ffmpeg`. The client floated at a known size — `hyprland.lua` has a rule for
`podshl-client` — and its window position from `hyprctl clients`.

**ydotool's absolute coordinates are not screen pixels.** On a 2560×1440
output they are half of them. Measure it before trusting any coordinate:
`ydotool mousemove --absolute -x 640 -y 360` then `hyprctl cursorpos`.

**Encode at the rate the frames were actually captured, not the rate asked
for.** `grim` takes long enough that 8 fps asked for was 5.2 fps captured;
encoding at 8 squeezed 147 seconds into 100 and the subtitles, which carry real
timestamps, drifted three quarters of a minute late by the end. `take-user.sh`'s
`.srt` ends at the true duration, so the rate is `frames ÷ that`.

## Driving it blind

Every coordinate in `take-user.sh` was read off a screenshot of the real window
during a dry run, one screen at a time. There is no way around that here, and a
layout change means doing it again. Take the screenshot with `grim -g` on the
window's own geometry, not the whole screen, and scale it up with `ffmpeg
-vf scale=iw*8:ih*8:flags=neighbor` when something is too small to place.

Close anything behind the window first. Another window under a floating one
shows through and reads as the client failing to repaint.

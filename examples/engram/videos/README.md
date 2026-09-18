# Screencasts, in English and German

**The user's walk is back** (2026-09-18), recorded on a real Omarchy desktop
against the live operator. It was withdrawn on 2026-09-17 because the client
window showed a program's full path, and on the machine it was recorded on that
path held the account name; the window now shortens paths in the person's own
profile (`%LOCALAPPDATA%\…`, `~/…`), and the new cut shows them shortened. The
**short, vertical cut is still to be recorded.**

Nothing is mocked in either: every number on screen is what the running system
answered.

The **maintainer's cut** was recorded on 2026-09-13 by
[`scripts/walk/record_videos.mjs`](../../../scripts/walk/record_videos.mjs) — the
operator's pages in headless Chrome, and on Windows the shipped client window
over WebView2's DevTools protocol.

The **user's walk** was recorded differently, because that recorder cannot run
on Linux: WebKitGTK has no DevTools protocol to drive, so there is no
`Page.startScreencast` and no way to click a selector. It was driven on the real
desktop by pointer and keyboard, captured frame by frame with `grim` and timed
into H.264 by ffmpeg, with the subtitles written as the driving happened and
burned in afterwards. Every coordinate was read off a screenshot of the real
window first. The client is the one Omarchy's plugin installed — `podshl-bin`
0.1.6 from the package, not a build from the tree — and it answers from
engram's own published files through `sdota.de`.

`podshl-consent.gif` is cut from the English walk (43s–59s, 9 fps, 620px wide,
229 KB) and is what the front page shows: GitHub will not play an `.mp4` from a
repository path in Markdown, so a GIF is the only moving picture that works
there without a third party.

| English | German | Length | For |
|---|---|---|---|
| [`podshl-user.mp4`](podshl-user.mp4) | [`podshl-user-de.mp4`](podshl-user-de.mp4) | 2:36 / 2:55, 960×780 | **The user.** A fix another tool made on this machine, still there and wanting another look; then a question about engram, the classes engram itself publishes, consent item by item, the three things only a person can answer, everything that would leave the machine shown before it leaves, engram's own answer, and the Markdown to take to the project. Subtitles are beside each in [`podshl-user.srt`](podshl-user.srt) and [`podshl-user-de.srt`](podshl-user-de.srt). The German cut also shows the **Original (EN)** chip, which the window offers because engram's labels and answer are machine-translated, and the panel that explains — in German — why the Markdown for the issue is English |
| [`podshl-maintainer.mp4`](podshl-maintainer.mp4) | [`podshl-maintainer-de.mp4`](podshl-maintainer-de.mp4) | 1:15, 1280×800 | **The maintainer.** Why this exists, what to publish, the builder — its example checked by the operator, engram's real files loaded into it — registering, the dashboard of a project with 263 recurring configurations grouped by the file to edit, a fork with the condition to paste, what the mirror could not make of the files, and the public log |

The operator's pages exist in English only; in the German maintainer cut the
subtitles are German and the pages are not.

Re-record with the stack up, engram enrolled and the client or Chrome started
with a debugging port — the script's header lists what each one needs;
`UI_LANG=de` records the German cut. The dashboard token is masked before it is
typed; the log the user loads is
[`../harness/fixtures/ollama-server.log`](../harness/fixtures/ollama-server.log),
synthetic and full of the things the anonymiser has to catch.

# Three screencasts, in English and German

Recorded from the real surfaces on 2026-09-13 by
[`scripts/record_videos.mjs`](../../../scripts/record_videos.mjs): the shipped
client window over WebView2's DevTools protocol, and the operator's pages in
headless Chrome. Nothing is mocked — every number on screen is what the running
system answered, and the engram binary asked for its version is a real build.

| English | German | Length | For |
|---|---|---|---|
| [`podshl-user.mp4`](podshl-user.mp4) | [`podshl-user-de.mp4`](podshl-user-de.mp4) | 1:23 / 1:30, 1280×800 | **The person with the broken machine.** A question about engram, the readings shown and chosen item by item, "where is engram?" answered with a folder and read as `1.2.2`, Ollama's log loaded and kept on the machine, what is sent where and why before anything leaves, the maintainer's own answer with its log entry checked on the device, "did it help?", the report — measured apart from typed — and the free text anonymised before it is offered. The German cut shows the answer translated by the person's own model, with engram's command and terms kept as written |
| [`podshl-maintainer.mp4`](podshl-maintainer.mp4) | [`podshl-maintainer-de.mp4`](podshl-maintainer-de.mp4) | 1:15, 1280×800 | **The maintainer.** Why this exists, what to publish, the builder — its example checked by the operator, engram's real files loaded into it — registering, the dashboard of a project with 263 recurring configurations grouped by the file to edit, a fork with the condition to paste, what the mirror could not make of the files, and the public log |
| [`podshl-short.mp4`](podshl-short.mp4) | [`podshl-short-de.mp4`](podshl-short-de.mp4) | 0:51 / 0:59, 1080×1920 | **Under a minute, vertical.** The user's walk at a quicker beat |

The operator's pages exist in English only; in the German maintainer cut the
subtitles are German and the pages are not.

Re-record with the stack up, engram enrolled and the client or Chrome started
with a debugging port — the script's header lists what each one needs;
`UI_LANG=de` records the German cut. The dashboard token is masked before it is
typed; the log the user loads is
[`../harness/fixtures/ollama-server.log`](../harness/fixtures/ollama-server.log),
synthetic and full of the things the anonymiser has to catch.

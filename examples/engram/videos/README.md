# Screencasts, in English and German

**The user's walk and the short cut are withdrawn for now** (2026-09-17). The
client window showed a program's full path, and on the machine they were
recorded on that path held the account name. The window now shortens paths in
the person's own profile (`%LOCALAPPDATA%\…`, `~/…`), and both are to be
recorded again with it. The maintainer's cut shows only the operator's pages and
stays.

Recorded from the real surfaces on 2026-09-13 by
[`scripts/walk/record_videos.mjs`](../../../scripts/walk/record_videos.mjs): the shipped
client window over WebView2's DevTools protocol, and the operator's pages in
headless Chrome. Nothing is mocked — every number on screen is what the running
system answered, and the engram binary asked for its version is a real build.

| English | German | Length | For |
|---|---|---|---|
| [`podshl-maintainer.mp4`](podshl-maintainer.mp4) | [`podshl-maintainer-de.mp4`](podshl-maintainer-de.mp4) | 1:15, 1280×800 | **The maintainer.** Why this exists, what to publish, the builder — its example checked by the operator, engram's real files loaded into it — registering, the dashboard of a project with 263 recurring configurations grouped by the file to edit, a fork with the condition to paste, what the mirror could not make of the files, and the public log |

The operator's pages exist in English only; in the German maintainer cut the
subtitles are German and the pages are not.

Re-record with the stack up, engram enrolled and the client or Chrome started
with a debugging port — the script's header lists what each one needs;
`UI_LANG=de` records the German cut. The dashboard token is masked before it is
typed; the log the user loads is
[`../harness/fixtures/ollama-server.log`](../harness/fixtures/ollama-server.log),
synthetic and full of the things the anonymiser has to catch.

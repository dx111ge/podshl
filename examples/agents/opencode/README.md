# opencode

opencode does not run a command before a tool. It calls a function inside its
own process, so there is nothing for `agent-hook` to be wired to — and that is
the shape a lot of agents have. `podshl.js` is the joint: a plugin whose two
hooks hand what they were given to `podshl-repairs agent-hook`, the same
program every other agent's hook calls.

    mkdir -p .opencode/plugin
    cp podshl.js .opencode/plugin/

In one project, as above, or in `~/.config/opencode/plugin/` for all of them.
The plugin calls `podshl-repairs` from the path; set `PODSHL_REPAIRS` if it
lives somewhere else.

**It forwards whatever it is handed, as it is handed**, and adds the directory
it is running in. It does not pick out the file or the command, because what
those are called is the thing being measured — a bridge that decided that here
would be a guess wearing a plugin's clothes. So the same two commands work:

    podshl-repairs measure-agent-hook --agent opencode
    # … use opencode as you normally would …
    podshl-repairs measure-agent-hook --write-format
    podshl-repairs install-agent-hook --agent opencode --guessed

## What has been walked, and what has not

Walked on an Omarchy desktop with opencode 1.18.31 against a local Ollama model
on 2026-09-21: the plugin loaded, both hooks fired on a real file write, and
the two calls were kept. The draft in
[`examples/agent-formats.json`](../../agent-formats.json) was read off them —
`tool`, `args.filePath`, `sessionID`, `callID` — and it recorded and restored
that file when the captured call was played back through it.

**Not walked:** a shell command. The local model never ran one, so `command`
and `shell` in that draft are empty, which means **a change opencode makes
through a shell command leaves no record**. That is a gap, and it is written
down here rather than filled in from opencode's documentation. Measure it on
your machine and the draft gets better; send the samples and it gets better for
everybody.

The record from a live opencode edit has not been walked either — the payload
that made one was real, the edit that triggered the recording was replayed.
Until somebody walks that, the format stays a draft and says so in every record
it makes.

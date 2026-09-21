// A bridge from opencode's plugin hooks to the record.
//
// opencode does not run a command before a tool — it calls a function inside
// its own process. So the two ends are joined here: the function is the hook,
// and it hands what it was given to `podshl-repairs agent-hook`, which is the
// same program every other agent's hook calls.
//
// **It forwards whatever it is handed, as it is handed.** It does not pick out
// a file path or a command, because what those are called is the thing being
// measured; a bridge that decided that here would be a guess wearing a
// plugin's clothes. Both arguments are merged into one object and sent whole.
//
// It waits for the record to finish before the tool runs — a copy taken after
// the write is a copy of the wrong thing — and it never lets its own failure
// reach the agent: the record missing a line is a gap, the agent stopped by
// the record is a plugin somebody removes.

import { spawn } from "node:child_process"

const PROGRAM = process.env.PODSHL_REPAIRS || "podshl-repairs"
const AGENT = "opencode"

function tell(event, payload) {
  return new Promise((done) => {
    let child
    try {
      child = spawn(PROGRAM, ["agent-hook", event, "--agent", AGENT], {
        stdio: ["pipe", "ignore", "ignore"],
      })
    } catch {
      return done()
    }
    child.on("error", () => done())
    child.on("close", () => done())
    try {
      child.stdin.write(JSON.stringify(payload))
      child.stdin.end()
    } catch {
      done()
    }
  })
}

// Both arguments, merged, with the ones opencode gives kept under their own
// names. `cwd` is added because a record needs to know where a relative path
// is relative to, and a plugin knows it while the payload might not.
function whole(input, output, directory) {
  const merged = { cwd: directory }
  for (const side of [input, output]) {
    if (side && typeof side === "object") Object.assign(merged, side)
  }
  return merged
}

export const podshl = async ({ directory, worktree }) => {
  const here = directory || worktree || process.cwd()
  return {
    "tool.execute.before": async (input, output) => {
      await tell("pre", whole(input, output, here))
    },
    "tool.execute.after": async (input, output) => {
      await tell("post", whole(input, output, here))
    },
  }
}

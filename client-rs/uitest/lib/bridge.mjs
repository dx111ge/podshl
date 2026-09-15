// Serves the window's own files, and answers its `invoke` from the real binary.
//
// **This is Tauri's half, and nothing else.** In the application, Tauri puts
// `window.__TAURI__` in the page before its script runs and carries every
// `invoke` to Rust. Here a shim does the same and carries it to the same Rust,
// as a subprocess. The page is byte-for-byte what ships; what is replaced is
// the transport, and it is replaced by the real thing rather than by a
// recording.
//
// The Content-Security-Policy is the one from `tauri.conf.json`, on purpose.
// It is what makes `script-src 'self'` real here — an inline shim would be
// blocked, exactly as an inline style was blocked in the application and had
// to be found by looking. So the shim is a file, served from this origin.

import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { execFile } from "node:child_process";
import { join, extname, normalize } from "node:path";
import { createHash } from "node:crypto";

//: The application's own policy, copied rather than relaxed. A test that runs
//: under a weaker policy than the product is testing a different product.
const CSP = "default-src 'self'; style-src 'self' 'unsafe-inline'; script-src 'self'";

/** The same policy, with the page's own inline script allowed by its hash.
 *
 *  **Not `'unsafe-inline'`.** `script-src 'self'` blocks an inline `<script>`,
 *  and the window's whole flow is one — so served plainly, nothing ran at all
 *  and the page sat there with no panels. In the application Tauri rewrites the
 *  policy for the script it is about to serve; serving the page directly means
 *  doing that here, explicitly.
 *
 *  A hash rather than a relaxation, because the property worth keeping is the
 *  one that has already caught something: no *arbitrary* inline script. The
 *  page's own is allowed because it is this exact text and no other, so an
 *  edit to it is an edit to the policy too. */
function cspFor(html) {
  const hashes = [...html.matchAll(/<script>([\s\S]*?)<\/script>/g)]
    .map(m => "'sha256-" + createHash("sha256").update(m[1], "utf8").digest("base64") + "'");
  return CSP.replace("script-src 'self'", `script-src 'self' ${hashes.join(" ")}`);
}

const TYPES = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".svg": "image/svg+xml",
};

//: What Tauri would have put there. `core.invoke` is the only thing the window
//: uses — `index.html` binds it once, in one place, which is the whole reason
//: this is possible at all.
const SHIM = `
window.__TAURI__ = {
  core: {
    invoke: async (cmd, args) => {
      const r = await fetch("/__invoke", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ cmd, args: args || {} }),
      });
      const out = await r.json();
      if (!r.ok) throw new Error(out.error || "invoke failed");
      return out.value;
    },
  },
};
`;

/** Start the bridge. Returns its origin and a way to stop it. */
export async function startBridge({ uiDir, client, port = 0, onInvoke = null } = {}) {
  const calls = [];

  const run = (cmd, args) => new Promise((res, rej) => {
    execFile(client, ["invoke", cmd, JSON.stringify(args)], { timeout: 120000 },
      (err, stdout, stderr) => {
        if (err) return rej(new Error((stderr || err.message).trim()));
        try { res(stdout.trim() ? JSON.parse(stdout) : null); }
        catch (e) { rej(new Error(`${cmd} did not return JSON: ${stdout.slice(0, 200)}`)); }
      });
  });

  const server = createServer(async (req, res) => {
    if (req.method === "POST" && req.url === "/__invoke") {
      let body = "";
      for await (const chunk of req) body += chunk;
      let cmd = "?", args = {};
      try { ({ cmd, args } = JSON.parse(body)); } catch { /* reported below */ }
      calls.push(cmd);
      if (onInvoke) {
        // A test may answer for one command — to make the operator unreachable,
        // for instance. Everything it does not answer goes to the binary.
        const taken = await onInvoke(cmd, args);
        if (taken !== undefined) {
          res.writeHead(taken instanceof Error ? 500 : 200, { "content-type": "application/json" });
          return res.end(JSON.stringify(taken instanceof Error
            ? { error: taken.message } : { value: taken }));
        }
      }
      try {
        const value = await run(cmd, args);
        res.writeHead(200, { "content-type": "application/json" });
        res.end(JSON.stringify({ value }));
      } catch (e) {
        res.writeHead(500, { "content-type": "application/json" });
        res.end(JSON.stringify({ error: e.message }));
      }
      return;
    }

    // The window's own files, and the shim beside them.
    const path = (req.url || "/").split("?")[0];
    if (path === "/__tauri-shim.js") {
      res.writeHead(200, { "content-type": TYPES[".js"], "content-security-policy": CSP });
      return res.end(SHIM);
    }
    const rel = normalize(path === "/" ? "index.html" : path.replace(/^\/+/, ""));
    if (rel.startsWith("..")) { res.writeHead(403); return res.end(); }
    try {
      let body = await readFile(join(uiDir, rel));
      if (rel === "index.html") {
        // Before the page's own script, because the page binds `invoke` at the
        // top of it. Tauri injects its runtime the same way and for the same
        // reason.
        body = Buffer.from(String(body).replace(
          "<script>", '<script src="/__tauri-shim.js"></script>\n<script>'));
      }
      res.writeHead(200, {
        "content-type": TYPES[extname(rel)] || "application/octet-stream",
        "content-security-policy": rel === "index.html" ? cspFor(String(body)) : CSP,
      });
      res.end(body);
    } catch {
      res.writeHead(404);
      res.end();
    }
  });

  await new Promise(r => server.listen(port, "127.0.0.1", r));
  const origin = `http://127.0.0.1:${server.address().port}`;
  return { origin, calls, stop: () => new Promise(r => server.close(r)) };
}

// A headless browser showing the window's page, with the bridge behind it.
//
// System Chrome rather than a downloaded one: there is already a browser on
// every machine this is developed on, and a test harness that fetches 150 MB
// before it can assert anything is a harness people skip.
//
// Node 22 or newer. No dependencies.

import { spawn, execFileSync } from "node:child_process";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { findPage, refuseIfOccupied, attach, waitFor, sleep } from "./cdp.mjs";

const CANDIDATES = process.platform === "win32"
  ? ["C:/Program Files/Google/Chrome/Application/chrome.exe",
     "C:/Program Files (x86)/Google/Chrome/Application/chrome.exe"]
  : ["/usr/bin/google-chrome", "/usr/bin/chromium", "/usr/bin/chromium-browser"];

export function chromePath() {
  const named = process.env.UITEST_CHROME;
  if (named) {
    if (!existsSync(named)) throw new Error(`UITEST_CHROME is ${named}, which does not exist`);
    return named;
  }
  for (const c of CANDIDATES) if (existsSync(c)) return c;
  throw new Error(
    "no Chrome or Chromium found. Set UITEST_CHROME to one. This drives the " +
    "window's page headlessly; there is nothing to assert without a browser.");
}

/** Refuse a client that cannot answer the whole flow.
 *
 *  `perform_reads`, `send_published_report` and `llm_translate` are on the
 *  `invoke` surface only under `--features uitest`, because a released client
 *  must not have them. Driving the flow without them stops at the consent
 *  panel and times out twenty-five seconds later with nothing to read, so it
 *  is said here instead. */
export function refuseWithoutTestSurface(client) {
  let answer;
  try {
    answer = execFileSync(client, ["invoke", "perform_reads", "{}"],
                          { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
  } catch (e) {
    answer = String(e.stdout || "") + String(e.stderr || "");
  }
  if (answer.includes("is not in this build")) {
    throw new Error(
      `${client} was built without the test surface, so the flow cannot be driven ` +
      `past the consent panel.\n` +
      `  Build one with: scripts\\build_client.ps1 <operator> -UiTest   (or build_client.sh --uitest)\n` +
      `  and point PODSHL_CLIENT at it. A released client must not have those commands, ` +
      `which is why this is a separate build rather than a flag at run time.`);
  }
}

/** Open the page in a headless browser and return a driver for it. */
export async function openHeadless({ url, port = 9555 } = {}) {
  await refuseIfOccupied(port);
  const chrome = chromePath();
  const profile = mkdtempSync(join(tmpdir(), "podshl-uitest-chrome-"));

  const child = spawn(chrome, [
    "--headless=new",
    `--remote-debugging-port=${port}`,
    `--user-data-dir=${profile}`,
    "--no-first-run", "--no-default-browser-check",
    // Nothing here should reach the network except through the bridge, and the
    // bridge is loopback. These keep a browser from being a second actor.
    "--disable-extensions", "--disable-background-networking",
    "--disable-component-update", "--no-sandbox",
    url,
  ], { stdio: "ignore" });

  const page = await findPage(port, { timeoutMs: 25000 });
  if (!page) {
    child.kill();
    rmSync(profile, { recursive: true, force: true });
    throw new Error(`the browser never opened a debugging port on ${port}`);
  }

  const { evaluate, close: detach } = await attach(page);

  const panels = () => evaluate(
    `[...document.querySelectorAll(".panel")].map(p => (p.querySelector("h3")||p).textContent.trim())`);

  const context = async () => `  panels on screen: ${JSON.stringify(await panels())}`;

  return {
    evaluate,
    panels,
    waitFor: (expr, what, ms = 25000) => waitFor(evaluate, expr, what, { ms, context }),
    async close() {
      detach();
      child.kill();
      await sleep(200);
      rmSync(profile, { recursive: true, force: true });
    },
  };
}

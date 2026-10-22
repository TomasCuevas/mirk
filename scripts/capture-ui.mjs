#!/usr/bin/env node
/**
 * Capture mirk's interface so an agent can look at it, instead of a person taking screenshots.
 *
 * The desktop window's size is the reader's stored layout choice (Ajustes → Ventana, S2 of
 * `odd/tasks/window-layout.md`): two, three or four documents per row, or full screen. Every capture is
 * taken at the size that layout names, at device scale factor 2, because judging a four-per-row grid at
 * the two-per-row window would invent defects nobody sees.
 *
 * The app renders honestly in a plain browser — it says nothing is connected — but with no data it
 * only shows the empty state, which is not the product. So this script injects a fake
 * `window.__TAURI_INTERNALS__` before the app boots. `web/src/lib/tauri.ts` forwards every call to
 * that object and is left untouched: the seed lives here, not in the application.
 *
 * Usage:
 *   node scripts/capture-ui.mjs                 # start the dev server if needed, capture, exit
 *   node scripts/capture-ui.mjs --out /tmp/ux  # choose the output directory
 *   node scripts/capture-ui.mjs --port 5173    # another dev server port
 *   node scripts/capture-ui.mjs --layout columns-4
 *                                              # the size that layout names, and its grid
 *
 * Exit code is non-zero when the page threw: a screenshot of a crashed page is not evidence.
 */
import { spawn } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";

// --------------------------------------------------------------------------- configuration

const argv = process.argv.slice(2);
const flag = (name) => argv.includes(name);
const value = (name, fallback) => {
  const at = argv.indexOf(name);
  return at >= 0 && argv[at + 1] ? argv[at + 1] : fallback;
};

const PORT = Number(value("--port", "1420"));
const URL_UNDER_TEST = `http://localhost:${PORT}/`;
const CHROME = value("--chrome", process.env.CHROME_PATH ?? "google-chrome");
const stamp = new Date().toISOString().replace(/[:.]/g, "-").slice(0, 19);
const OUT = value("--out", join(".impeccable", "screenshots", stamp));

/**
 * The size each layout pins, mirroring `crates/mirk-desktop/src/layout.rs` (S2).
 *
 * The desktop process is the authority; these numbers are what a browser can reproduce without it. Full
 * screen has no window of its own, so the capture uses a common laptop screen and lets `auto-fill` decide
 * the columns.
 */
const LAYOUTS = {
  "columns-2": { width: 430, height: 844 },
  "columns-3": { width: 634, height: 844 },
  "columns-4": { width: 838, height: 844 },
  auto: { width: 1440, height: 900 },
};

const LAYOUT = value("--layout", "columns-2");
if (!(LAYOUT in LAYOUTS)) {
  console.error(`--layout must be one of ${Object.keys(LAYOUTS).join(", ")}, not "${LAYOUT}"`);
  process.exit(2);
}

/** The window being captured: the chosen layout's size at device scale factor 2. */
const WINDOW = { ...LAYOUTS[LAYOUT], deviceScaleFactor: 2 };

/** One entry per screen the navbar can reach, in the order a reader meets them. */
const SCREENS = [
  { name: "home", label: "Home" },
  { name: "library", label: "Library" },
  { name: "settings", label: "Settings" },
];

// --------------------------------------------------------------------------- the seed

/**
 * A book-shaped cover, so a card is judged against real cover art rather than a void.
 *
 * It is an inline SVG data URL: no network, no fixture files, and every book looks like a book
 * instead of the diagonal placeholder the app draws when it has no image.
 */
function cover(title, hue) {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="400" height="600">
  <defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1">
    <stop offset="0" stop-color="hsl(${hue},32%,22%)"/><stop offset="1" stop-color="hsl(${hue},26%,11%)"/>
  </linearGradient></defs>
  <rect width="400" height="600" fill="url(#g)"/>
  <rect x="22" y="22" width="356" height="556" fill="none" stroke="hsl(${hue},40%,60%)" stroke-opacity=".35"/>
  <text x="200" y="290" font-family="Georgia,serif" font-size="32" fill="#f4f4f6" text-anchor="middle">${title}</text>
</svg>`;
  return `data:image/svg+xml;utf8,${encodeURIComponent(svg)}`;
}

/**
 * Four documents, chosen to cover the states a card can be in rather than to look tidy.
 *
 * One read through an external provider, one set by hand, one nearly finished and one just started:
 * that exercises the progress ramp (green at 70 %, amber below), the provenance sentence, and the
 * page badge without needing the Rust side.
 */
const BOOKS = [
  { id: 1, title: "The Pale Blue Dot", authors: ["Carl Sagan"], pages: 432, fraction: 0.72, hue: 210 },
  { id: 2, title: "Thinking in Systems", authors: ["Donella Meadows"], pages: 240, fraction: 0.41, hue: 150 },
  { id: 3, title: "The Timeless Way of Building", authors: ["Christopher Alexander"], pages: 576, fraction: 0.93, hue: 20 },
  { id: 4, title: "Anatomy of a Story", authors: ["John Truby"], pages: 448, fraction: 0.18, hue: 280 },
];

const now = Math.floor(Date.now() / 1000);
const DAY = 86400;

const entries = BOOKS.map(({ id, title, authors, pages, fraction, hue }, index) => ({
  id,
  path: `/home/tomas-cuevas/Books/${title.toLowerCase().replace(/\W+/g, "-")}.pdf`,
  title,
  userTitle: null,
  authors,
  readerAuthors: [],
  pages,
  progress: {
    page: Math.round(pages * fraction),
    total: pages,
    source: index === 0 ? "evince-gvfs" : "manual",
    confidence: index === 0 ? "close-time" : "manual",
    recordedAt: now - index * 3600,
    readAt: now - index * DAY,
  },
  coverPage: null,
  coverImage: null,
  coverSource: null,
}));

const covers = Object.fromEntries(BOOKS.map(({ id, title, hue }) => [id, cover(title, hue)]));

/**
 * The object `@tauri-apps/api` calls into, defined before the app boots.
 *
 * `invoke` is the only method the app actually reaches at startup, but `transformCallback` and
 * `unregisterCallback` are what `listen()` touches, and a missing one turns the first event
 * subscription into a thrown exception — which this script treats as a failed capture.
 */
const SEED = `
(() => {
  const covers = ${JSON.stringify(covers)};
  const answers = {
    "get_language": "en",
    "get_window_layout": ${JSON.stringify(LAYOUT)},
    "list_library": ${JSON.stringify(entries)},
    "progress_providers": [
      { id: "native", capability: "position", availability: "available" },
      { id: "evince-gvfs", capability: "position", availability: "unavailable" }
    ],
    "watched_folders": [{ id: 1, path: "/home/tomas-cuevas/Books", documents: ${entries.length} }],
    "list_suggestions": [],
    "plugin:event|listen": 1,
    "plugin:event|unlisten": null,
  };
  window.__TAURI_INTERNALS__ = {
    invoke: (command, args) => {
      if (command === "document_cover") return Promise.resolve(covers[args && args.id] ?? null);
      if (command in answers) return Promise.resolve(answers[command]);
      return Promise.reject(new Error("capture-ui: unmocked command " + command));
    },
    transformCallback: (callback) => {
      const id = Math.floor(Math.random() * 1e9);
      window["_" + id] = callback;
      return id;
    },
    unregisterCallback: () => {},
    convertFileSrc: (path) => path,
  };
})();
`;

// --------------------------------------------------------------------------- CDP

/** A minimal Chrome DevTools Protocol client. Node 22 ships the WebSocket it needs. */
class Cdp {
  #socket;
  #sequence = 0;
  #pending = new Map();
  #waiters = new Map();
  /** Exceptions and console errors seen while the page was alive. */
  problems = [];

  constructor(socket) {
    this.#socket = socket;
    socket.onmessage = ({ data }) => {
      const message = JSON.parse(data);
      if (message.id !== undefined) {
        const slot = this.#pending.get(message.id);
        if (!slot) return;
        this.#pending.delete(message.id);
        if (message.error) slot.reject(new Error(message.error.message));
        else slot.resolve(message.result);
        return;
      }
      if (message.method === "Runtime.exceptionThrown") {
        const detail = message.params.exceptionDetails;
        this.problems.push(detail.exception?.description ?? detail.text ?? "exception");
      }
      if (message.method === "Runtime.consoleAPICalled" && message.params.type === "error") {
        this.problems.push(
          "console.error: " + message.params.args.map((a) => a.value ?? a.description ?? "?").join(" "),
        );
      }
      const waiters = this.#waiters.get(message.method) ?? [];
      this.#waiters.delete(message.method);
      waiters.forEach((resolve) => resolve(message.params));
    };
  }

  static async connect(url) {
    const socket = new WebSocket(url);
    await new Promise((resolve, reject) => {
      socket.onopen = resolve;
      socket.onerror = () => reject(new Error("could not open the DevTools socket"));
    });
    return new Cdp(socket);
  }

  send(method, params = {}) {
    const id = ++this.#sequence;
    this.#socket.send(JSON.stringify({ id, method, params }));
    return new Promise((resolve, reject) => this.#pending.set(id, { resolve, reject }));
  }

  once(method) {
    return new Promise((resolve) =>
      this.#waiters.set(method, [...(this.#waiters.get(method) ?? []), resolve]),
    );
  }

  /** Evaluates `expression` and returns its value, so callers read the page as a person would. */
  async evaluate(expression) {
    const { result } = await this.send("Runtime.evaluate", {
      expression,
      returnByValue: true,
      awaitPromise: true,
    });
    return result.value;
  }

  /** Polls `expression` until it is truthy. A fixed sleep is a guess; this is the condition. */
  async until(expression, { timeoutMs = 10_000, label = expression } = {}) {
    const deadline = Date.now() + timeoutMs;
    while (Date.now() < deadline) {
      if (await this.evaluate(expression)) return;
      await sleep(150);
    }
    throw new Error(`timed out waiting for: ${label}`);
  }

  close() {
    this.#socket.close();
  }
}

// --------------------------------------------------------------------------- process handling

/** Whether something already answers on the port — very often the dev server a person left open. */
async function listening() {
  try {
    const response = await fetch(URL_UNDER_TEST, { signal: AbortSignal.timeout(1500) });
    return response.ok;
  } catch {
    return false;
  }
}

/** Holds the two child processes this script owns, so both are gone when it exits. */
const children = [];

/**
 * Kills each child's whole process group, not just the child.
 *
 * `bun run dev` starts `vite` as its own child, so a plain `child.kill()` leaves the dev server
 * listening and the next capture silently talks to a stale build. Spawning detached makes the two a
 * group, and a negative pid signals the group.
 */
const killChildren = () => {
  for (const child of children) {
    if (!child.pid) continue;
    try {
      process.kill(-child.pid, "SIGKILL");
    } catch {
      try {
        child.kill("SIGKILL");
      } catch {}
    }
  }
  children.length = 0;
};
process.on("exit", killChildren);
process.on("SIGINT", () => {
  killChildren();
  process.exit(130);
});

async function ensureDevServer() {
  if (await listening()) {
    console.log(`dev server: already up on ${URL_UNDER_TEST}`);
    return;
  }
  console.log(`dev server: starting (bun run dev)…`);
  const child = spawn("bun", ["run", "dev"], { stdio: "ignore", detached: true });
  children.push(child);
  for (let attempt = 0; attempt < 120; attempt++) {
    if (await listening()) return;
    if (child.exitCode !== null) throw new Error("the dev server exited before serving");
    await sleep(500);
  }
  throw new Error(`the dev server never answered on ${URL_UNDER_TEST}`);
}

async function launchChrome() {
  const profile = join(tmpdir(), `mirk-capture-${process.pid}`);
  rmSync(profile, { recursive: true, force: true });
  mkdirSync(profile, { recursive: true });
  const child = spawn(
    CHROME,
    [
      "--headless=new",
      "--remote-debugging-port=0", // the browser picks a free port and writes it to the profile
      `--user-data-dir=${profile}`,
      "--no-first-run",
      "--no-default-browser-check",
      "--disable-gpu",
      "--no-sandbox",
      "--hide-scrollbars",
      "--force-device-scale-factor=2",
      `--window-size=${WINDOW.width},${WINDOW.height}`,
      "about:blank",
    ],
    { stdio: "ignore", detached: true },
  );
  children.push(child);

  const portFile = join(profile, "DevToolsActivePort");
  for (let attempt = 0; attempt < 120; attempt++) {
    if (existsSync(portFile)) {
      const port = readFileSync(portFile, "utf8").split("\n")[0].trim();
      if (port) {
        const response = await fetch(`http://127.0.0.1:${port}/json/list`);
        const targets = await response.json();
        const page = targets.find((t) => t.type === "page");
        if (page) return { child, profile, port, page };
      }
    }
    if (child.exitCode !== null) throw new Error(`${CHROME} exited during startup`);
    await sleep(250);
  }
  throw new Error(`${CHROME} never exposed a DevTools page target`);
}

// --------------------------------------------------------------------------- the capture

/**
 * Captures the visible window, then one frame per scroll of `main`.
 *
 * `main` is the scroll container (`.app` is a fixed-height column and only the screen between the
 * top bar and the navbar scrolls), so a document-level "full page" capture silently returns one
 * viewport. Walking the container's own scroll offsets is what actually shows the rest.
 */
async function captureScreen(cdp, name, offsetHeight) {
  const files = [];
  const shoot = async (suffix) => {
    const { data } = await cdp.send("Page.captureScreenshot", { format: "png" });
    const file = join(OUT, `${name}${suffix}.png`);
    writeFileSync(file, Buffer.from(data, "base64"));
    return file;
  };

  files.push(await shoot(""));

  const geometry = await cdp.evaluate(`(() => {
    const main = document.querySelector("main");
    return main ? { scrollHeight: main.scrollHeight, clientHeight: main.clientHeight } : null;
  })()`);
  if (!geometry) return files;

  const scrollable = geometry.scrollHeight - geometry.clientHeight;
  if (scrollable <= 4) return files;

  const frames = Math.ceil(scrollable / offsetHeight);
  for (let frame = 1; frame <= frames; frame++) {
    const offset = Math.min(frame * offsetHeight, scrollable);
    await cdp.evaluate(`document.querySelector("main").scrollTop = ${offset}`);
    await sleep(250);
    files.push(await shoot(`-scroll${frame}`));
  }
  await cdp.evaluate(`document.querySelector("main").scrollTop = 0`);
  return files;
}

/** Clicks the navbar item for `label`, the way a reader changes screens. */
async function goTo(cdp, label) {
  const clicked = await cdp.evaluate(`(() => {
    const item = [...document.querySelectorAll("nav button, nav a")]
      .find((el) => el.textContent.trim().toLowerCase() === ${JSON.stringify(label.toLowerCase())});
    if (!item) return false;
    item.click();
    return true;
  })()`);
  if (!clicked) throw new Error(`no navbar item labelled "${label}"`);
  await sleep(600);
}

async function main() {
  mkdirSync(OUT, { recursive: true });
  await ensureDevServer();
  const { page } = await launchChrome();
  const cdp = await Cdp.connect(page.webSocketDebuggerUrl);

  await cdp.send("Page.enable");
  await cdp.send("Runtime.enable");
  await cdp.send("Emulation.setDeviceMetricsOverride", {
    width: WINDOW.width,
    height: WINDOW.height,
    deviceScaleFactor: WINDOW.deviceScaleFactor,
    mobile: false,
  });
  await cdp.send("Page.addScriptToEvaluateOnNewDocument", { source: SEED });

  const loaded = cdp.once("Page.loadEventFired");
  await cdp.send("Page.navigate", { url: URL_UNDER_TEST });
  await loaded;
  await cdp.until(`document.querySelectorAll("article").length > 0`, {
    label: "the seeded library to render a card",
  });
  await sleep(800); // fonts and cover images settle after the first paint

  const manifest = {
    capturedAt: new Date().toISOString(),
    url: URL_UNDER_TEST,
    layout: LAYOUT,
    window: WINDOW,
    screens: {},
    problems: cdp.problems,
  };

  for (const screen of SCREENS) {
    if (screen.name !== "home") await goTo(cdp, screen.label);
    const files = await captureScreen(cdp, screen.name, WINDOW.height);
    manifest.screens[screen.name] = files.map((f) => f.slice(OUT.length + 1));
    console.log(`  ${screen.name}: ${files.length} capture(s)`);
  }

  // S5: the window layout's own sub-screen. Settings sub-screens are addressable by hash (R43/S2), so
  // this is the same view a reload at `#/settings/window` lands on, not a click path the capture invents.
  await cdp.evaluate(`window.location.hash = "#/settings/window"`);
  await sleep(600);
  const windowScreen = await captureScreen(cdp, "settings-window", WINDOW.height);
  manifest.screens["settings-window"] = windowScreen.map((f) => f.slice(OUT.length + 1));
  console.log(`  settings-window: ${windowScreen.length} capture(s)`);

  await goTo(cdp, "Home"); // leave the app where a reader starts

  manifest.problems = cdp.problems;
  writeFileSync(join(OUT, "manifest.json"), JSON.stringify(manifest, null, 2));
  cdp.close();

  console.log(`\ncaptures: ${OUT}`);
  if (manifest.problems.length > 0) {
    console.error(`\n${manifest.problems.length} page problem(s):`);
    for (const problem of manifest.problems) console.error(`  ! ${String(problem).split("\n")[0]}`);
    process.exitCode = 1;
    return;
  }
  console.log("no exceptions or console errors");
}

await main();
killChildren();

if (!flag("--keep")) process.exit(process.exitCode ?? 0);

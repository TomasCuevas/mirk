// r12 spike runner.
// Sequence:  1) start the localhost server + collector
//            2) for each engine run, SEQUENTIALLY (never concurrently):
//                  reset stats -> launch real windowed browser -> wait for the
//                  page's self-measured POST -> kill the browser -> snapshot stats
//            3) stop the server, write results.json, print a compact summary
//
// Usage:
//   node runner.mjs                      # primary PDF, WebKitGTK then Chrome
//   node runner.mjs --secondary          # also run the 61 MB PDF (extra launches)
//   node runner.mjs --only=webkitgtk     # single engine
//   node runner.mjs --retry-legacy       # retry WebKitGTK with the legacy pdf.js build
//   node runner.mjs --smoke              # harness self-test: 2 pages, no sequential phase
//   node runner.mjs --server-only        # just run the collector (manual curl debugging)
//   node runner.mjs --no-server          # use an already-running server
import { spawn } from "node:child_process";
import fs from "node:fs";
import fsp from "node:fs/promises";
import path from "node:path";
import url from "node:url";

const HERE = path.dirname(url.fileURLToPath(import.meta.url));
const LOGS = path.join(HERE, "logs");
const PORT = Number(process.env.PORT || 8123);
const ORIGIN = `http://127.0.0.1:${PORT}`;
const RAW_RESULTS = path.join(HERE, "results.raw.jsonl");
const RESULTS_JSON = path.join(HERE, "results.json");

const MINIBROWSER = "/usr/lib/x86_64-linux-gnu/webkit2gtk-4.1/MiniBrowser";
const CHROME = "/usr/bin/google-chrome";

const PRIMARY_PDF = "learning-patterns.pdf";
const SECONDARY_PDF = "cracking-the-coding-interview.pdf";
const PRIMARY_PDF_REAL = "/home/tomas-cuevas/Documents/Mega/Libros/Programación/Learning Patterns - Lydia Hallie and Addy Osmani.pdf";
const SECONDARY_PDF_REAL = "/home/tomas-cuevas/Documents/Mega/Libros/Programación/Cracking the Coding Interview.pdf";

const TIMEOUT_MS = Number(process.env.RUN_TIMEOUT_MS || 120_000); // hard bound per run
const SEQ = Number(process.env.SEQ_PAGES || 20);
const PROBE = Number(process.env.PROBE_PAGES ?? 5);

const args = process.argv.slice(2);
const flag = (n) => args.includes(`--${n}`);
const opt = (n, d) => {
  const a = args.find((x) => x.startsWith(`--${n}=`));
  return a ? a.slice(n.length + 3) : d;
};

const SMOKE = flag("smoke");
const ONLY = opt("only", null);
const SECONDARY = flag("secondary");
const SKIP_PRIMARY = flag("skip-primary");
const RETRY_LEGACY = flag("retry-legacy");
const NO_SERVER = flag("no-server");
const SERVER_ONLY = flag("server-only");
const REPORT_ONLY = flag("report-only");

fs.mkdirSync(LOGS, { recursive: true });

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function httpJson(p, init) {
  const r = await fetch(ORIGIN + p, init);
  if (!r.ok) throw new Error(`GET ${p} -> ${r.status}`);
  return r.json();
}

// ---------------------------------------------------------------- server
let serverProc = null;
async function startServer() {
  if (NO_SERVER) return;
  serverProc = spawn(process.execPath, [path.join(HERE, "server.mjs")], {
    cwd: HERE,
    stdio: ["ignore", "pipe", "pipe"],
    env: { ...process.env, PORT: String(PORT) },
  });
  serverProc.stdout.on("data", (d) => process.stdout.write(String(d)));
  serverProc.stderr.on("data", (d) => process.stderr.write(`[server:err] ${d}`));
  for (let i = 0; i < 100; i++) {
    try {
      await httpJson("/health");
      return;
    } catch {
      await sleep(100);
    }
  }
  throw new Error("server did not come up");
}
function stopServer() {
  if (!serverProc) return;
  serverProc.kill("SIGTERM");
  serverProc = null;
}

// ------------------------------------------------------------- result wait
function entryFor(raw, runId) {
  return raw.find((e) => e.run === runId);
}
async function waitForResult(runId, proc, deadline) {
  while (Date.now() < deadline) {
    if (proc && proc.exitCode !== null) {
      // browser exited on its own; give the collector a moment, then stop
      await sleep(1500);
      break;
    }
    let raw = "";
    try {
      raw = await fsp.readFile(RAW_RESULTS, "utf8");
    } catch {}
    const hit = entryFor(
      raw
        .split("\n")
        .filter(Boolean)
        .map((l) => {
          try {
            return JSON.parse(l);
          } catch {
            return null;
          }
        })
        .filter(Boolean),
      runId,
    );
    if (hit) return hit;
    await sleep(400);
  }
  return null;
}

// ------------------------------------------------------------------ launch
function launchSpec(which, pageUrl, runId) {
  if (which === "webkitgtk") {
    const dataDir = path.join(LOGS, `xdg-${runId}`);
    return {
      cmd: MINIBROWSER,
      args: [pageUrl],
      env: {
        ...process.env,
        DISPLAY: process.env.DISPLAY || ":0",
        XDG_DATA_HOME: path.join(dataDir, "data"),
        XDG_CACHE_HOME: path.join(dataDir, "cache"),
      },
      display: "windowed (MiniBrowser, GTK3; same WebKitGTK 4.1 engine Tauri uses)",
    };
  }
  if (which === "chrome") {
    const profile = path.join(LOGS, `chrome-profile-${runId}`);
    return {
      cmd: CHROME,
      args: [
        `--user-data-dir=${profile}`,
        "--no-first-run",
        "--no-default-browser-check",
        "--window-size=1280,900",
        "--window-position=20,20",
        pageUrl,
      ],
      env: { ...process.env, DISPLAY: process.env.DISPLAY || ":0" },
      display: "windowed (real Chrome, no --headless)",
    };
  }
  throw new Error(`unknown engine ${which}`);
}

async function runOne(plan) {
  const pageUrl =
    `${ORIGIN}/index.html?engine=${encodeURIComponent(plan.which)}` +
    `&run=${encodeURIComponent(plan.runId)}` +
    `&pdf=${encodeURIComponent(plan.pdf)}` +
    `&build=${plan.build}` +
    (SMOKE ? "&smoke=1" : `&seq=${SEQ}&probe=${PROBE}`);

  await httpJson("/reset-stats");
  const spec = launchSpec(plan.which, pageUrl, plan.runId);
  const logFile = path.join(LOGS, `${plan.runId}.log`);
  const out = fs.createWriteStream(logFile);

  const startedAt = new Date().toISOString();
  const t0 = Date.now();
  const proc = spawn(spec.cmd, spec.args, {
    detached: true, // own process group so we can kill the whole tree
    stdio: ["ignore", "pipe", "pipe"],
    env: spec.env,
  });
  proc.stdout.pipe(out);
  proc.stderr.pipe(out);

  const result = await waitForResult(plan.runId, proc, Date.now() + TIMEOUT_MS);
  const elapsedMs = Date.now() - t0;

  // kill the whole process group; a browser must never outlive its run
  let killed = false;
  try {
    process.kill(-proc.pid, "SIGKILL");
    killed = true;
  } catch {
    try {
      proc.kill("SIGKILL");
      killed = true;
    } catch {}
  }
  await sleep(1500);
  try {
    proc.stdout.unpipe(out);
    out.end();
  } catch {}

  let serverStats = null;
  try {
    serverStats = await httpJson("/stats");
  } catch {}

  const outcome = result ? result.status : "timeout-no-result";
  const entry = {
    runId: plan.runId,
    engine: plan.which,
    pdf: plan.pdf,
    build: plan.build,
    launch: { cmd: spec.cmd, args: spec.args, display: spec.display, env: spec.env },
    startedAt,
    runnerElapsedMs: elapsedMs,
    timeoutMs: TIMEOUT_MS,
    outcome,
    killedProcessGroup: killed,
    logFile: path.relative(HERE, logFile),
    serverStats,
    result,
  };
  if (!result) {
    entry.failure = {
      reason: `no result POSTed within ${TIMEOUT_MS / 1000}s (page hung, worker failed, or engine never rendered)`,
      runnerElapsedMs: elapsedMs,
    };
  }
  return entry;
}

// ------------------------------------------------------------------- main

// Latest archived run entry for an engine/pdf/build that succeeded.
async function latestArchived(which, pdf, build) {
  let names = [];
  try {
    names = (await fsp.readdir(LOGS)).filter((n) => n.startsWith("run-") && n.endsWith(".json"));
  } catch {
    return null;
  }
  const entries = [];
  for (const n of names) {
    try {
      const e = JSON.parse(await fsp.readFile(path.join(LOGS, n), "utf8"));
      if (
        e.engine === which &&
        e.pdf === pdf &&
        e.build === build &&
        e.result &&
        e.result.status === "ok"
      ) {
        entries.push(e);
      }
    } catch {}
  }
  if (!entries.length) return null;
  entries.sort((a, b) => String(a.startedAt).localeCompare(String(b.startedAt)));
  return entries[entries.length - 1];
}
// Every archived run entry, deduplicated per engine+pdf+build (newest wins).
async function collectArchived() {
  let names = [];
  try {
    names = (await fsp.readdir(LOGS)).filter((n) => n.startsWith("run-") && n.endsWith(".json"));
  } catch {
    return [];
  }
  const byKey = new Map();
  for (const n of names) {
    try {
      const e = JSON.parse(await fsp.readFile(path.join(LOGS, n), "utf8"));
      if (!e.engine || !e.pdf) continue;
      const key = `${e.engine}|${e.pdf}|${e.build}`;
      const prev = byKey.get(key);
      if (!prev || String(e.startedAt) > String(prev.startedAt)) byKey.set(key, e);
    } catch {}
  }
  return [...byKey.values()];
}

async function main() {
  if (SERVER_ONLY) {    await startServer();
    process.stdout.write("server running; Ctrl-C to stop\n");
    return;
  }

  if (!REPORT_ONLY) await startServer();

  const plan = [];
  const whichList = ONLY ? [ONLY] : ["webkitgtk", "chrome"];
  const stamp = new Date().toISOString().replace(/[:.]/g, "-");
  if (!SKIP_PRIMARY) {
    for (const which of whichList) {
      plan.push({
        which,
        runId: `${which}-${SMOKE ? "smoke" : "primary"}-${stamp}`,
        pdf: PRIMARY_PDF,
        build: "modern",
      });
    }
  }
  if (RETRY_LEGACY) {
    plan.push({
      which: "webkitgtk",
      runId: `webkitgtk-primary-legacy-${stamp}`,
      pdf: PRIMARY_PDF,
      build: "legacy",
    });
  }
  if (SECONDARY) {
    for (const which of whichList) {
      plan.push({
        which,
        runId: `${which}-secondary-${stamp}`,
        pdf: SECONDARY_PDF,
        build: "modern",
      });
    }
  }

  const runs = [];
  if (REPORT_ONLY) {
    runs.push(...(await collectArchived()));
    process.stdout.write(`report-only: rebuilt from ${runs.length} archived run(s), no browser launched\n`);
  }
  for (const p of REPORT_ONLY ? [] : plan) {
    process.stdout.write(`\n=== RUN ${p.runId} (${p.which}, ${p.pdf}, ${p.build}) ===\n`);
    const entry = await runOne(p);
    runs.push(entry);
    // archive the full entry (including server stats) so results.json can be
    // rebuilt across invocations without re-launching a browser
    await fsp.writeFile(path.join(LOGS, `run-${p.runId}.json`), JSON.stringify(entry, null, 2));
    const r = entry.result;
    process.stdout.write(
      r
        ? `outcome=${entry.outcome} firstPage=${r.firstPage?.navigationToFirstVisibleMs ?? "-"}ms seqMean=${r.sequential?.meanTotalMs ?? "-"}ms errors=${r.errorCount ?? "-"}\n`
        : `outcome=${entry.outcome} (${entry.failure?.reason})\n`,
    );
    // sequential, never concurrent: wait for the machine to settle
    await sleep(4000);
  }

  // Reuse earlier good runs of the same engine/pdf/build from the per-run
  // archive instead of spending another browser launch (the launch budget is
  // deliberately small).
  const reused = [];
  for (const which of ["webkitgtk", "chrome"]) {
    const already = runs.some(
      (r) => r.engine === which && r.pdf === PRIMARY_PDF && r.build === "modern",
    );
    if (already) continue;
    const archived = await latestArchived(which, PRIMARY_PDF, "modern");
    if (archived) {
      archived.sourcedFrom = "logs/run-*.json (earlier launch in this spike; not re-launched)";
      runs.push(archived);
      reused.push(which);
    }
  }
  if (reused.length) {
    process.stdout.write(`reused archived runs for: ${reused.join(", ")} (no new browser launch)\n`);
  }

  stopServer();
  await sleep(500);

  // ------------------------------------------------------------- results.json
  const primaryInfo =
    runs.find((r) => r.pdf === PRIMARY_PDF && r.build === "modern" && r.engine === "webkitgtk") ??
    runs.find((r) => r.pdf === PRIMARY_PDF && r.build === "modern");
  const secondaryInfo = runs.find((r) => r.pdf === SECONDARY_PDF);
  const out = {
    spike: "r12",
    question:
      "Is pdf.js rendering performance on WebKitGTK 2.52.6 good enough to be the rendering foundation of a Tauri PDF reader on Linux? (Chrome 155 as reference, not decision-maker)",
    generatedAt: new Date().toISOString(),
    machine: {
      os: "Ubuntu 24.04.5 LTS, GNOME 46, Wayland session with XWayland",
      cpuCores: 12,
      memoryNote: "14 GB total",
      session: { DISPLAY: process.env.DISPLAY, WAYLAND_DISPLAY: process.env.WAYLAND_DISPLAY, XDG_SESSION_TYPE: process.env.XDG_SESSION_TYPE },
    },
    engines: {
      webkitgtk: { engine: "WebKitGTK 4.1 (libwebkit2gtk-4.1-0) 2.52.6-0ubuntu0.24.04.1", harness: MINIBROWSER },
      chrome: { engine: "Google Chrome 155.0.8059.39", harness: CHROME },
    },
    primaryPdf: {
      servedAs: PRIMARY_PDF,
      realPath: PRIMARY_PDF_REAL,
      pages: primaryInfo?.result?.numPages ?? null,
      bytes: 142801554,
      note: "symlinked into public/pdf, never copied; range-served over http://127.0.0.1",
    },
    secondaryPdf: { servedAs: SECONDARY_PDF, realPath: SECONDARY_PDF_REAL, pages: secondaryInfo?.result?.numPages ?? null, bytes: 64112405 },
    pdfjs: {
      version: primaryInfo?.result?.pdfjsVersion ?? null,
      dist: "npm pdfjs-dist",
      vendoredAt: "vendor/pdfjs-dist",
      builds: { modern: "build/pdf.mjs", legacy: "legacy/build/pdf.mjs" },
    },
    method: {
      transport: "HTTP on 127.0.0.1, never file://",
      selfMeasuringPage: "public/index.html + public/measure.js POST /result",
      sequential: "engines run one at a time, browser killed after every run",
      pdfDelivery:
        "the page fetches the whole PDF into an ArrayBuffer and passes it to getDocument({data}), so render cost is isolated from network/streaming behaviour; fetchMs is reported separately because it is part of first-page latency",
      renderSize: primaryInfo?.result?.render ?? null,
      renderPaths:
        "headline = pdf.js 'default' path (page.render({canvas})), which is what the stock pdf.js viewer does; explicit-context probe re-renders pages with a harness-owned getContext('2d') context",
      timeoutPerRunMs: TIMEOUT_MS,
      smoke: SMOKE,
      rawEvidence: "results.raw.jsonl (append-only, every run including the harness self-test), logs/",
    },
    runs,
    comparison: buildComparison(runs),
  };

  await fsp.writeFile(RESULTS_JSON, JSON.stringify(out, null, 2));
  process.stdout.write(`\nwrote ${RESULTS_JSON}\n`);
  printSummary(out);
  process.exit(0);
}

function buildComparison(runs) {
  const prim = runs.filter((r) => r.pdf === PRIMARY_PDF && r.build === "modern");
  const pick = (which) => prim.find((r) => r.engine === which);

  // Derived worker classification, applied identically to every run: pdf.js v6
  // exposes the Worker object itself as PDFWorker.port, so "is it a dedicated
  // worker" is proven by the Worker constructor call, not by the port class.
  const deriveWorker = (e) => {
    const w = e?.result?.worker;
    if (!w) return null;
    const ctorCalls = w.workerCtorCalls ?? [];
    const workerErrors = w.workerErrorEvents ?? [];
    const dedicated =
      w.dedicatedWorker === true ||
      (ctorCalls.length > 0 && w.portConstructor !== "LoopbackPort" && workerErrors.length === 0);
    const fetched = Object.entries(e?.serverStats?.files ?? {}).find(
      ([k, v]) => k.endsWith("pdf.worker.mjs") && v.requests > 0,
    );
    return {
      workerTypeDerived: dedicated
        ? "dedicated-worker"
        : w.portConstructor === "LoopbackPort"
          ? "fake-worker (main thread)"
          : "unknown",
      portConstructor: w.portConstructor ?? null,
      workerCtorCalls: ctorCalls,
      workerErrorEvents: workerErrors,
      workerScriptFetchedByPage: !!fetched,
      workerScriptFetchCount: fetched ? fetched[1].requests : 0,
      workerScriptBytes: fetched ? fetched[1].bytesServed : 0,
    };
  };

  // The harness asserts worker health itself and records that assertion in
  // errors[]; separate it from real pdf.js errors so reporting stays honest.
  const splitErrors = (e) => {
    const all = e?.result?.errors ?? [];
    return {
      totalRecorded: all.length,
      harnessAssertions: all.filter((x) => x.type === "worker").length,
      pdfjsOrRuntimeErrors: all.filter((x) => x.type !== "worker"),
      pdfjsConsoleLines: (e?.result?.consoleLines ?? []).length,
    };
  };

  const row = (e) => {
    if (!e) return null;
    const seq = e.result?.sequential;
    const pages = seq?.pages ?? [];
    const probe = e.result?.explicitContextProbe;
    const pairs = pages
      .filter((p) => (probe?.pages ?? []).some((q) => q.page === p.page))
      .map((p) => ({
        page: p.page,
        defaultMs: p.totalMs,
        explicitMs: probe.pages.find((q) => q.page === p.page).totalMs,
      }));
    const meanOf = (a, k) => (a.length ? Number((a.reduce((s, x) => s + x[k], 0) / a.length).toFixed(2)) : null);
    const ratioOf = (a, b) => (a && b ? Number((a / b).toFixed(2)) : null);
    return {
      runId: e.runId,
      status: e.outcome,
      numPages: e.result?.numPages ?? null,
      worker: deriveWorker(e),
      errors: splitErrors(e),
      firstPageVisibleMs: e.result?.firstPage?.navigationToFirstVisibleMs ?? null,
      firstPageRenderCompleteMs:
        e.result?.firstPage?.navigationToRenderCompleteMs ??
        (e.result?.fetchMs != null && e.result?.docLoadMs != null && e.result?.firstPage?.renderOnlyMs != null
          ? Number((e.result.fetchMs + e.result.docLoadMs + e.result.firstPage.renderOnlyMs).toFixed(2))
          : null),
      firstPageRenderMs: e.result?.firstPage?.renderOnlyMs ?? null,
      firstPageRafMs: e.result?.firstPage?.rafMs ?? null,
      fetchMs: e.result?.fetchMs ?? null,
      docLoadMs: e.result?.docLoadMs ?? null,
      seqCount: seq?.count ?? null,
      seqMeanMs: seq?.meanTotalMs ?? null,
      seqMedianMs: seq?.medianTotalMs ?? null,
      seqMinMs: seq?.minTotalMs ?? null,
      seqMaxMs: seq?.maxTotalMs ?? null,
      seqMeanRenderOnlyMs: seq?.meanRenderOnlyMs ?? null,
      seqMeanRafMs: seq?.meanRafMs ?? null,
      seqPagesOver1000ms: pages.filter((p) => p.totalMs > 1000).length,
      throughputPagesPerSecond: seq?.throughputPagesPerSecond ?? null,
      midDocumentJumpMs: e.result?.randomAccess?.midDocumentMs ?? null,
      randomAccessJumps: (e.result?.randomAccess?.jumps ?? []).map((j) => ({ name: j.name, page: j.page, totalMs: j.totalMs, renderMs: j.renderMs })),
      explicitContextMeanTotalMs: probe?.meanTotalMs ?? null,
      defaultVsExplicitSamePages: {
        pages: pairs,
        defaultMeanMs: meanOf(pairs, "defaultMs"),
        explicitMeanMs: meanOf(pairs, "explicitMs"),
        explicitOverDefault: ratioOf(meanOf(pairs, "explicitMs"), meanOf(pairs, "defaultMs")),
      },
      featureSupport: e.result?.featureSupport?.webAssemblyProbe ?? null,
      wasmAvailableRecorded: e.result?.featureSupport?.hasWebAssembly ?? null,
      failedStage: e.result?.failedStage ?? null,
      pageLoadToFinishMs: e.result?.navigationToFinishMs ?? null,
    };
  };

  const wRow = row(pick("webkitgtk"));
  const cRow = row(pick("chrome"));
  const ratio = (a, b) => (a && b ? Number((a / b).toFixed(2)) : null);

  // Harness defects found during the spike are recorded here rather than
  // silently rewritten: the raw values stay in runs[].result, and the
  // correction is stated next to them.
  const harnessArtifactsCorrected = [];
  for (const e of runs) {
    const f = e.result?.featureSupport;
    const w = e.result?.worker;
    const fields = {};
    if (f && f.hasWebAssembly === false && f.webAssemblyProbe?.typeofWebAssembly === "object") {
      fields["featureSupport.hasWebAssembly"] =
        "recorded false, actually true: the page tested `typeof WebAssembly === \"function\"`, but in browsers typeof WebAssembly is \"object\"";
    }
    if (f?.webAssemblyProbe && f.webAssemblyProbe.instantiateError) {
      fields["featureSupport.webAssemblyProbe"] = `wasm instantiation error: ${f.webAssemblyProbe.instantiateError}`;
    }
    if (w && w.workerType === "unknown" && (w.workerCtorCalls ?? []).length > 0) {
      fields["worker.workerType"] =
        "recorded unknown, actually dedicated-worker: the page expected PDFWorker.port to be a MessagePort, but pdf.js v6 exposes the Worker object itself";
    }
    if ((e.result?.errors ?? []).some((x) => x.type === "worker")) {
      fields["errors[worker]"] = "harness self-assertion (see worker.workerType above), not a pdf.js error";
    }
    if (Object.keys(fields).length) harnessArtifactsCorrected.push({ runId: e.runId, fields });
  }

  return {
    primaryPdf: PRIMARY_PDF,
    webkitgtk: wRow,
    chrome: cRow,
    ratiosWebkitOverChrome: {
      firstPageVisibleMs: ratio(wRow?.firstPageVisibleMs, cRow?.firstPageVisibleMs),
      firstPageRenderMs: ratio(wRow?.firstPageRenderMs, cRow?.firstPageRenderMs),
      seqMeanMs: ratio(wRow?.seqMeanMs, cRow?.seqMeanMs),
      seqMedianMs: ratio(wRow?.seqMedianMs, cRow?.seqMedianMs),
      seqMaxMs: ratio(wRow?.seqMaxMs, cRow?.seqMaxMs),
      midDocumentJumpMs: ratio(wRow?.midDocumentJumpMs, cRow?.midDocumentJumpMs),
      webkitThroughputDeficit: ratio(cRow?.throughputPagesPerSecond, wRow?.throughputPagesPerSecond),
    },
    webkitRenderedSuccessfully: wRow ? wRow.status === "ok" : false,
    harnessArtifactsCorrected,
    note: "worker.workerTypeDerived is recomputed here from raw fields (Worker constructor calls + port class). errors.harnessAssertions counts the harness's own worker self-assertions separately from real pdf.js errors. See harnessArtifactsCorrected for field-level corrections found during the spike.",
  };
}

function printSummary(out) {
  const c = out.comparison;
  process.stdout.write("\n---------------- COMPARISON (136 MB PDF) ----------------\n");
  process.stdout.write(
    `${"metric".padEnd(28)}${"webkitgtk".padStart(14)}${"chrome".padStart(14)}\n`,
  );
  const m = [
    ["status", "status"],
    ["pages", "numPages"],
    ["worker", "worker"],
    ["first page visible (ms)", "firstPageVisibleMs"],
    ["first page render (ms)", "firstPageRenderMs"],
    ["first page rAF (ms)", "firstPageRafMs"],
    ["fetch (ms)", "fetchMs"],
    ["getDocument (ms)", "docLoadMs"],
    ["seq mean (ms)", "seqMeanMs"],
    ["seq median (ms)", "seqMedianMs"],
    ["seq min (ms)", "seqMinMs"],
    ["seq max (ms)", "seqMaxMs"],
    ["seq pages >1s", "seqPagesOver1000ms"],
    ["seq mean rAF (ms)", "seqMeanRafMs"],
    ["throughput (pages/s)", "throughputPagesPerSecond"],
    ["mid-doc jump (ms)", "midDocumentJumpMs"],
    ["explicit-ctx mean (ms)", "explicitContextMeanTotalMs"],
    ["pdf.js errors", "errors"],
    ["failed stage", "failedStage"],
    ["page load->finish (ms)", "pageLoadToFinishMs"],
  ];
  const fmt = (v) => {
    if (v && typeof v === "object") {
      if ("workerTypeDerived" in v) return v.workerTypeDerived;
      if ("totalRecorded" in v) return String(v.totalRecorded);
      return "-";
    }
    return String(v ?? "-");
  };
  for (const [label, key] of m) {
    process.stdout.write(
      `${label.padEnd(28)}${fmt(c.webkitgtk?.[key]).padStart(14)}${fmt(c.chrome?.[key]).padStart(14)}\n`,
    );
  }
  process.stdout.write(`webkitgtk rendered successfully: ${c.webkitRenderedSuccessfully}\n`);
}

main().catch(async (e) => {
  process.stderr.write(`runner failed: ${e?.stack || e}\n`);
  stopServer();
  process.exit(1);
});

// r12 spike measuring page.
// Self-measures pdf.js render performance and POSTs the JSON result to the
// collector at /result. The runner only loads the URL and waits for the POST.
//
// Query params:
//   engine  = label for this engine (webkitgtk | chrome | ...)   [required]
//   run     = run id, used to correlate the POST with the launch  [required]
//   pdf     = file name under /pdf/                              [required]
//   build   = modern | legacy       (default modern)
//   seq     = number of sequential pages to render (default 20)
//   probe   = pages to render with the explicit-context path (default 5)
//   smoke   = 1 -> skip the sequential and random access phases (harness self-test)
//
// Two render paths are measured, because they are not equivalent:
//   "default"  -> page.render({ canvas, viewport })      : pdf.js builds the 2D
//                 context itself with { alpha:false, willReadFrequently:true }.
//                 This is exactly what the stock pdf.js viewer does.
//   "explicit" -> page.render({ canvasContext, viewport }): the harness supplies a
//                 plain getContext("2d") context (alpha:true, no
//                 willReadFrequently), which keeps the GPU-accelerated canvas path
//                 available where the engine has one.
// The headline comparison uses "default" on both engines; "explicit" is a probe.
const P = new URLSearchParams(location.search);
const ENGINE = P.get("engine") || "unknown";
const RUN = P.get("run") || "norun";
const PDF_FILE = P.get("pdf") || "";
const BUILD = P.get("build") === "legacy" ? "legacy" : "modern";
const SEQ_PAGES = Math.max(1, Number(P.get("seq") || 20));
const PROBE_PAGES = Math.max(0, Number(P.get("probe") || 5));
const SMOKE = P.get("smoke") === "1";
const TARGET_WIDTH = 900; // CSS px, identical on both engines
const MAX_SCALE = 2.0;

const t = {
  scriptStartMs: performance.now(), // ms since navigationStart
  timeOriginEpochMs: performance.timeOrigin,
};
const logLines = [];
const errors = [];
const consoleLines = [];
const workerCtorCalls = [];
const workerErrors = [];

// Capture every console line the engine/pdf.js emits (e.g. "Setting up fake worker.")
for (const level of ["log", "info", "warn", "error", "debug"]) {
  const orig = console[level]?.bind(console);
  console[level] = (...args) => {
    if (consoleLines.length < 200) {
      consoleLines.push({
        level,
        at: Number(performance.now().toFixed(1)),
        text: args
          .map((a) => {
            try {
              return typeof a === "string" ? a : String(a);
            } catch {
              return "[unprintable]";
            }
          })
          .join(" ")
          .slice(0, 1000),
      });
    }
    orig?.(...args);
  };
}

// Definitive worker evidence: observe whether pdf.js actually constructs a Worker.
const NativeWorker = globalThis.Worker;
if (typeof NativeWorker === "function") {
  globalThis.Worker = class ObservedWorker extends NativeWorker {
    constructor(scriptUrl, options) {
      workerCtorCalls.push({
        at: Number(performance.now().toFixed(1)),
        url: String(scriptUrl),
        type: options?.type ?? null,
      });
      super(scriptUrl, options);
      this.addEventListener("error", (ev) => {
        workerErrors.push({
          at: Number(performance.now().toFixed(1)),
          message: String(ev.message ?? "worker error event"),
          filename: ev.filename ?? null,
          lineno: ev.lineno ?? null,
        });
      });
    }
  };
  globalThis.Worker.prototype.constructor = NativeWorker;
}
let lastLoggedStage = null;

const hud = document.getElementById("hud");
const logEl = document.getElementById("log");
// canvasA: pdf.js owns the 2D context (default path). canvasB: harness owns it.
const canvasA = document.getElementById("stageDefault");
const canvasB = document.getElementById("stageExplicit");
const ctxB = canvasB.getContext("2d"); // default attributes, GPU-friendly

function render() {
  logEl.textContent = logLines.join("\n");
  hud.textContent = `${ENGINE} · ${PDF_FILE} · ${BUILD}\n${logLines[logLines.length - 1] || ""}`;
}
// Progress telemetry: fire-and-forget so a hung run still leaves a trace on disk.
function progress(msg) {
  try {
    void fetch("/log", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ engine: ENGINE, run: RUN, at: performance.now(), stage: msg }),
    }).catch(() => {});
  } catch {}
}
function log(msg) {
  logLines.push(`${performance.now().toFixed(1)}ms ${msg}`);
  render();
  progress(msg);
}
function recordError(type, err, extra = {}) {
  const e = {
    type,
    at: Number(performance.now().toFixed(2)),
    message: String(err?.message ?? err),
    name: err?.name ?? null,
    stack: err?.stack ? String(err.stack).slice(0, 4000) : null,
    ...extra,
  };
  errors.push(e);
  log(`ERROR ${type}: ${e.message}`);
}

addEventListener("error", (ev) => {
  recordError("window.error", ev.error || new Error(ev.message), {
    source: ev.filename ? `${ev.filename}:${ev.lineno}:${ev.colno}` : null,
  });
});
addEventListener("unhandledrejection", (ev) => {
  recordError("unhandledrejection", ev.reason);
});

const round = (n) => (Number.isFinite(n) ? Number(n.toFixed(2)) : null);
const mean = (a) => (a.length ? a.reduce((x, y) => x + y, 0) / a.length : null);
const nextFrames = async () => {
  const t0 = performance.now();
  await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
  return round(performance.now() - t0);
};

// pdf.js v6 ships WASM (jbig2/openjpeg/qcms) and falls back to JS without it.
async function probeWebAssembly() {
  const out = { typeofWebAssembly: typeof WebAssembly };
  if (typeof WebAssembly === "undefined") return out;
  const empty = new Uint8Array([0, 0x61, 0x73, 0x6d, 1, 0, 0, 0]);
  try {
    out.validateEmptyModule = WebAssembly.validate(empty);
  } catch (e) {
    out.validateError = String(e);
  }
  try {
    const m = await WebAssembly.instantiate(empty);
    out.instantiateEmptyModule = !!m.instance;
  } catch (e) {
    out.instantiateError = String(e);
  }
  return out;
}

async function post(endpoint, body) {
  const r = await fetch(endpoint, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  if (!r.ok) throw new Error(`POST ${endpoint} -> ${r.status}`);
  return r.json().catch(() => ({}));
}

async function main() {
  const result = {
    status: "running",
    engine: ENGINE,
    run: RUN,
    build: BUILD,
    pdf: PDF_FILE,
    mode: SMOKE ? "smoke" : "full",
    seqPagesRequested: SMOKE ? 0 : SEQ_PAGES,
    startedAt: new Date().toISOString(),
    navigationToScriptStartMs: round(t.scriptStartMs),
    featureSupport: {
      userAgent: navigator.userAgent,
      platform: navigator.platform,
      hardwareConcurrency: navigator.hardwareConcurrency,
      devicePixelRatio: window.devicePixelRatio,
      hasFloat16Array: typeof Float16Array !== "undefined",
      hasPromiseWithResolvers: typeof Promise.withResolvers === "function",
      hasStructuredClone: typeof structuredClone === "function",
      hasOffscreenCanvas: typeof OffscreenCanvas !== "undefined",
      hasImageDecoder: typeof ImageDecoder !== "undefined",
      hasCreateImageBitmap: typeof createImageBitmap === "function",
      hasWebAssembly: typeof WebAssembly !== "undefined",
      webAssemblyProbe: await probeWebAssembly(),
      // Chrome-only non-standard API
      jsHeapLimitBytes: performance.memory ? performance.memory.jsHeapSizeLimit : null,
    },
  };

  let stage = "start";
  let pdfDoc = null;

  async function renderTo(canvas, ctx, pageNum, scale, mode, note) {
    const tg = performance.now();
    const page = await pdfDoc.getPage(pageNum);
    const getPageMs = performance.now() - tg;
    const vp = page.getViewport({ scale });
    canvas.width = Math.max(1, Math.floor(vp.width));
    canvas.height = Math.max(1, Math.floor(vp.height));
    const tr = performance.now();
    const params = { viewport: vp };
    if (mode === "explicit") params.canvasContext = ctx;
    else params.canvas = canvas;
    await page.render(params).promise;
    const renderMs = performance.now() - tr;
    const renderDoneMs = performance.now();
    // two rAFs: the frame in which the canvas is actually composited.
    // NOTE: on an unfocused/occluded window this can be throttled, so it is
    // recorded separately from renderMs and never used as the render cost.
    const rafMs = await nextFrames();
    const paintedMs = performance.now();
    const out = {
      page: pageNum,
      mode,
      note,
      getPageMs: round(getPageMs),
      renderMs: round(renderMs),
      totalMs: round(renderDoneMs - tg),
      rafMs,
      paintedAtMs: round(paintedMs),
      canvasWidth: canvas.width,
      canvasHeight: canvas.height,
    };
    page.cleanup();
    return out;
  }
  const stats = (rows) => {
    const totals = rows.map((r) => r.totalMs);
    const renders = rows.map((r) => r.renderMs);
    const sorted = [...totals].sort((a, b) => a - b);
    return {
      count: rows.length,
      totalMs: round(totals.reduce((a, b) => a + b, 0)),
      meanTotalMs: round(mean(totals)),
      medianTotalMs: round(sorted[Math.floor(sorted.length / 2)]),
      minTotalMs: round(Math.min(...totals)),
      maxTotalMs: round(Math.max(...totals)),
      meanRenderOnlyMs: round(mean(renders)),
      maxRenderOnlyMs: round(Math.max(...renders)),
      meanRafMs: round(mean(rows.map((r) => r.rafMs))),
      meanGetPageMs: round(mean(rows.map((r) => r.getPageMs))),
      throughputPagesPerSecond: round(rows.length / (totals.reduce((a, b) => a + b, 0) / 1000)),
    };
  };

  try {
    stage = "fetch-pdf";
    const pdfUrl = `/pdf/${encodeURIComponent(PDF_FILE)}`;
    const tf0 = performance.now();
    const resp = await fetch(pdfUrl);
    if (!resp.ok) throw new Error(`GET ${pdfUrl} -> ${resp.status}`);
    result.http = {
      status: resp.status,
      contentType: resp.headers.get("content-type"),
      contentLength: Number(resp.headers.get("content-length")) || null,
      acceptRanges: resp.headers.get("accept-ranges"),
      contentEncoding: resp.headers.get("content-encoding"),
    };
    const buf = await resp.arrayBuffer();
    result.bytesFetched = buf.byteLength;
    result.fetchDoneMs = round(performance.now());
    result.fetchMs = round(performance.now() - tf0);
    log(`fetched ${buf.byteLength} bytes in ${result.fetchMs} ms`);

    stage = "import-pdfjs";
    const ti0 = performance.now();
    const base = BUILD === "legacy" ? "/vendor/legacy/build" : "/vendor/build";
    let pdfjs;
    try {
      pdfjs = await import(/* @vite-ignore */ `${base}/pdf.mjs`);
    } catch (e) {
      recordError("import.build", e, { build: BUILD, url: `${base}/pdf.mjs` });
      throw e;
    }
    result.pdfjsVersion = pdfjs.version ?? null;
    result.pdfjsBuild = pdfjs.build ?? null;
    result.importMs = round(performance.now() - ti0);
    result.assetUrls = {
      workerSrc: `${base}/pdf.worker.mjs`,
      cMapUrl: "/vendor/cmaps/",
      standardFontDataUrl: "/vendor/standard_fonts/",
      wasmUrl: "/vendor/wasm/",
      iccUrl: "/vendor/iccs/",
    };
    pdfjs.GlobalWorkerOptions.workerSrc = result.assetUrls.workerSrc;
    log(`pdf.js ${result.pdfjsVersion} (${BUILD}) imported in ${result.importMs} ms`);

    stage = "getDocument";
    const td0 = performance.now();
    const task = pdfjs.getDocument({
      // pdf.js defaults only; just point it at the vendored static assets
      data: buf,
      cMapUrl: result.assetUrls.cMapUrl,
      cMapPacked: true,
      standardFontDataUrl: result.assetUrls.standardFontDataUrl,
      wasmUrl: result.assetUrls.wasmUrl,
      iccUrl: result.assetUrls.iccUrl,
    });
    pdfDoc = await task.promise;
    result.docLoadMs = round(performance.now() - td0);
    result.docReadyAtMs = round(performance.now());
    result.numPages = pdfDoc.numPages;
    result.fingerprints = pdfDoc.fingerprints ?? null;
    log(`getDocument ok: ${result.numPages} pages in ${result.docLoadMs} ms`);

    const worker = task._worker ?? pdfDoc.loadingTask?._worker ?? null;
    const workerPortCtor = worker?.port ? (worker.port.constructor?.name ?? null) : null;
    // pdf.js v6: a dedicated worker is proven by (a) the Worker constructor being
    // invoked by pdf.js and (b) the port not being pdf.js's in-process
    // LoopbackPort. The dedicated-worker port is the Worker object itself in v6,
    // not a MessagePort.
    const dedicatedWorker =
      workerCtorCalls.length > 0 && workerPortCtor !== "LoopbackPort" && workerErrors.length === 0;
    const fakeWorker = workerPortCtor === "LoopbackPort";
    result.worker = {
      workerSrc: pdfjs.GlobalWorkerOptions.workerSrc,
      globalWorkerOptionsWorkerPort: pdfjs.GlobalWorkerOptions.workerPort ? "set" : "null",
      loadingTaskHasWorker: !!task._worker,
      pdfWorkerClass: worker?.constructor?.name ?? null,
      portConstructor: workerPortCtor,
      dedicatedWorker,
      fakeWorker,
      workerType: dedicatedWorker ? "dedicated-worker" : fakeWorker ? "fake-worker (main thread)" : "unknown",
      workerCtorCalls,
      workerErrorEvents: workerErrors,
      workerDestroyed: worker?.destroyed ?? null,
      note: "pdf.js v6 keeps the PDFWorker on loadingTask._worker; the dedicated-worker port is the Worker object, a fake worker uses LoopbackPort. pdf.js parses in the worker but rasterizes on the main thread.",
    };
    if (!dedicatedWorker) {
      recordError(
        "worker",
        new Error(
          `pdf.js is NOT using a dedicated worker (port=${workerPortCtor}, ctorCalls=${JSON.stringify(workerCtorCalls)}, workerErrors=${JSON.stringify(workerErrors)})`,
        ),
      );
    }
    log(`worker: ${result.worker.workerType} (port=${workerPortCtor}, Worker ctor calls=${workerCtorCalls.length}, worker errors=${workerErrors.length})`);

    // baseline geometry from page 1 -> identical render size on both engines
    stage = "page1-viewport";
    const p1 = await pdfDoc.getPage(1);
    const baseVp = p1.getViewport({ scale: 1 });
    const scale = Math.min(MAX_SCALE, TARGET_WIDTH / baseVp.width);
    result.render = {
      targetWidthCssPx: TARGET_WIDTH,
      scale: Number(scale.toFixed(4)),
      page1BaseViewport: { width: round(baseVp.width), height: round(baseVp.height) },
      dprAppliedToCanvas: false,
      defaultPathContextAttributes: "determined by pdf.js (alpha:false, willReadFrequently:!enableHWA)",
      explicitPathContextAttributes: ctxB.getContextAttributes ? ctxB.getContextAttributes() : null,
    };

    // --- FIRST PAGE LATENCY: navigationStart -> first page actually composited
    stage = "first-page";
    const first = await renderTo(canvasA, null, 1, scale, "default", "first-page");
    result.firstPage = {
      ...first,
      definition:
        "navigationStart -> page 1 rendered AND two rAFs elapsed (canvas visibly composited). rafMs is reported separately because rAF can be throttled in an unfocused window.",
      navigationToFirstVisibleMs: first.paintedAtMs,
      navigationToRenderCompleteMs: round(result.fetchMs + result.docLoadMs + first.renderMs),
      fetchPlusDocLoadMs: round(result.fetchMs + result.docLoadMs),
      renderOnlyMs: first.renderMs,
    };
    result.render.defaultPathContextAttributes = canvasA.getContext
      ? canvasA.getContext("2d").getContextAttributes?.() ?? null
      : null;
    log(`first page visible at ${first.paintedAtMs} ms (render ${first.renderMs} ms)`);

    result.phases = {};

    if (!SMOKE) {
      stage = "sequential";
      const seq = [];
      for (let n = 1; n <= SEQ_PAGES; n++) {
        const r = await renderTo(canvasA, null, n, scale, "default", "sequential");
        seq.push(r);
        if (n % 5 === 0) log(`sequential ${n}/${SEQ_PAGES} (last total ${r.totalMs} ms, render ${r.renderMs} ms)`);
      }
      result.sequential = { ...stats(seq), pages: seq, phaseStartMs: seq[0].paintedAtMs, firstPageMs: seq[0].totalMs, lastPageMs: seq[seq.length - 1].totalMs };
      result.phases.sequentialTotalMs = result.sequential.totalMs;

      stage = "random-access";
      const mid = Math.floor(pdfDoc.numPages / 2);
      const targets = [
        { name: "mid_N_over_2", page: mid },
        { name: "quarter", page: Math.max(1, Math.floor(pdfDoc.numPages / 4)) },
        { name: "three_quarter", page: Math.max(1, Math.floor((3 * pdfDoc.numPages) / 4)) },
      ];
      result.randomAccess = {
        midPageNumber: mid,
        definition: "getPage(N) + full render + two rAFs, after the sequential phase (page cache warm)",
        jumps: [],
      };
      for (const tgt of targets) {
        const r = await renderTo(canvasA, null, tgt.page, scale, "default", tgt.name);
        result.randomAccess.jumps.push({ ...r, name: tgt.name });
        log(`random ${tgt.name} (page ${tgt.page}) total ${r.totalMs} ms`);
      }
      result.randomAccess.midDocumentMs = result.randomAccess.jumps[0].totalMs;
      result.phases.randomAccessMs = result.randomAccess.midDocumentMs;

      if (PROBE_PAGES > 0) {
        stage = "explicit-context-probe";
        const probe = [];
        for (let n = 1; n <= PROBE_PAGES; n++) {
          probe.push(await renderTo(canvasB, ctxB, n, scale, "explicit", "explicit-context-probe"));
        }
        result.explicitContextProbe = { ...stats(probe), pages: probe };
        log(`explicit-context probe mean ${result.explicitContextProbe.meanTotalMs} ms`);
      }
    }

    stage = "done";
    result.finishedAt = new Date().toISOString();
    result.navigationToFinishMs = round(performance.now());
    result.status = "ok";
  } catch (err) {
    recordError("fatal", err, { stage });
    result.status = "error";
    result.failedStage = stage;
  }

  result.errors = errors;
  result.errorCount = errors.length;
  result.consoleLines = consoleLines;
  result.pageLog = logLines;
  result.wallClockTotalMs = round(performance.now());

  log(`POSTing result status=${result.status}`);
  try {
    await post(`/result?run=${encodeURIComponent(RUN)}`, result);
    hud.textContent = `POSTED status=${result.status} errors=${result.errorCount}`;
    document.title = `r12-${ENGINE}-${result.status}`;
  } catch (e) {
    hud.textContent = `POST FAILED: ${e}`;
  }
}

main().catch((e) => {
  // main() should not throw, but keep a safety net so the runner gets a signal
  // instead of a bare timeout.
  recordError("outer", e);
  void post(`/result?run=${encodeURIComponent(RUN)}`, {
    status: "error",
    engine: ENGINE,
    run: RUN,
    build: BUILD,
    pdf: PDF_FILE,
    failedStage: "outer",
    errors,
    pageLog: logLines,
  });
});

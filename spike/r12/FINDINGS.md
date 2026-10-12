# R12 spike — pdf.js rendering on WebKitGTK: good enough?

**Question.** Is pdf.js rendering performance on WebKitGTK good enough to be the rendering foundation of a
Tauri-based PDF reader on Linux? Chrome is used as a *reference point*, not as the decision-maker.

**Answer (short).** **Yes — for rendering performance, on this hardware and these documents.** WebKitGTK 4.1
2.52.6 rendered pdf.js 6.4.299 successfully on both real PDFs, with a dedicated ES-module worker and the
wasm path active, and it produced **no errors and no console warnings**. On the 136 MB, 436-page PDF it was
at *rough parity* with Chrome 155: faster on the image-heavy pages, slower on small text pages, and
1.2× faster overall in throughput. R12 ("WebKitGTK is the weak link") is **not confirmed for render
throughput**. A native `pdfium` path is not needed on this evidence; keep it behind one module as an
escape hatch, do not build it now.

Machine / versions:
`Ubuntu 24.04.5 LTS · GNOME 46 · Wayland with XWayland (:0) · 12 cores · 14 GB RAM · WebKitGTK 4.1 2.52.6
(libwebkit2gtk-4.1-0) via MiniBrowser · Google Chrome 155.0.8059.39 (real windowed mode) · pdf.js 6.4.299
(npm pdfjs-dist 6.4.299, vendored) · node v22.20.0`

## Primary target — `Learning Patterns...pdf`, 142,801,554 bytes, 436 pages

| Metric (900 px-wide canvas, letter page) | WebKitGTK 2.52.6 | Chrome 155 | WebKit / Chrome |
| --- | --- | --- | --- |
| pdf.js version / build | 6.4.299 / d0991a0d5 | 6.4.299 / d0991a0d5 | — |
| Worker | dedicated `type:module` worker | dedicated `type:module` worker | same |
| WebAssembly (qcms) | available, instantiated, `qcms_bg.wasm` fetched | same | same |
| pdf.js console warnings | 0 | 0 | same |
| Fetch 136 MB over localhost (ms) | 1861 | 2123 | 0.88 |
| `getDocument` (ms) | 121 | 276 | 0.44 |
| **First page render only (ms)** | **2827** | **4004** | **0.71** |
| **First page visible incl. fetch (ms)** | **4953** | **7662** | **0.65** |
| Sequential pages 1–20, total (ms) | 16 552 | 19 373 | 0.85 |
| Sequential **mean** page (ms) | 827.6 | 968.6 | 0.85 |
| Sequential **median** page (ms) | 167 | 139.6 | 1.20 |
| Sequential min / max page (ms) | 21 / 4379 | 13.6 / 5356 | — |
| Pages over 1000 ms (of 20) | 4 | 4 | same |
| **Throughput (pages/s)** | **1.21** | **1.03** | 0.85 (Chrome/WebKit) |
| Random access, page 218 = N/2 (ms) | 165 | 140.5 | 1.17 |
| Random access, page 109 / 327 (ms) | 125 / 25 | 134.7 / 18.9 | — |
| Mean compositor rAF after render (ms) | 24.7 | 22.5 | — |
| Page load → end of whole script (ms) | 31 846 | 37 810 | — |
| JS exceptions | 0 (see note ¹) | 0 | same |

¹ The single `errors[]` entry in the WebKitGTK run is the harness's own worker-health assertion misfiring on
pdf.js v6 internals (v6 exposes the `Worker` object itself as `PDFWorker.port`), not a pdf.js error. It is
separated as `errors.harnessAssertions` in `results.json`, the corrected classifier is used for every run,
and both corrections are listed explicitly in `comparison.harnessArtifactsCorrected`.

Per-page detail explains the mean/median split. Pages 1, 2, 5 and 6 each contain **a single 7650×9900 px
image (~76 megapixels)** — `pdfimages -list` — and cost 2.7–5.4 s on *both* engines. Every other page in the
sample cost 13–283 ms. So the workload is dominated by decoding/scaling those few huge images, and that cost
is pdf.js-side, not engine-side. That is why the two engines converge.

| Page | Content (`pdfimages -list`) | WebKitGTK (ms) | Chrome (ms) |
| --- | --- | --- | --- |
| 1 | 1 × 7650×9900 image | 2663 | 2682 |
| 2 | 1 × 7650×9900 jpeg | 3788 | 5356 |
| 3 | 2 × 1280×1280 images | 432 | 199 |
| 5 | 1 × 7650×9900 jpeg | 4379 | 4740 |
| 6 | 1 × 7650×9900 jpeg | 3439 | 4721 |
| 7 | text only | 21 | 18.5 |

## Secondary target — `Cracking the Coding Interview.pdf`, 64,112,405 bytes, 708 pages

WebKitGTK only (the launch budget was deliberately capped; the decision engine here is WebKitGTK):

| Metric | WebKitGTK 2.52.6 |
| --- | --- |
| Fetch 61 MB over localhost (ms) | 489 |
| `getDocument` (ms) | 123 |
| First page render only (ms) | 366 |
| First page visible incl. fetch (ms) | 1086 |
| Sequential pages 1–20 mean / median (ms) | 116.3 / 116 |
| Sequential min / max (ms) | 70 / 173 |
| Throughput (pages/s) | 8.6 |
| Random access, pages 354 / 177 / 531 (ms) | 122 / 123 / 114 |
| Page load → finish (ms) | 4767 |
| JS exceptions / console warnings | 0 / 0 |

On a text-and-figure book WebKitGTK renders pdf.js at ~8.6 pages/s with no page over 173 ms. Both real
documents — one 136 MB image-heavy, one 61 MB text-heavy — are handled without failure.

## What worked

- pdf.js 6.4.299 **modern** ESM build runs unmodified on WebKitGTK 2.52.6: `Promise.withResolvers`,
  `Float16Array`, `structuredClone`, `OffscreenCanvas`, `createImageBitmap` all present; JS-only fallback
  paths in the bundle were not needed.
- **Dedicated worker works.** pdf.js constructed
  `new Worker("/vendor/build/pdf.worker.mjs", { type: "module" })` on both engines, the worker script was
  fetched exactly once per run (server-side evidence), and there were zero worker error events. No
  "Setting up fake worker" fallback, no main-thread parsing.
- **WebAssembly works on WebKitGTK.** `WebAssembly.validate` and `WebAssembly.instantiate` both succeeded,
  and pdf.js actually fetched `wasm/qcms_bg.wasm` on both engines. (An earlier harness revision reported
  `hasWebAssembly:false` because it tested `typeof WebAssembly === "function"`; in browsers the correct test
  is `!== "undefined"`. Fixed and re-measured.)
- Random access is cheap: a mid-document jump costs 165 ms (WebKitGTK) vs 141 ms (Chrome) on the 136 MB file.
- Sequential runs never hung; every phase completed well inside the 120 s per-run bound (31.8 s and 37.8 s
  for the whole WebKitGTK and Chrome scripts respectively).

## What failed / what did not go right

- **Nothing failed in the engines.** No JS exception, no worker failure, no timeout, no hang, on either
  engine, on either PDF. There is no negative result to report.
- Two defects were in **my harness**, found and corrected: the `typeof WebAssembly` test above, and a
  worker-health classifier that did not know pdf.js v6 exposes the `Worker` as `PDFWorker.port`. Both were
  fixed before the second and fourth launches and the affected fields are recomputed from raw values in
  `results.json`.
- **Latency variance is large.** Page 1 of the 136 MB PDF rendered in 4988 ms in the smoke launch and
  2827 ms in the measured launch on the *same* engine — ~1.8× spread. Treat single-run first-page numbers as
  ±30 %, not as benchmarks.

## Caveats — read before quoting any number

1. **MiniBrowser is a proxy, not Tauri.** It is the same WebKitGTK 4.1 engine (2.52.6), but not Tauri's
   webview configuration. Tauri v2 sets its own `WebKitSettings`, and on Linux its
   `hardwareAccelerationPolicy` / DMABUF/compositing setup differs. Every WebKitGTK number here is
   "the engine can do this", not "the Tauri app will do this". Re-verify inside a real Tauri window.
2. **Buffered delivery, not streaming.** The page fetches the whole PDF into an `ArrayBuffer` and passes it
   to `getDocument({ data })`, so the server saw `rangedRequests: 0` for every run. This isolates render cost
   from network behaviour, and it is the same on both engines, but a real reader would pass `url:` and let
   pdf.js range-stream. `fetchMs` is reported separately because it is inside the "first page visible"
   number: 1861 ms of the WebKitGTK 4953 ms is the 136 MB localhost download.
3. **No GPU-acceleration claim.** Both engines ran in real windowed mode on a Wayland session with
   XWayland. pdf.js's stock render path (`page.render({ canvas })`) makes the engine build a 2D context with
   `willReadFrequently: true`, which keeps the canvas on the software path in Chrome; MiniBrowser reported
   the same attribute. Whether WebKitGTK's accelerated compositing helped or hurt was not isolated.
4. **The `willReadFrequently` observation is indicative, not proven.** Re-rendering pages 1–5 with a
   harness-owned `getContext("2d")` context (`willReadFrequently: false`) was 19 % faster on WebKitGTK and
   25 % faster on Chrome, and 34 % faster on the secondary PDF — but that second pass also had warm caches,
   so the effect is confounded. Worth its own A/B experiment in the app; do not treat 19 % as measured.
5. **One measured run per engine.** Chrome ran second, WebKitGTK first; the user's own Chrome session was
   running throughout (two pre-existing `chrome_crashpad` processes were left untouched). `hardwareConcurrency`
   was reported as 8 by WebKitGTK and 12 by Chrome, which is itself a MiniBrowser/engine difference worth
   noting.
6. **pdf.js 6.4.299 ≠ the pdf.js viewer.** This measures the API (`getDocument` + `page.render`), not the
   full viewer with text layer, annotation layer, and continuous scrolling. Those add their own cost.

## Harness

`Workspace/Proyectos/oct/spike/r12/`

| Path | Role |
| --- | --- |
| `vendor/pdfjs-dist/` | vendored pdf.js 6.4.299 (`npm pack pdfjs-dist`), modern + legacy builds, cmaps, fonts, wasm |
| `public/index.html`, `public/measure.js` | self-measuring page; POSTs its JSON result to `/result` |
| `public/pdf/*.pdf` | **symlinks** to the user's real PDFs — nothing was copied or modified |
| `server.mjs` | localhost static server with HTTP Range + result collector + per-file fetch stats |
| `runner.mjs` | sequential launcher: resets stats, launches a real windowed browser, waits for the POST, kills the process group |
| `results.json` | measurements + derived comparison |
| `results.raw.jsonl` | append-only raw record of every run, including the harness self-test |
| `logs/` | per-run browser output, page progress logs, archived per-run entries, browser profiles |

```bash
cd Workspace/Proyectos/oct/spike/r12
node runner.mjs                     # primary PDF: WebKitGTK then Chrome, sequentially
node runner.mjs --report-only        # rebuild results.json from logs/run-*.json, no browser launched
node runner.mjs --only=webkitgtk --secondary --skip-primary
```

Four browser launches were used in total: 1 WebKitGTK smoke self-test, the 2 primary runs, and 1 WebKitGTK
run of the 61 MB PDF. Each run is bounded at 120 s and the browser process group is killed afterwards; no
browser or server process was left running.

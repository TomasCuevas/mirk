# Feature: automatic UI capture loop and the aesthetic pass it exposed

Status: **in progress** — T1 (capture script) landing first; the fixes follow in their own commit
Created: 2026-10-21
Branch: `feat/phase-1-core`
Engram mirror: `odd/ui-capture-loop/tasks`

## Specs

**S1 — the request, quoted verbatim (L1).**
> "Necesito que investigues alguna skill que permita al agente analizar el frontend de la aplicacion generada. Esto para evitar que yo manualmente tenga que tomar capturas, pasarselas al agente y que recien ahi pueda analizar. Esto deberia hacerlo el agente automaticamente, de esa forma, mejorar el criterio sobre la estetica de la UI"

The agent captures and inspects mirk's interface by itself. A capture that needs a human to take a
screenshot is a failure of this feature, not a workaround for it.

**S2 — two commits, in this order (L2: `"C"`).**

1. `scripts/capture-ui.mjs` — the capture loop, committed alone so every later review can reuse it.
2. The aesthetic fixes S3 names.

S2.1: commit 1 must not modify `web/src/lib/tauri.ts`. The seed is injected by the capture script;
the application's shared IPC bridge stays untouched.

**S3 — the defects the loop found, each verified against source, not inferred from a canvas.**

| # | Screen | Defect |
| --- | --- | --- |
| D1 | Home | The horizontal carousel's next/prev button floats **over the cover art** of the trailing card, with no inset. |
| D2 | Home | An empty section renders a full-size ghost card (~185×215 CSS px, diagonal stripes, "Nothing here yet.") that outranks the real content above it. |
| D3 | Library | Pagination (`‹ 1 ›`) renders **above** the grid, orphaned in a gap between the filters and the cards — and renders at all when there is only one page. |
| D4 | Library + Home | The author line carries the same size and nearly the same weight as the document title, so the two do not separate. |
| D5 | Library + Home | The provenance sentence ("recorded when Evince closed") overflows the card and is clipped by its right edge. |
| D6 | All | The bottom navbar's active pill has a fixed width while inactive items are content-width, so the three labels are spread unevenly. |
| D7 | Settings | The three section cards have no navigation affordance: no chevron, no hover state. They read as static text. |

Verified **not** a defect: the percentage pill sitting on the progress bar is deliberate
(`BookCard.svelte`: `left: clamp(2.1em, var(--at), calc(100% - 2.1em))`). It is not in scope.

**S4 — the capture matrix is the shipped window, quoted from `crates/mirk-desktop/tauri.conf.json`.**
Width 430, height 844, `resizable: false`, `maxWidth`/`maxHeight` equal. Captures are 430×844 at
device scale factor 2. No other viewport is a target, and "responsive" is out of scope: the window
cannot be resized, so a layout judged at 1440 px invents defects the user never sees.

**S5 — the capture fails loudly.** A JavaScript exception during capture is a non-zero exit, not a
warning: a screenshot of a crashed page is not evidence about design.

## Tasks

| ID | Specs | Task | Route | Commit |
| --- | --- | --- | --- | --- |
| T1 | S1, S2, S2.1, S4, S5 | Write `scripts/capture-ui.mjs`: seed, matrix, scroll-container full page, console gate | inline | 1 |
| T2 | S3 D1 | Inset the carousel controls so they never sit on cover art | inline | 2 |
| T3 | S3 D2 | Make the empty section slot compact instead of a full ghost card | inline | 2 |
| T4 | S3 D3 | Move pagination below the grid and hide it for a single page | inline | 2 |
| T5 | S3 D4 | Separate title from author in size and colour | inline | 2 |
| T6 | S3 D5 | Stop the provenance sentence from overflowing the card | inline | 2 |
| T7 | S3 D6 | Give the navbar's three items equal width | inline | 2 |
| T8 | S3 D7 | Give the settings section cards a chevron and a hover state | inline | 2 |
| T9 | S1 | Re-capture and confirm every fix in the screenshot, then report | inline | 2 |

## Log

**L1** — "Necesito que investigues alguna skill que permita al agente analizar el frontend de la
aplicacion generada. Esto para evitar que yo manualmente tenga que tomar capturas, pasarselas al
agente y que recien ahi pueda analizar. Esto deberia hacerlo el agente automaticamente, de esa forma,
mejorar el criterio sobre la estetica de la UI"

**L2** — "C" — both, in two commits: the script first, then the fixes.

**L3 (evidence)** — Spike over CDP (`/tmp/mirk-cdp/shot.mjs`, not committed): injecting
`window.__TAURI_INTERNALS__` before boot made the app render as inside Tauri. Probe:
`{"seeded":true,"cards":4,"images":4}`, 0 exceptions. The seven findings in S3 were read from those
captures at 430×844 @2x.

**L4 (measured)** — `main` in `App.svelte` is the scroll container (`overflow-y: auto`), so a
full-page capture must scroll that element; `Page.getLayoutMetrics` returns the window height and a
document-level capture silently yields one viewport.

**L5 (measured)** — The native Tauri window cannot be captured on this machine: a Wayland session
with no `grim`, `gnome-screenshot` or xdg-desktop-portal screenshot path, and `xwd` cannot see
Wayland surfaces. Chrome renders layout and hierarchy faithfully enough to judge those; it is not
proof of WebKitGTK paint fidelity.

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

**S3 — the defects the loop found, and what measurement did to each claim.**

The first pass read seven findings off the captures. Measuring them in the live page (geometry and
computed styles, not the canvas) confirmed two, disproved three, and found two to be deliberate
choices documented in the source. The verdict column is the specification now; a claim that was
never measured is not a task.

| # | Screen | Claim | Verdict |
| --- | --- | --- | --- |
| D1 | Home | The carousel arrow floats over the cover art | **Deliberate.** `.strip-arrow` is an absolute overlay with its own scrim (`rgba(7,8,10,.78)`), an accessible name, and a `disabled` state at each end (R15) |
| D2 | Home | An empty section renders a full-size ghost card | **Deliberate.** HomeScreen B1 reserves the cover slot on purpose and already made it shorter than a populated carousel |
| D3 | Library | Pagination renders above the grid | **Half.** The placement is the reader's own request (R10, L22, L27). The real defect is that it renders **at all with one page** — measured: `pageCount` 1 and the strip still drawn |
| D4 | Library + Home | The author line outranks the title | **Confirmed, and worse than claimed.** Measured: title `13px/600`, author `14px/400`, both `rgb(244,244,246)`. The author is a pixel *larger* than the name it belongs to |
| D5 | Library + Home | The provenance sentence overflows the card | **Disproved.** `.provenance` is 192 px wide with `scrollWidth` 192 and `white-space: normal`, in a 206 px card: it fits. The clipped text in the capture was the *second card*, cut by the carousel's own viewport |
| D6 | All | The navbar items are spread unevenly | **Disproved.** Measured: three items of 135 px at x = 8, 147, 287 |
| D7 | Settings | The section cards have no navigation affordance | **Half.** `.section:hover` exists (background and border both change), so "no hover state" was wrong. No chevron remains — a judgment, not a defect, and the source records the cards as one target on purpose |

In scope: D4 and the single-page case of D3.

**S3.1 — the hierarchy rule for D4.** On a card, the title outranks the author, and the author
outranks the provenance. Three sizes are not required, but the order is: title `--text-sm` semibold
in `--fg`, author `--text-xs` in `--fg-muted`, provenance `--text-xs` in `--fg-subtle`.

**S3.2 — the rule for D3.** The pager renders only when there is a page to turn (`pageCount > 1`).
Its position above and below the grid is untouched.

**S3.3 — what is not in scope, and why.** D1, D2, the pager's placement and D7's missing chevron are
recorded decisions in the source, each with the reader's own words behind it. Reversing them is a
product decision, not a defect fix, and this feature does not make it.

**S4 — the capture matrix is the shipped window, quoted from `crates/mirk-desktop/tauri.conf.json`.**
Width 430, height 844, `resizable: false`. **Superseded in part** by `odd/tasks/window-layout.md`: the window's size is the reader's layout choice, so the capture script takes `--layout` and opens the size that layout names. Captures are 430×844 at
device scale factor 2. No other viewport is a target, and "responsive" is out of scope: the window
cannot be resized, so a layout judged at 1440 px invents defects the user never sees.

**S5 — the capture fails loudly.** A JavaScript exception **or a console error** during capture is a
non-zero exit, not a warning: a screenshot of a crashed page is not evidence about design, and
neither is a screenshot of one whose data never loaded. Corrected on 2026-11-02 (L9): the rule the
script already enforced in code was broader than this spec said, which is exactly why the drift it
was meant to catch went unnoticed.

## Tasks

| ID | Specs | Task | Route | Commit |
| --- | --- | --- | --- | --- |
| T1 | S1, S2, S2.1, S4, S5 | Write `scripts/capture-ui.mjs`: seed, matrix, scroll-container full page, console gate | inline | `fc4db32` |
| T2 | S3 D4, S3.1 | Author one step below the title: `--text-xs` and `--fg-muted` | inline | `ac9ce39` |
| T3 | S3 D3, S3.2 | Draw the pager only when `pageCount > 1`, with a DOM test that fails first | inline | `ac9ce39` |
| T4 | S3 D1, D2, D7 | Dropped: each is a recorded decision, not a defect (S3.3) | — | — |
| T5 | S3 D5, D6 | Dropped: measurement disproved both claims | — | — |
| T6 | S1 | Re-capture and confirm both fixes in the image, then report | inline | `ac9ce39` |
| T7 | S5, S4 | The harness stops drifting: answer `get_window_preset`, default to the app's own layout, and correct the spec | inline | pending |

Verified before closing: `bun run typecheck` 0 errors and 0 warnings; `bun run test` 333 passed in 19
files (the new pager test is one of them); a fresh `node scripts/capture-ui.mjs` reported no
exceptions and the captures show the pager gone and the author a clear step under the title.

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

**L6 (measured, and it corrected S3)** — Six of the seven findings were re-checked in the live page
(`/tmp/probe2.mjs`: geometry plus computed styles, over the same seed). Result: title `13px/600` vs
author `14px/400` (D4 confirmed and worse); `.provenance` 192/192 px in a 206 px card (D5 disproved);
navbar items 135 px at x = 8/147/287 (D6 disproved); `.strip-arrow` has its own scrim and disabled
states (D1 deliberate); HomeScreen's empty slot is documented as reserved on purpose (D2 deliberate);
`.section:hover` exists (D7 half wrong); the pager's placement above the grid is the reader's own
request (D3 half wrong — only the single-page case survived).

**L7** — The capture script earned its keep before it fixed anything: the three disproved claims were
the ones read off pixels alone. Measuring in the page is the step that turns a screenshot into a
finding, and it is the difference between improving the interface and rewriting decisions the reader
already made.

**L8** — Delivery note: the branch `feat/phase-1-core` is already ~124 commits ahead of `main`, far
past the ~400-line review slice. The two commits of this feature are small and self-contained, but
the branch as a whole needs a delivery strategy (chained PRs) before it is reviewable.

**L9 (2026-11-02, parent) — the harness drifted behind the application, and the spec was looser than
the code.** A fresh `node scripts/capture-ui.mjs` exited **1** with one problem: *“the stored window
preset could not be read — capture-ui: unmocked command `get_window_preset`”*. The command was added
by `980902f` (the window's gestures) and calls the app at startup from `web/src/lib/window.ts:122`
(`crates/mirk-desktop/src/main.rs:468` is the handler), so **every capture taken since that commit**
ran the Settings → Ventana screen on its error path. The image hid it: the selected row comes from
`get_window_layout`, so the screen looked right while the preset read had failed.

Three things were wrong, and each one was measured before it was changed:

1. **The answer was missing.** `get_window_preset` is now seeded from the same value the process
   would give: the layout when it is windowed, and `columns-3` when the capture is full screen,
   mirroring `layout::preset` (`crates/mirk-desktop/src/layout.rs:122`) — full screen is never a
   preset.
2. **The default capture was not the app's default.** The script captured `columns-2` (430×844)
   while the application opens at `layout::DEFAULT_LAYOUT` = **full screen** (`layout.rs:29`), and a
   window leaving full screen restores three per row (`RESTORE_LAYOUT`). The default is now `auto`,
   so a capture without flags shows what a reader meets on first launch — the sidebar layout, not the
   narrowest grid.
3. **The README described neither the default nor the roadmap.** The capture block called two per row
   “the default”, and the roadmap still listed the tray as *Planned* and Phase 1 as untouched while
   the tray, the review queue, the filters and the editors were implemented and tested. Both now say
   what the code does, and Phase 4 names the one thing actually missing there: overriding progress by
   hand.

**RED/GREEN, on the script itself.** This harness has no unit test, so the proof is its own exit code:
a copy of the script with the one seeding line deleted exits **1** with that message (`/tmp/red-capture.mjs`,
`/tmp/red.log`); the shipped script exits **0** with `problems: []`. Both runs are in the manifest:
`auto` (1440×900, 5 captures) and `--layout columns-2` (430×844, 5 captures).

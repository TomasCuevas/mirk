# Feature: PDF Reading Progress Tracker (cross-platform, open source)

Status: **planning complete — all design decisions locked; awaiting repository bootstrap**
Created: 2026-10-09 · Revised: 2026-10-09 (scope change: personal tool -> open-source multi-platform product; decisions locked)
Engram mirrors: `odd/pdf-progress-tracker/tasks`, `odd/pdf-progress-tracker/phase-0-results`

---

## Problem

Readers accumulate PDF libraries across deep folder trees and lose track of what they started.
The system records nothing queryable about reading position, so reading state is destroyed every
time a viewer closes. Finding a half-read document again means walking the folder tree by hand.

Measured on the reference machine: **192 PDFs**, and **27 of them already hold real reading
progress that is invisible to the user** (see "Measured distribution" below).

## Scope and audience

**This is an open-source, cross-platform desktop product**, not a personal tool. That changes
the engineering constraints substantially:

| Constraint | Consequence |
|---|---|
| Multi-platform: Linux first, then Windows, then macOS | A platform abstraction layer is required from day one, not retrofitted. Every OS-specific source lives behind a trait. |
| Open source | Licensing, CONTRIBUTING, CI, packaging, i18n and stable internal contracts are deliverables, not afterthoughts. The adapter interface is a **public API** because contributors will implement it. |
| International audience | UI strings externalised from the first screen. The maintainer works in Spanish; the product cannot assume it. |
| Untrusted user environments | No assumption that Evince exists, that GNOME is the desktop, or that the user is on Ubuntu. |
| Contributors | The contributor pool is a design input. See the stack tradeoff below. |

## Solution shape (revised)

A lightweight tray-resident desktop application that indexes PDFs, records reading position, and
offers a first-party reader.

The original plan delegated rendering entirely to the user's existing viewer and read progress
back from that viewer's private state. **Research across all three platforms shows this cannot be
the durable core of a cross-platform product** — every such source is an undocumented,
version-fragile, sandbox-hostile artifact (see "Cross-platform evidence"). The revised shape:

- **First-party reader (pdf.js in the WebView) is the strategic core.** It is the only source of
  progress that is exact, live, platform-independent, and under our control.
- **External-viewer adapters are an extensibility and graceful-degradation layer**, not the
  foundation. They let existing users keep their habits and let the community add coverage
  incrementally. Each adapter is small, isolated, and failure-tolerant by contract.

## Locked decisions

| Decision | Choice | Status |
|---|---|---|
| Audience | Open source, cross-platform (Linux -> Windows -> macOS) | **Locked** |
| Strategic core | **First-party reader (pdf.js) is the core.** External-viewer adapters are a compatibility and extensibility layer, never the foundation. | **Locked** |
| License | **GPL-3.0** | **Locked** |
| Distribution | **Native packages, no sandbox**: deb / rpm / AppImage / MSI / dmg | **Locked** |
| Stack | **Tauri v2 (Rust + WebView)** | **Locked** |
| Ingestion | **Hybrid**: watched folders are the source of truth; live detection feeds a reviewable suggestion queue | Locked |
| Progress provenance | Every progress row carries `source` + `confidence` | Locked |

### Licensing implications (verified)

| Component | License | Compatible with GPL-3.0? |
|---|---|---|
| Tauri v2 | MIT / Apache-2.0 | **Yes** — both are GPLv3-compatible |
| pdf.js | Apache-2.0 | **Yes** |
| WebKitGTK (Linux WebView) | LGPL-2.1+ | Yes, as a system library |
| WebView2 (Windows WebView) | Microsoft proprietary, redistributable as a system runtime | Acceptable in the same way as a system library, but **must be documented** in third-party notices |
| macOS App Store distribution | — | **Incompatible with GPL-3.0.** Consistent with the native-packages decision; the Mac App Store is off the table permanently under this license. |

The project must ship the GPL-3.0 text, a source tarball per release, and third-party notices for
pdf.js and the WebView runtimes.

## Platform abstraction

Three independent axes, each a trait with per-OS implementations selected at compile time.

| Axis | Linux | Windows | macOS |
|---|---|---|---|
| **A. Recent-document ingestion** | `recently-used.xbel` (freedesktop spec, GTK) | `IApplicationDocumentLists` (Jump List Recent/Frequent, official API) | `~/Library/Application Support/com.apple.sharedfilelist/` (`.sfl3`) |
| **B. Live open-file detection** | `/proc/<pid>/fd` + `cmdline` | Handle enumeration, or skip in favour of axis A | `proc_pidinfo` / `PROC_PIDLISTFDS` (libproc) |
| **C. Launch / resume** | `xdg-open`, or viewer CLI (`evince -i N`) | `ShellExecute` / association | `open -a` / Launch Services |

**File identity must be abstracted too.** `(st_dev, st_ino)` works on Linux and macOS but not
Windows, which needs `(dwVolumeSerialNumber, nFileIndexHigh/Low)` from
`GetFileInformationByHandle`. Portable identity is a trait, with content hashing as the fallback
when the OS index is unavailable (network shares, FAT, exFAT).

## Cross-platform evidence (researched, not yet tested)

### Progress sources per platform

| Platform | Viewer | Source | Quality |
|---|---|---|---|
| Linux | Evince / Papers | GVFS metadata `metadata::evince::page` | **Best in class — LIVE.** Confirmed writing within 3 s of a page change while the document is open (T1). |
| Linux | Okular | doc data (per-snap or XDG path) | Documented internally, path varies by install method |
| Linux | Zathura | `~/.local/share/zathura/history` | Simple, stable |
| Windows | **SumatraPDF** | `%LOCALAPPDATA%\SumatraPDF\SumatraPDF-settings.txt`, block `FileStates[]` with `FilePath`, `PageNo`, `DisplayMode`, `ZoomVirtual` | **Best on Windows.** Open source, documented, stable schema. |
| Windows | Adobe Reader / Foxit | Registry, e.g. `HKCU\Software\Foxit Software\...\Preferences\History\LastOpen`, one subkey per file | Undocumented, version-fragile registry archaeology |
| Windows | Edge / Chrome | — | **Nothing exposed** |
| macOS | Preview | `com.apple.Preview.ViewState.plist`; `~/Library/Containers/com.apple.Preview/Data/Library/Saved Application State/com.apple.Preview.savedState`; also a `com.apple.Preview.UIstate.v1` **xattr on the PDF itself** | Requires Full Disk Access; breaks across OS versions |
| macOS | Skim / Adobe | Skim keeps its own store; Adobe exposes nothing dependable | Partial |

### Why this cannot be the foundation

1. **Version fragility is systemic, not incidental.** macOS 15 (Sequoia) moved saved-application
   state out of per-app directories into a new `talagentd` daemon container, invalidating the
   previous approach outright. This is a normal macOS release, not an exception.
2. **macOS sandboxing blocks the data.** Reading `~/Library/Containers/com.apple.Preview/...`
   requires the user to grant **Full Disk Access**. A notarised app asking for that on first run
   for a reading-progress feature is a hard sell and a review risk.
3. **Every adapter is an undocumented internals parser.** Each viewer update can break it
   silently, producing wrong percentages rather than errors — the worst failure mode for the
   exact feature the product promises.
4. **Coverage is structurally incomplete.** Browsers expose nothing on any platform. Without a
   first-party reader, the product's headline feature is unavailable for the most common way
   people open a PDF from a web page.

## Verified environment evidence (Linux reference host)

Ubuntu 24.04.5 LTS, GNOME 46, Wayland.

| Assumption | Verdict | Evidence |
|---|---|---|
| Tray icon is displayable | **Confirmed** | GNOME 46 has no native tray, but `appindicatorsupport@rgcjonas.gmail.com` and `ubuntu-appindicators@ubuntu.com` are installed. Without these the tray requirement would be dead on arrival. |
| Progress readable without a first-party viewer | **Confirmed** | Evince 46.3.1 is the default handler and writes per-document metadata into the GVFS store at `~/.local/share/gvfs-metadata/home`. Keys via `strings`: `evince::page`, `zoom`, `continuous`, `sidebar_page`, `bookmarks`, `title`, `author`, `window_*`. **45 of 192 PDFs carry `evince::page`.** |
| Resume at a saved page is possible | **Confirmed** | `evince -i, --page-index=NUMBER` opens at a page; `-p/--page-label` addresses by label. |
| Metadata sweep cost at scale | **Confirmed** | All 192 PDFs swept via `gio info` in **1929 ms (10.0 ms/file)**. Background polling is viable. |
| **Progress updates LIVE while the document is open** | **Confirmed (T1)** | Launched `evince -i 5`, then `-i 10` on the same instance. The store moved `72` (control) -> `4` -> `9`, **within 3 seconds each time, with the window still open**. The write is not deferred to close. |
| The write is flushed to disk, not buffered | **Confirmed (T1)** | After a page change the value was readable from disk immediately, and it survived `SIGTERM` with the process gone (`pgrep -c evince` = 0). Nothing is lost on an abrupt exit. |
| `evince -i N` and the stored page are **off by one** | **Confirmed 3x (T1)** | `-i 5` -> store `4`; `-i 10` -> store `9`; `-i 73` -> store `72`. **`--page-index` is 1-based; `metadata::evince::page` is a 0-based index.** |
| A cross-desktop recent-documents source exists | **Confirmed, but weaker than expected** | `~/.local/share/recently-used.xbel` exists: 712 KB, 4875 bookmarks, but only **17 `application/pdf`** entries — *fewer* than the 45 in the Evince store. The viewer's own store is the stronger PDF signal; the XBEL list is a broad, shallow complement. |
| inotify capacity | **Confirmed** | `max_user_watches=524288`, `max_user_instances=128`. |
| Toolchain | **Confirmed** | Rust/cargo 1.98.1, Node 22.20.0, Bun 1.3.11, Go 1.26.8, Python 3.12.3, uv 0.10.7. |

### Measured distribution of `evince::page` (45 of 192 documents)

| Value (0-based) | Documents | Reading |
|---|---|---|
| `0` | 18 | Opened, never advanced past page 1 |
| `1`, `4`, `7`, `9` | 8 | Early |
| `47`, `52`, `56`, `72`, `73`, `92` | 6 | Substantial progress |
| `524` | 1 | Deep into a long book |

**This is the product's business case, measured.** 27 documents on one machine already hold real
reading progress nobody can see. A first launch surfaces them before a new PDF is opened. This is
the demo, and it is real.

## Stack tradeoff (RESOLVED: Tauri v2 retained)

The first-party-reader bet changed the calculus. Rendering consistency across platforms — not
runtime efficiency — became the deciding factor.

| | Tauri v2 (Rust) | Electron (TypeScript) |
|---|---|---|
| Binary / RAM | ~10-15 MB / ~50-80 MB | ~150 MB / ~200-300 MB |
| WebView engine | WebView2 (Win), WKWebView (macOS), **WebKitGTK (Linux)** | Chromium, identical everywhere |
| pdf.js rendering | Varies by engine; WebKitGTK is the weak link | Consistent, best-in-class |
| Contributor pool | Rust — smaller, higher barrier | TypeScript — far larger, lower barrier |
| Tray | `tray-icon` crate, all platforms | `Tray` API, all platforms |
| Fit for a tray-resident app | **Excellent** | Poor (a browser engine idling in the tray) |

The tension: a tray-resident utility wants a small footprint (Tauri), but a first-party PDF reader
wants one consistent, modern rendering engine (Electron).

**Resolution: Tauri v2 retained.** The tray-resident nature of the product and the small-download
benefit for an open-source audience won. WebKitGTK on Linux is accepted as a known weak link and
becomes an explicit measurement target: pdf.js must be spiked against WebKitGTK before the reader
work is considered done. If SpiderMonkey/WebKitGTK performance proves unacceptable, the fallback is
a native rendering path (pdfium via FFI) rather than rewriting the stack — that keeps the
contingency inside one module instead of changing the whole application.

## Risks

**R1 — RESOLVED in T1, and favourably: the Evince write is LIVE, not close-time.** The store moved
from `72` to `4` to `9` within ~3 seconds each time, with the window still open, and the value
survived `SIGTERM`. The Evince adapter's capability is therefore **Live**, not `CloseTime`, and the
"percentage while reading" feature works without a first-party viewer on Linux at all.

Residual uncertainty (small): the measurement drove navigation through the Evince CLI on a single
instance, which exercises the same navigation path as user input. User-driven mouse/keyboard
navigation was **not** separately measured. The first adapter test (T9) must assert this directly.

**New hard invariant from T1 — the off-by-one.** `evince --page-index N` is **1-based** while
`metadata::evince::page` is a **0-based index**. Confirmed three times. An adapter that feeds the
CLI page straight into the store, or the reverse, silently resumes the reader one page off. This
needs a dedicated round-trip test, not a comment.

**R2 — Naive page counting is wrong.** Counting `/Type /Page` by regex returned 8/8 on a simple
PDF but undercounts on documents storing pages in compressed object streams. A real parser is
mandatory. Owned by T6.

**R3 — RESOLVED (T2).** Okular was installed as a snap but **never used** — `~/snap/okular/current/`
is empty. Non-issue on the reference host. Still relevant as a supported target for other users.

**R4 — No OS API exposes "which file another app opened".** Scanning handles is a heuristic;
thumbnail indexers, backups and antivirus open PDFs without anyone reading them. This is why live
detection feeds a reviewable suggestion queue rather than the library. Now mitigated further by
axis A (OS recent-documents lists), which is a curated signal.

**R5 — Browsers and LibreOffice expose no progress on any platform.** Manual progress override
is therefore load-bearing, and the first-party reader is the real answer. *(Severity reduced by
the revised shape.)*

**R6 — RESOLVED.** `oct` is an **independent repository** at the project root with **`main`** as the
default branch, bootstrapped in commit `66f299a`. A git repository does exist at
`/home/tomas-cuevas` (branch `master`, 0 commits) and still has `oct` as an untracked
directory nested inside it; that repository is unrelated and is left untouched. Nothing in this
project runs git against it. If it is ever committed, `oct` would appear as an embedded
repository — the honest long-term fix is to remove or relocate that accidental home-level
repository, which is outside this project's scope.

**R7 — Path encoding is a correctness requirement.** The reference corpus has names like
`Comparación de Métodos de Strings entre JavaScript y Python.pdf`. Unquoted shell iteration over
`find` silently split these and produced a wrong Phase 0 measurement (`16` documents instead of
`45`). Portable handling must treat paths as opaque UTF-8 everywhere, and:
- Linux/GVFS keys by **percent-encoded `file://` URIs** — encode/decode fidelity matters.
- Windows paths are **UTF-16** and case-insensitive.
- macOS normalises to **NFD**, so `Comparación` differs in bytes from the NFC form produced by
  Linux and Windows. Comparing or hashing raw paths across platforms will silently mismatch.
  Normalise explicitly at the boundary.

**R8 — RESOLVED by the packaging decision.** Native packages (deb/rpm/AppImage, MSI, dmg) were
chosen over Flatpak/Snap, so adapters keep full filesystem access. Sandboxed distribution stays
available as a possible later variant, but it will ship with declared reduced capabilities. The
conflict is now a documented product boundary rather than an unresolved tension.

**R11 — GPL-3.0 permanently excludes the Mac App Store.** Accepted and consistent with the native
packaging choice. macOS distribution will be a notarised `.dmg`, which requires the paid Apple
Developer Program (R9). Without notarisation, Gatekeeper will warn on every launch.

**R12 — RESOLVED in the R12 spike: WebKitGTK is NOT the weak link for render throughput.**

Measured with a purpose-built harness at `spike/r12/` (pdf.js 6.4.299 vendored via `npm pack`,
served over localhost HTTP, both engines in real windowed mode, sequential runs, 4 browser
launches) against two of the user's real PDFs.

| Metric — 136 MB PDF, **436 pages** | WebKitGTK 2.52.6 | Chrome 155 |
|---|---|---|
| Worker | dedicated `type:module` | dedicated `type:module` |
| WebAssembly (qcms) | available, instantiated | same |
| JS errors / console warnings | 0 / 0 | 0 / 0 |
| First page render only | **2827 ms** | 4004 ms |
| First page visible incl. fetch (136 MB over localhost) | **4953 ms** | 7662 ms |
| Sequential pages 1-20, mean | 827.6 ms | 968.6 ms |
| Sequential pages 1-20, median | 167 ms | 139.6 ms |
| Throughput | **1.21 pages/s** | 1.03 pages/s |
| Random access to page 218 (N/2) | 165 ms | 140.5 ms |

Second PDF (61 MB, **708 pages**, WebKitGTK only): first page 1086 ms, mean 116.3 ms/page,
**8.6 pages/s**, no page above 173 ms, zero errors.

**The decisive finding: the cost is content-bound, not engine-bound.** Pages 1, 2, 5 and 6 of the
136 MB PDF each contain a single 7650x9900 px image (~76 megapixels, per `pdfimages -list`) and
cost 2.7-5.4 s on **both** engines; every other page costs 13-283 ms. The two engines converge
because the work is pdf.js-side image decode and scale, not engine rendering. Choosing the engine
does not determine performance; **handling pathological pages does.**

**Residual gaps — R12 is downgraded, not retired.**
1. **MiniBrowser is the same engine but NOT Tauri's webview configuration.** Tauri v2 sets its own
   `WebKitSettings`, and on Linux its `hardwareAccelerationPolicy`/DMABUF/compositing setup
   differs. Every number above is "the engine can do this", not "the Tauri app will do this".
2. **Continuous scrolling, text layer and annotation layer were never measured.** These are the API
   path only (`getDocument` + `page.render`). Continuous scroll is the core reading interaction of
   this product and carries its own cost.
3. **No streaming.** The harness passed a buffered `ArrayBuffer`; the server recorded
   `rangedRequests: 0`. A real reader would pass `url:` and let pdf.js range-stream, which changes
   first-page latency materially for a 136 MB file.
4. **One measured run per engine, with ~1.8x spread on first-page latency** (4987 ms vs 2827 ms on
   the same engine). Treat single-run first-page figures as ±30 %, not as benchmarks.
5. `hardwareConcurrency` was reported as 8 by WebKitGTK and 12 by Chrome — a real difference that
   affects pdf.js worker pooling.

**New performance lead (not yet established).** pdf.js's stock render path makes the engine create
its 2D context with `willReadFrequently: true`, which keeps the canvas on the software path. A
harness-owned context with `willReadFrequently: false` measured 19 % faster on WebKitGTK and 25 % on
Chrome — **but that second pass also had warm caches, so the effect is confounded and must not be
quoted as measured.** It needs a clean A/B. If real, it is a cheap, engine-independent win.

**Conclusion**: keep Tauri and pdf.js. Do not build the pdfium FFI path. Keep it as a documented
escape hatch behind one module, and revisit only if the in-Tauri re-verification fails.

**R9 — macOS distribution has a real cost.** Notarisation requires the paid Apple Developer
Program. Without it, Gatekeeper blocks or warns on every launch. This is a budget and identity
decision, not a technical one.

**R10 — Adapter maintenance is unbounded work.** Each external viewer is a separate moving target.
Treating adapters as a public, community-extensible interface converts this liability into an
asset — but only if the interface is stable and documented from the start.

## Architecture

### Progress provider registry (public interface)

Contributors implement this. It is a published API, and versioned as such.

```
trait ProgressProvider {
    fn id(&self) -> &'static str;                     // "sumatrapdf-settings"
    fn platform(&self) -> Platform;                   // Linux | Windows | MacOS
    fn viewer_ids(&self) -> &'static [&'static str];  // exe names / bundle ids / desktop ids
    fn capability(&self) -> ProgressCapability;       // Live | CloseTime | None
    fn detect(&self) -> Availability;                 // Installed | Missing | Unsupported
    fn read(&self, doc: &Document) -> Result<Option<StoredProgress>, AdapterError>;
    fn resume(&self, doc: &Document, page: u32) -> Result<LaunchOutcome, AdapterError>;
}
```

Contract requirements: an adapter must never panic, never block the UI thread, must degrade to
`None` on unrecognised input rather than guessing, and must declare its capability honestly so the
UI can warn before opening.

| Viewer | Platform | Source | Capability |
|---|---|---|---|
| First-party reader | all | local store | **Live** (exact) |
| Evince / Papers | Linux | GVFS metadata | **Live** — proven in T1 (write lands within ~3 s of a page change, survives abrupt exit) |
| Zathura | Linux | history file | CloseTime |
| SumatraPDF | Windows | `SumatraPDF-settings.txt` `FileStates[]` | CloseTime (documented) |
| Okular | Linux | doc data (path varies) | CloseTime |
| Preview | macOS | ViewState plist / container savedState | CloseTime, needs Full Disk Access |
| Adobe / Foxit | Win/macOS | registry / — | CloseTime, undocumented |
| Firefox / Chrome / Edge / LibreOffice | all | — | **None** — record the open, never a percentage |

### Data model

```sql
documents(
  id, path, path_normalized, uri,
  volume_id, file_index,        -- portable identity (dev+ino on POSIX, serial+index on Windows)
  size, mtime, sha1,
  title, author, pages, pages_source,
  discovered_via,               -- 'watched' | 'recent' | 'live' | 'native'
  platform,
  first_seen_at, last_opened_at, status
)
progress(
  id, doc_id, page, total,
  source,                       -- 'native' | 'evince' | 'sumatrapdf' | 'manual' | ...
  confidence,                   -- 'exact' | 'close-time' | 'manual' | 'unknown'
  updated_at
)
open_events(id, doc_id, pid, viewer_id, started_at, ended_at)
viewers(id, exe, desktop_id, bundle_id, platform, capability, detail)
folders(id, path, recursive, enabled, added_at)
suggestions(id, path, seen_count, first_seen_at, decided_at)
```

**`source` and `confidence` are load-bearing.** Progress from the first-party reader, progress read
at Evince close time, and progress typed by hand are **not the same truth**. Collapsing them into
one column yields a UI that silently lies — a precise-looking 40% that corresponds to nothing.
Provenance lets the UI be honest: *"p. 12 of 80, recorded when Evince closed"*.

## Roadmap (revised)

**Phase 0 — Validation spike. COMPLETE.** T1 (Evince flush timing), T2 (Okular), T3 (metadata
sweep at scale) and the R12 rendering spike are all done. R1, R3 and R12 are resolved.
**There is no remaining project-wide unknown that can invalidate the plan.** What is left are
contained, testable follow-ups: re-verifying pdf.js inside a real Tauri webview, and the
content-bound render cost.
**Phase 1 — Rust core.** Tauri scaffold, SQLite, portable file identity, scanner with real page
counting, watcher, recent-documents ingestion (axis A), provider registry.
**Phase 2 — First-party reader.** pdf.js integration, live progress, resume. **Promoted from
"future" to a near-term pillar.**
**Phase 3 — Tray.** Icon, quick menu, popup.
**Phase 4 — UI.** Library, filters, capabilities screen, manual override, suggestion queue.
**Phase 5 — Ecosystem.** Adapters for SumatraPDF, Zathura; contributor docs; packaging.

### Existing tasks

- [x] **T2** Validate snap Okular docdata path and sandbox access — done, non-issue
- [x] **T3** Benchmark GVFS metadata reads across 192 PDFs — done, 10.0 ms/file
- [ ] **T1** Measure Evince GVFS page-write semantics (live vs close) — **open**
- [ ] **T0** Resolve git repository layout — **blocker**
- [ ] **T4** Scaffold Tauri v2 workspace and crate layout — needs scope revision
- [ ] **T5** SQLite schema and migrations — needs portable-identity revision
- [ ] **T6** Watched-folder scanner with real page counting via `lopdf`
- [ ] **T7** inotify watcher over watched folders (Linux; per-OS backend later)
- [ ] **T8** `/proc/*/fd` open-document detector (Linux backend of axis B)
- [ ] **T9** Progress provider registry plus Evince GVFS adapter
- [ ] **T10-T16** Tray, UI, manual override, capabilities screen, suggestion queue

New tasks implied by the scope change: portable file identity, recent-documents ingestion per
platform, first-party reader, i18n scaffolding, license + CONTRIBUTING + CI, packaging matrix.

## Open decisions

**Nothing blocking.** Both blocking decisions were made by the user:

- **Product name**: **`oct`**.
- **Git layout**: an independent repository at the project root, default branch **`main`**.
  Bootstrapped in commit `66f299a`; Phase 1 work proceeds on `feat/phase-1-core`.

**Non-blocking, deferred**: the **frontend framework**. The T4 scaffold deliberately uses plain
TypeScript + Vite with no framework, so nothing is locked in. React / Svelte / Solid is a real
choice with long-term consequences — the contributor pool matters for an open-source project — and
it must be settled before the UI phase (T12 onward), not during a scaffold task.

## Task log / evidence

| Task | Status | Commit | Evidence |
|---|---|---|---|
| — | Planning complete (v1) | — | Superseded by the scope change |
| T2 | **Done** | — (blocked by R6) | `~/snap/okular/current/` empty; Okular installed but never used |
| T3 | **Done** | — (blocked by R6) | 192 PDFs in 1929 ms (10.0 ms/file); 45 carry `evince::page` |
| T1 | **Done** | — (blocked by R6) | Evince writes **live** (store moved 72 -> 4 -> 9 within 3 s, window open). Survives SIGTERM. **`-i` is 1-based, stored page is 0-based.** Side effect disclosed and reverted: the test overwrote `Cosmos - Carl Sagan.pdf` (page 72) and it was restored to exactly 72 via `-i 73`. |
| — | Re-planning | — | Scope changed to open-source multi-platform; decisions reopened |
| — | **Decisions locked** | — | First-party reader as core; native unsandboxed packages; Tauri v2 retained; GPL-3.0 |
| — | **R12 spike done** | — | WebKitGTK 2.52.6 renders pdf.js 6.4.299 at rough parity with Chrome 155 on a real 136 MB / 436-page PDF (1.21 vs 1.03 pages/s). Cost is content-bound, not engine-bound. Artifacts: `spike/r12/` (`FINDINGS.md`, `results.json`, `results.raw.jsonl`, 4 runs). |

**Independently verified by the parent**: `pdfinfo` confirms 436 and 708 pages; the PDFs in
`public/pdf/` are symlinks, not copies (nothing of the user's was modified); zero MiniBrowser and
zero spike-Chrome processes remain; no listener left on the spike port. The worker's own report of
"no processes left" was accurate — an initial parent check using `pgrep -f MiniBrowser` returned a
false positive by matching its own command line, the same self-contamination class as R7.

**Remaining contained follow-ups**: in-Tauri webview re-verification (continuous scroll, text
layer, range streaming), content-bound render cost, and the unproven `willReadFrequently` lead.

| Task | Status | Commit | Evidence |
|---|---|---|---|
| T0 | **Done** | `66f299a` | Independent repository, default branch `main`, GPL-3.0, README, `.gitignore`. PDF symlinks and vendored pdf.js correctly excluded (`git status --ignored` confirmed). |
| — | Phase 1 started | — | Working branch `feat/phase-1-core` created from `main`. |
| T4 | **Done** | `b34ec4f` | Cargo workspace, 4 crates, 1391 lines of Rust across 39 tracked files. `cargo build --workspace` and 25 tests pass, clippy `--all-targets -D warnings` clean, frontend builds, app launches and exits with no process left. See "Phase 1 — what T4 established" below. |

## Phase 1 — what T4 established

The platform abstraction is now compiler-checked rather than documented. Five traits in
`oct-core` are the public, contributor-facing interface, and all five are asserted object-safe and
`Send + Sync` by an integration test, because the provider registry stores
`Box<dyn ProgressProvider>` and the scan runs off the interface thread:

| Trait | Axis | On Linux | Task |
| --- | --- | --- | --- |
| `RecentDocumentsSource` | A — what the user opened | `recently-used.xbel` | T12 |
| `OpenDocumentDetector` | B — what is open now | `/proc/<pid>/fd` | T8 |
| `Launcher` | C — open at a page | `xdg-open` / viewer CLI | T7 |
| `FileIdentity` | portable identity | `st_dev` + `st_ino` | T11 |
| `ProgressProvider` | reading position | GVFS metadata | T9 |

Three things landed as enforced invariants rather than comments:

1. **Honest stubs.** An unimplemented backend returns `OctError::NotSupported`, never `Ok(vec![])`.
   The difference matters: the second is indistinguishable from "I looked and there are none", so
   a missing capability would render as an empty library. A test asserts this for every stub.
2. **Zero-based pages, everywhere in the domain.** `evince --page-index` is one-based while its
   stored metadata is zero-based; the conversion belongs at the adapter boundary, and the type
   documents it so nobody has to rediscover the off-by-one.
3. **Percent-encoded URIs are tested, not assumed.** GVFS keys its per-document state by URI, so
   `%C3%B3` versus `%F3` decides whether a lookup finds anything. `file_uri_from_path` is covered
   by seven tests including the multi-byte escape and the URI-delimiter cases. It does **not**
   normalise Unicode — macOS stores NFD while Linux stores NFC, and normalising would break the
   lookup on whichever platform actually holds the file (R7).

**Deferred, not skipped**: `oct-platform` returns empty from `progress_providers()` until T9, and
`oct-store` is a documented placeholder until T5. Both say so in code.

**Process note**: T4 was implemented inline rather than delegated. Subagent delegation broke
mid-session — creating the repository changed the session's Git identity, and the delegation layer
now refuses with *"Writer session Git authority changed before admission"* while
`session_worktree_register` reports *"Session Git identity changed; cannot register a worktree"*.
A Pi session restart is required to restore delegation. Until then, remaining Phase 1 tasks are
implemented inline, which is workable but far more context-expensive, so they should be kept small
and committed one work unit at a time.

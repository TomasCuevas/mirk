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
| **pdfium** (native page counting, T6) | BSD-3-Clause | **Yes** |
| **inotify** (Linux folder watching, T7) | **ISC** — not MIT | **Yes** |
| **rusqlite** + bundled SQLite (T5) | MIT / public domain | **Yes** |
| WebKitGTK (Linux WebView) | LGPL-2.1+ | Yes, as a system library |
| WebView2 (Windows WebView) | Microsoft proprietary, redistributable as a system runtime | Acceptable in the same way as a system library, but **must be documented** in third-party notices |
| macOS App Store distribution | — | **Incompatible with GPL-3.0.** Consistent with the native-packages decision; the Mac App Store is off the table permanently under this license. |

The project must ship the GPL-3.0 text, a source tarball per release, and third-party notices for
pdf.js, **pdfium**, the WebView runtimes, and the Rust dependencies above. Licences are recorded here
because they are a packaging deliverable, and a licence recorded wrong is worse than one not recorded
at all: `inotify` was assumed MIT in a task brief and is in fact **ISC**.

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
  file_id_kind,                 -- 'posix' | 'windows' | 'content-hash' | NULL (no identity known)
  volume_id, file_index,        -- decimal TEXT: the full u64 range must survive SQLite's INTEGER
  content_hash,                 -- FileId::ContentHash verbatim; the algorithm lives in the value
  size, mtime,
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

**Reconciled with T5's implementation (2026-10-12).** Three details were only visible once the
schema was written, and all three are plan changes rather than implementation drift:

1. **`file_id_kind` added.** `volume_id` / `file_index` alone cannot tell a POSIX `(dev, ino)` pair
   from a Windows `(serial, index)` pair, and neither can carry `ContentHash`. Without a
   discriminator the three `FileId` variants are not losslessly storable, so S3 would fail.
2. **Numeric identity stored as decimal TEXT.** `u64` exceeds SQLite's signed 64-bit `INTEGER`;
   `u64::MAX` must round-trip, and a truncating cast would silently merge two documents.
3. **`sha1` renamed `content_hash`.** T11's fallback hashes with **SHA-256**, so a column named
   `sha1` would have been a permanent lie in a name that cannot be fixed once data exists. The
   algorithm stays inside the value.

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

### Task list (reconciled 2026-10-12)

The list below is the current record. Commit hashes and measurements live in the evidence tables
further down; this block is what a worker reads to know what is done and what is active.

- [x] **T0** Resolve git repository layout — done (`66f299a`): independent repository, default branch `main`
- [x] **T1** Measure Evince GVFS page-write semantics — done: the write is **Live**; off-by-one confirmed 3x
- [x] **T2** Validate snap Okular docdata path and sandbox access — done, non-issue
- [x] **T3** Benchmark GVFS metadata reads across 192 PDFs — done, 10.0 ms/file
- [x] **T4** Scaffold Tauri v2 workspace and platform abstraction — done (`b34ec4f`)
- [x] **T11** Portable file identity (`FileIdentity`) — done (`2b8e9da`)
- [x] **T5** SQLite schema, migrations and the queries behind them — done (`c2a059d`); specs below
- [x] **T6** Watched-folder scanner with real page counting via `pdfium` — done (`a5b3ea5`); resolves R2; specs below
- [x] **T7** inotify watcher over watched folders (Linux; per-OS backend later) — done (T7a `8355b2d`, T7b `41e11c6`); specs below
- [x] **T8** `/proc/*/fd` open-document detector — done (`bfcd234`); Linux backend of axis B
- [x] **UI slice** the first vertical slice over the store — done (`59f542c`); pulled ahead of T12 and the tray (L13, L15)
- [ ] **T12** `RecentDocumentsSource` for `recently-used.xbel` — Linux backend of axis A
- [x] **T9** Progress provider registry plus Evince GVFS adapter — done (`9bc6d9f`); the first real `ProgressProvider` (`Live`), and the first caller of `Launcher`
- [ ] **T10, T13–T16** Tray, library and UI, manual override, capabilities screen, suggestion queue

Still unnumbered, implied by the scope change: first-party reader (pdf.js), i18n scaffolding,
license + CONTRIBUTING + CI, packaging matrix, in-Tauri webview re-verification.

**Numbering note.** An earlier draft gave axis C (`Launcher`) the label `T7` in the trait table and
reused `T12` for both recent documents and the UI phase. The authoritative order is the one
recorded with T4 — `T5 → T6 → T7 (inotify) → T8 (/proc) → T9 (providers) → T12 (recent documents)` —
and `Launcher` has no dedicated number: it lands together with `T9`, whose Evince adapter is the
first thing that needs to reopen a document at a page.

## Specs — T5 (SQLite schema, migrations, queries)

Acceptance criteria. They are **not new product requirements**: each one restates a decision locked
earlier in this document, so a worker does not have to reinterpret it. Line references are to this
file's own "Data model" section, the `oct-store` crate documentation, and the T11 record.

- **S1 — The six tables, by name.** `documents`, `progress`, `open_events`, `viewers`, `folders`,
  `suggestions`, with the columns given in "Data model". No table may be silently dropped or renamed.
- **S2 — Provenance survives storage.** `progress` keeps `source` and `confidence` as **separate
  columns**; they are never collapsed into one value or inferred from each other. Two rows with the
  same page but different `source`/`confidence` must be distinguishable after a round trip.
- **S3 — Portable identity, never only a path.** `documents` persists `oct_core::FileId` in all three
  variants — `Posix { dev, ino }`, `Windows { volume_serial, file_index }`, `ContentHash(String)` —
  and can find a document again by identity alone.
- **S4 — Pages are zero-based in storage.** `progress.page` matches `StoredProgress::page`
  (0-based). The 1-based `evince --page-index` conversion stays at the adapter boundary and must
  **not** leak into the schema.
- **S5 — The database lives in the user data directory.** `$XDG_DATA_HOME/oct` on Linux,
  `%APPDATA%` on Windows, `~/Library/Application Support` on macOS — never the repository, never the
  current working directory.
- **S6 — Migrations are versioned, forward-only and idempotent.** Opening a database brings it to
  the current schema version and records that version inside the database; opening an already
  current database changes nothing and succeeds; a fresh file is created at the current version.
- **S7 — Paths are opaque UTF-8 and are not Unicode-normalised.** The GVFS lookup key is a
  percent-encoded `file://` URI, so NFC/NFD must survive a round trip byte-for-byte (risk R7).
- **S8 — Honest failures.** Nothing returns an empty result to mean "unsupported"; failures surface
  as `oct_core::OctError`, consistent with the honest-stub invariant established in T4.

## Specs — T6 (watched-folder scanner)

Same rule as T5: acceptance criteria that restate decisions already locked in this document, so a
worker does not reinterpret them.

- **S1 — Discovery.** A recursive walk of one watched folder. PDFs are recognised by extension
  (case-insensitive `.pdf`). Paths are treated as **opaque UTF-8** and are never shell-split or
  Unicode-normalised (R7). The watched root is canonicalised, and symlinks encountered *inside* it are
  not followed — a symlink loop must not hang the scan.
- **S2 — Real page counting (this is R2).** Page counts come from a real PDF parser (`pdfium`), never
  from a byte scan, a regex, or counting `/Type /Page`. Proven against a fixture whose page objects
  live inside a **FlateDecode-compressed object stream**: the test asserts the correct count *and*
  that the naive scan is wrong on that same fixture, so the fixture cannot quietly become vacuous.
- **S3 — Metadata.** `title` and `author` come from the document's Info dictionary when present, and
  are `None` when absent. Never invented, never taken from the file name.
- **S4 — Honest failures.** An unreadable, corrupt or encrypted PDF must not panic and must not abort
  the scan. It appears in the report as a failure with its path. Its stored `pages` is `None` when no
  count was ever known, and **never `0`** — `0` is indistinguishable from a real count and would render
  as "0% read". A count established earlier is **preserved** rather than erased (T5's "unknown never
  erases known"), and the failure entry is what tells the caller the count was *not* re-verified this
  time; `pages_source` and `status` are the eventual place to record that distinction, and both are
  deferred. This wording was corrected during T6's verification: the earlier text said `pages` simply
  "stays `None`", which is only true of a document that never had a count.
- **S5 — Filesystem robustness.** A permission-denied directory, a symlink loop, and a file that
  disappears mid-scan must not panic or hang. The scan finishes, reports unreadable paths as failures,
  and reports **deliberately skipped** paths (symlinks inside the root are never followed) in their own
  `skipped` list — a skip is not a failure, but it must not be invisible either.
- **S6 — Persistence and idempotence.** Counted documents are persisted through
  `Store::upsert_document`, with identity from the injected `FileIdentity` (content-hash fallback), so
  scanning the same folder twice leaves the same rows and the same counts — no duplicates. The watched
  folder is recorded in `folders`. The scan is **read-only** over the PDFs.
- **S7 — Layering.** `oct-scan` receives the identity provider and the store by injection and depends
  only on `oct-core`, `oct-store` and the PDF parser — **not** on `oct-platform`, so a fake identity
  provider can drive the tests and the crate stays testable without a filesystem backend.

**Deliberately deferred, with reasons** (they are separate work units, not omissions): size/mtime-based
incremental rescan and the metadata columns it needs (`size`, `mtime`, `discovered_via`, `platform`,
`status` — T5 left them unwritten because nothing filled them); the live watcher (T7); the suggestion
queue for PDFs found outside watched folders; and scanning a folder the user has not nominated.

## Specs — T7 (watched-folder watcher)

Same rule as T5 and T6: acceptance criteria that restate decisions already locked here, so a worker
does not reinterpret them. T7 is split into two work units on purpose — **T7a the mechanism** (this
spec) and **T7b the wiring into the library** (specified when T7a lands), because one delegation that
does both is a review nobody can hold in their head.

- **S1 — A sixth trait, following the established pattern.** `FolderWatcher` lives in `oct-core` with
  `id()` and `platform()`, plus the value types it speaks: `WatchTarget { path, recursive }`,
  `WatchEvent { path, kind }` and `WatchEventKind { Added, Changed, Removed }`. It must be object-safe
  and `Send + Sync` like the other five, and the existing `object_safety` test must cover it.
- **S2 — A Linux backend and an honest stub.** `oct-platform` implements it with **inotify** behind
  `#[cfg(target_os = "linux")]`, and `unsupported.rs` answers `NotSupported` everywhere else — never an
  empty success. It is wired into `Backends` alongside the other backends. Recursive targets are
  watched by watching each directory, and **subdirectories that appear later are watched too**.
- **S3 — A file is reported when it is *settled*, not while it is being written.** A half-copied PDF
  must never be scanned: `Changed`/`Added` are emitted when the write **completes** (`IN_CLOSE_WRITE`)
  or when the file arrives already complete (`IN_MOVED_TO`), never on `IN_MODIFY`. Bursts are
  **coalesced per path** within a short window, so a copy of one large PDF produces one event rather
  than hundreds. `IN_CREATE` is observed but deliberately **not** emitted: reporting there would
  announce a file its writer still holds open, and the trade-off is that a file created and never
  written or closed is not reported at all. *(Corrected during T7a: the first wording listed
  `IN_CREATE` as a settle trigger, which contradicts the rule two clauses earlier.)*
- **S4 — Paths stay opaque (R7).** Events carry `PathBuf`, never `String`; nothing is split, trimmed,
  shell-quoted or Unicode-normalised on the way through.
- **S5 — Stopping and resource honesty.** The loop runs until the caller sets a stop flag, never
  panics, and never spins. A kernel limit failure (watch descriptors exhausted) surfaces as **one
  honest error naming the limit**, never a silently partial watch; a directory that disappears
  mid-watch is reported and does not end the loop.
- **S6 — No runtime assumptions.** The loop is blocking and belongs on the caller's own thread. The
  workspace has no async runtime and T7 does not introduce one.
- **S7 — Layering.** Trait and types in `oct-core` (still dependency-free), the Linux implementation
  in `oct-platform`, and `oct-platform` remains the only crate containing OS-specific code.

**What a removal means is deliberately *not* decided here, and this is the decision to record:** the
watcher reports `Removed`, and the library is **not** changed by it. Deleting the row would destroy
the reading history that is the entire point of the product, and representing "the file is gone" needs
`documents.status`, which this document already defers along with the other scanner metadata columns.
So T7b wires added/changed files into the library and leaves removals as a reported event, and
"the file is gone but your progress is not" becomes its own task with its own migration.

## Specs — T7b (wiring watcher events into the library)

Written once T7a landed, so the mechanism's contract was real rather than imagined. Same rule as
every other spec here: acceptance criteria that restate decisions already locked in this document.

- **S1 — Injected, never coupled.** The wiring lives in `oct-scan` and receives the `FolderWatcher`
  and the `Scanner` by injection, so `oct-scan` still does not depend on `oct-platform` (T6's S7). No
  async runtime, no new dependency, and no change to `oct-core`, `oct-platform` or `oct-store`.
- **S2 — `Added`/`Changed` reach the library through T6's path.** The document is counted and
  persisted with `count_and_persist`, so a PDF that appears in a watched folder shows up with a real
  page count. T6's failure rules apply unchanged: an unreadable, corrupt or encrypted file is reported
  and keeps `pages = None`, never `0`.
- **S3 — `Removed` does **not** change the library.** It is reported to the caller and nothing else
  happens. Deleting the row would destroy the reading history that is the whole point of the product,
  and representing absence needs `documents.status`, which is deferred — the decision recorded in L7.
- **S4 — The watcher's own blind spots get the right response.** `Lost` means events were dropped, so
  the reported path is **rescanned**; that is the recovery. `Unwatchable` is reported and **not**
  rescanned, because rescanning cannot fix a path that cannot be watched.
- **S5 — Nothing is swallowed.** Every outcome — counted, failed, removed, lost, unwatchable — reaches
  one sink the caller supplies, so the application can log it or tell the user. The wiring never panics
  and never blocks beyond the call itself.
- **S6 — The caller's order is scan first, then watch**, and the spec says so because T7a cannot watch
  a change that happens before its watches are installed. The wiring itself only reacts to changes; the
  initial scan is the caller's job with `Scanner::scan`.

**Deliberately not built**: any batching layer. Events already arrive coalesced per path, and pdfium
counts a document in about a millisecond (T6's measured 1.5 s for a 42-document library), so
per-event processing is not a bottleneck worth a second buffering window. Also deferred: the UI that
shows these outcomes, and `documents.status` for representing absence.

## Specs — T8 (`/proc` open-document detector, axis B)

Same rule as every other spec here. This axis is the plan's acknowledged **heuristic** (R4), and these
criteria keep it honest about that rather than pretending it is exact.

- **S1 — A real snapshot, not a stub.** `ProcOpenDocuments::open_documents()` walks `/proc/<pid>/fd` for
every process, resolves each descriptor's target, keeps targets that are regular files whose name ends
in `.pdf` (case-insensitive), and returns `OpenDocument { path, pid: Some(pid), viewer }`. On Linux it
no longer returns `NotSupported`.
- **S2 — The viewer is the process's real name.** Derived from the basename of the first argument in
`/proc/<pid>/cmdline`, falling back to `/proc/<pid>/comm`, and `None` when neither is readable. Never
invented, never the pid rendered as a string.
- **S3 — Noise is reported, not hidden (R4).** The detector reports what is *open*, including files
held by indexers, backups and antivirus, and it does not pretend to know that a process is *reading*:
that judgement belongs to the review queue this axis feeds. Two noise sources are documented rather
than filtered — the detector's own process, which may legitimately hold a PDF (the first-party reader
will), and the scanner's own transient opens while it counts pages.
- **S4 — A deleted file is not an open document.** A descriptor whose target ends in ` (deleted)`, or
whose target cannot be resolved, is skipped: reporting it would create a suggestion pointing at a path
that no longer exists. This is a deliberate skip, recorded here because the trait has no channel to
report it through — see S5.
- **S5 — Best effort is the contract, and it is written down.** A process whose `/proc/<pid>` cannot be
listed (another user's, `EACCES`) or that exits mid-scan is skipped without failing the call and
without panicking. The trait has no per-process failure channel, so the honest statement is that this
axis is a best-effort snapshot whose narrowness and noise are exactly why it feeds a reviewable queue
instead of the library (R4) — not a defect to be silently patched later.
- **S6 — Deterministic and deduplicated.** One entry per `(path, pid)`, deduplicated and sorted by path
then pid, so two consecutive calls on an unchanged system return identical vectors and a caller can
diff snapshots.
- **S7 — Layering, and no new dependency.** The implementation lives in `oct-platform` behind
`#[cfg(target_os = "linux")]`, the non-Linux stub keeps returning `NotSupported` rather than an empty
success, and `oct-core` stays dependency-free. `/proc` is read with `std` alone: a few `read_dir` and
`read_link` calls do not justify a `procfs` dependency in a project that already ships a native
library and a SQLite build.

## Specs — T9 (progress provider registry + the Evince GVFS adapter)

Same rule as every other spec here. This is the task that makes the product's headline claim real: the
plan measured that **26 of the maintainer's 42 PDFs already hold reading progress nobody can see**, and
this is what surfaces it.

- **S1 — A real provider, with a stable identity.** The adapter's `id()` is `evince-gvfs` — the value
already used as the example in the trait docs and asserted in T5's tests, so it is frozen: stored
progress rows reference it. `platform()` is Linux; `viewer_ids()` names what it claims (`evince`,
`org.gnome.Evince`, `papers`, `org.gnome.Papers`); `capability()` is **`Live`**, proven in T1.
`detect()` reports `Missing` when the viewer **or** `gio` is absent, because without either the provider
cannot work at all.
- **S2 — `read` returns the stored position, zero-based, with its provenance.**
`metadata::evince::page` is a **0-based** index and is stored as such; `total` comes from the document's
page count when known; `source` is `External("evince-gvfs")` and `confidence` is `Exact`, because T1
proved the write lands live and survives an abrupt exit. No record is `Ok(None)` — a normal answer, not
an error — and state that exists but cannot be understood is also `Ok(None)`, per the trait's own
contract rule. Never a guess, never a zero standing in for "unknown".
- **S3 — The off-by-one is converted exactly once, at the CLI boundary.** `resume(doc, page)` passes the
**1-based** index to `evince -i` (`page + 1`), because T1 confirmed three times that `--page-index` is
1-based while the stored value is 0-based. This gets a **dedicated round-trip test, not a comment**:
read a position, resume from it, and assert the CLI was handed `stored + 1`.
- **S4 — Read through `gio`, never by parsing a private binary format.** The GVFS store is queried with
`gio info -a metadata::evince::page`, the mechanism T3 measured (re-measured here: **8.7 ms/file**, the
whole 42-document library in **0.37 s**). The metadata file itself is a log-structured format that is
undocumented and version-fragile, and parsing it would be exactly the adapter fragility this project
treats as a liability (R10). The per-document cost is inherent to the trait's shape (`read(&self, doc)`);
a bulk path would need a bulk trait method, which the plan does not have.
- **S5 — `resume` goes through the Linux `Launcher` (axis C), implemented here because this adapter is
its first caller.** `launch(path, None)` opens with the system default (`xdg-open`);
`launch(path, Some(page))` uses a viewer CLI that accepts a page, and `LaunchOutcome` names whichever
program actually ran. **A missing page-capable viewer is a degraded success, not an error**: the
document is still opened with the default handler, because `Launcher`'s own documented contract says
landing on page one is better than refusing to open anything. An error is reserved for the case where
*nothing* can open the document at all. *(Corrected during T9: this clause first said a missing viewer
is an error, which contradicted the trait's documented contract — the worker followed the spec and
flagged the conflict instead of silently picking one.)* Known limitation, recorded: `LaunchOutcome`
cannot yet say "opened, but not at the page you asked for"; the viewer name is the only signal, and
adding a variant is cheap whenever a caller needs to tell the user their viewer cannot be asked for a
page.
- **S6 — Registered, and the registry's existing invariant still holds.** `oct-platform::progress_providers()`
returns the adapter on Linux — it returned an empty vector until T9 — and the test that asserts
provider ids are unique and trackable keeps passing, because it was written for exactly this moment.
- **S7 — Layering and honesty.** The adapter lives in `oct-platform` behind `#[cfg(target_os = "linux")]`,
`oct-core` stays dependency-free, **no new dependency** (a subprocess and `std` are enough), nothing
panics, and no call blocks longer than one `gio` invocation.

**Deferred on purpose**: Zathura, Okular and SumatraPDF adapters (separate viewers, separate tasks), and
any caching layer over `gio` — the trait's per-document shape is what the plan locked, and a cache would
be a workaround for a cost that is already acceptable at library scale.


## Specs — UI slice (the first vertical slice)

Written because the user wants to judge the architecture by using it, and because the store has no
library query at all — T5 deferred listing and filtering explicitly, "because nothing called them
yet". The caller now exists, which is the whole reason this slice goes before T12 and the tray.

- **S1 — The store gains the listing it never had.** A query returning documents with their **latest
  progress and its provenance** — page, total, source, confidence, when it was recorded — ordered
  deterministically. This is the contract T5 deliberately deferred; it is now written by its first
  caller rather than guessed in advance.
- **S2 — Three Tauri commands, and nothing speculative**: list the library, scan a folder and return
  the report, and report the progress-provider registry (id, capability, availability). Each one
  exists because a pixel of the screen needs it.
- **S3 — One screen, over the existing `web/` scaffold, in Svelte 5**: the library with its page
  counts and reading progress, where the progress is **stated with its provenance in words** — "p. 12
  of 80, recorded by Evince" — and never as a bare percentage. That sentence is the product; a
  precise-looking 40% that means "the file was closed four months ago" is the lie the provenance
  columns exist to prevent.
- **S4 — Honest states, no invented data.** An empty library says it is empty; a missing provider says
  which capability is missing and why; a scan failure shows the failure. The screen must never render
  a fabricated `0%`, and never a page count it does not have.
- **S5 — It actually runs.** `bun run build` succeeds, the Tauri app launches and exits cleanly with
  the frontend talking to the commands over real IPC — verified the way T4 verified the shell, by
  process, not by assertion.
- **S6 — Strings are externalised from this first screen**, which the plan locked as a constraint
  because the maintainer works in Spanish and an open-source product cannot assume it: one catalogue,
  no literals scattered through components, English by default with Spanish available.

**Deliberately not in this slice**: filters, sorting, manual override, the capabilities screen, the
suggestion queue, the tray, and the first-party reader. They are their own tasks, and the slice exists
to prove the spine end to end — store to provider to IPC to pixels — before more is built on it.


## Specs — UI shell (grid, navbar, screens, filter and sort)

Items 3, 5, 6 and 8 of L17, plus the parts of item 7 that need no new data. Item 7's numbers, item 2b's
editable titles, item 4's covers and item 1's folder chooser are separate units with their own specs.

- **S1 — A bottom navbar across Inicio / Biblioteca / Configuración**, with the current screen
  unmistakable, and **no invented content**: a screen with nothing to show yet says what it will show
  rather than filling the space with plausible-looking numbers.
- **S2 — Biblioteca is a three-column grid of cards.** Each card carries the document's title (the
  embedded one, or the file's own name — the rule shipped in L18), its page count, and its reading
  progress **with provenance in words**. Covers arrive later (item 4), so the grid reserves their space
  with an obvious placeholder rather than a fake image.
- **S3 — Filter and sort, decided here rather than left open.** Filtering by title/path text and sorting
  by title, page count or reading progress are done **over the listing the store already returns**, not
  in SQL. The reasoning: the plan deferred store-side filtering "because nothing called it", and now
  something does — but 42 documents, or a few thousand, do not need a query language, and a listing the
  screen already holds is the smallest thing that works. **The known limit is recorded with it**: the
  listing is not paginated, so a library large enough to matter will need the store to grow, and that
  day is what should trigger it.
- **S4 — A card action that opens the containing folder**, through the launcher, with the logic in a
  testable function and an honest error when it fails. A path that no longer exists must say so rather
  than silently doing nothing.
- **S5 — Configuración shows what genuinely exists**: the progress-provider registry (id, capability,
  availability, and why a missing one is missing), the language selector, and the watched folders the
  store has recorded. **No invented settings** — an empty section says it is empty.
- **S6 — Every new string goes through the catalogue in both languages**, the progress-with-provenance
  rule holds on every screen, and no screen shows a path where a title belongs.
- **S7 — It runs**: the frontend builds, the workspace builds, and the app launches with the new screens
  reachable. Verified by process, as always.


## Specs — UI mobile shell (window chrome, tray, carousels, category screens)

Items 1, 2 and 3 of L20, in two units so each is reviewable: **(A) the window chrome and the tray**,
**(B) the home carousels and the category screens**. Item 4 (covers live in the app's data directory)
is a decision recorded in L20, not a unit of its own.

### A — Window chrome and tray

- **A1 — A phone-shaped window, deliberately.** Undecorated (native decorations would bring the
  maximise button the user does not want), a fixed size with phone-like proportions, `resizable: false`,
  no fullscreen. The shape is a product statement, not a limitation to apologise for.
- **A2 — A custom top bar with exactly minimise, hide and close.** No maximise control anywhere, and
  no route the *application* can close: no button, no double-click on the bar (its drag is an
  application command rather than Tauri's drag region), and `maximizable: false` in the configuration.
  **Corrected after verification: on Linux that flag is a no-op.** Tao's GTK backend implements
  `set_maximizable` as an empty function, so a window-manager shortcut such as GNOME's Super+Up can
  still maximise the window and the application cannot prevent it. The criterion therefore covers the
  application's own affordances plus the flag **where the platform honours it** — not an absolute claim
  the platform cannot deliver.
- **A3 — "Hide" is only honest with a tray, so the minimal tray lands here.** An icon with show/hide and
  quit. **A hide button without a tray makes the application vanish with no way back**, which is a trap
  rather than a feature — this is the reason the tray is in this unit and not deferred to Phase 3.
  Tray support on the reference host was verified in Phase 0.
- **A4 — Closing really closes**, and the tray's quit is the same path. Nothing may leave a process
  behind: this project has already been bitten by a grandchild surviving its parent.

### B — Home carousels and category screens

- **B1 — Three carousels on Inicio**: documents in progress, finished, and not opened for a long time,
  each scrolling horizontally. **A carousel with nothing in it says so** rather than showing an empty
  strip — the honesty rule, applied to a component that invites fake filler.
- **B2 — A button at the top of each carousel opens the full list for that category**, a screen of its
  own with the same cards as the library.
- **B3 — The queries the categories need, written by their first caller.** "In progress" is a position
  recorded and not at the end; "finished" is a position at the end; "not opened for a long time" is
  answered by `progress.updated_at`, which is real and written. **`documents.last_opened_at` must not be
  used**: it exists but nothing writes it, and a query built on it would return confident nonsense.
- **B4 — Every category states its own definition on screen**, because "finished" and "in progress" are
  readings of stored positions, not facts the document carries.

## Specs — UI adjustments (round 2, L22)

Eleven items the user listed after using unit A, in their order, one criterion each. The verbatim request
is `L22`. Where an item changes something already specified, the newer request wins and the older
criterion is corrected here rather than silently contradicted.

- **R1 — The tray follows the interface language.** Choosing Spanish must also change the tray: "Show",
  "Hide" and "Quit" become "Mostrar", "Ocultar" and "Salir". The tray is built by the desktop process,
  so its labels come from the **stored** language setting — the same single source the interface reads —
  and a language change updates the tray without restarting the application. Verbatim: *"Cuando coloco
  el idioma en español, las opciones del tray siguen en ingles."*
- **R2 — Nothing in the Inicio header but what names the screen.** The explanatory prose goes away.
  Verbatim: *"No me gusta todo el texto que hay en el header de la pantalla 'Inicio'."*
- **R3 — Spanish says "Ajustes", not "Configuraciones".** English stays "Settings". Verbatim: *"En
  español, no me gusta que diga 'Configuraciones' prefiero 'Ajustes', ya que es mas corta."*
- **R4 — Inicio ships the carousels.** This is `B1`–`B4` moving from "later" into this round, and it is
  the item the user reports as missing. Verbatim: *"Falta implementar los carruseles en la pantalla
  inicio."*
- **R5 — A wider window: 430×844** (was 390×844), with the same undecorated fixed shape. Verbatim: *"El
  viewport de la aplicacion debe ser un poco más ancho."* **The width and the column count are linked,
  and the arithmetic is recorded so the number can be chosen rather than guessed**: with tiles at a
  minimum of `8.5rem` (136 px), three columns need roughly `3×136 + 2×12 + ~32 = 464 px`, so 430 px still
  renders two columns and only about 470 px renders three.
- **R6 — Smaller paddings.** The shell's and the screens' paddings are reduced to values that leave the
  content room on a 430 px window. Verbatim: *"Debes adaptar los padding tambien, tienes que reducirlos y
  no hacerlos tan grandes como estan actualmente."* **Refined in L23**: the top of the screen is the
  exception and goes the other way — the user found each screen's own heading too close to the window
  bar, so the shell's top padding went up while everything else came down.
- **R7 — Progress is a line with the number on it.** Each card draws a bar standing for the fraction
  read, with the percentage number placed **over the bar, at the position that fraction corresponds to**.
  The sentence *"No hay progreso de lectura registrado"* goes away. Verbatim: *"No me gusta que diga 'No
  hay progreso registrado', prefiero que muestres el porcentaje, osea, tiene que haber una linea que
  represente el porcentaje y superpuesta a esta debe estar el numero que indica el porcentaje en la
  posicion que le corresponda."* **One limit survives the change**: a document with **no reading record**
  must not be drawn as 0 %. The application would be claiming the user has read nothing, when what is
  true is that it has no record at all — they may be on page 100 of a document it has never seen. So:
  with a record, bar plus number at its position; without one, an empty bar and no number. The earlier
  honesty rule is not repealed by this item, it is restated: never a percentage the store cannot support.
  **Refined in L25, twice over.** The number has to be **legible**: at the first size the user found it
  too small to read and it is now `0.85rem`. And the pill keeps itself inside the bar **by
  construction**: it takes a fixed width in its own font units (`3.4em`, `border-box`) with a clamp
  inset of exactly half of it (`1.7em`), so changing the text size can never leave it hanging outside.
  The reason this is stated rather than left implicit: the first attempt used a pixel inset measured
  from the then-current text size (19.2 px), which is the kind of value that rots silently the next time
  somebody changes the font — and that measurement is what caught the pill hanging 3.3 px outside the
  bar at 100 % in the first place.
- **R8 — Rows in Biblioteca are separated vertically.** The user's screenshot shows the containing-folder
  button of one row touching the card of the next: the rows need the same separation the columns have.
  Verbatim: *"No hay separacion vertical entre los elementos en la pantalla 'Biblioteca', puedes verlo en
  esta captura: /home/tomas-cuevas/Pictures/Screenshots/Screenshot from 2026-10-13 16-44-17.png"*.
  **Resolved (L23), and the gap was never the problem**: the card overflowed its own grid row. With the
  default `content-box`, `min-height: 100%` set the card's content box to the height of its row and then
  added the padding and the border on top of it, so the card's border box stuck out 24.4 px, ate the
  20 px row gap and overlapped the next row by 4.4 px. Reproduced and measured in WebKitGTK 4.1 — the
  engine the application renders in, driven headless with a faithful copy of the grid and the card: a row
  gap of **-4.4 px** as committed, exactly **20 px** with `box-sizing: border-box` on the card. Two
  earlier suspects were cleared by measurement rather than by argument: the `gap` shorthand (the built
  CSS did contain it) and the cover's `aspect-ratio`. **Refined in L24**: the two gaps must be *equal* —
  the user's follow-up, once the rows were visible at all, was that the vertical separation was larger
  than the horizontal one. The grid therefore went back to a single `gap: 0.75rem` for both axes, which
  is what this section asked for from the beginning.
- **R9 — The containing-folder action is an icon, at the top.** Icon only, no text label, moved away from
  the bottom of the card to the top, with an accessible name because an icon alone is not a name.
  Verbatim: *"El boton para ir a la carpeta contenedora solamente debe tener un icono y debe estar
  situado arriba en alguna parte, no me gusta donde esta actualmente."* **Refined in L27**: "arriba"
  turned out to need an anchor. The first attempt gave the action a row of its own above the cover, and
  the user's verdict was that it looked loose up there with nothing to belong to, and that the icon
  itself did not please them. It now sits on the **cover's top-right corner**, inside a relative wrapper,
  so it reads as an action on the document's own body — and it stays there once a real cover replaces
  the placeholder. The drawing is redrawn cleaner and larger (18 px). **The success sentence is gone**:
  opening the folder window is its own confirmation, and only a **failure** is stated, because silence
  there would leave the reader thinking it worked.
- **R10b — The page controls are reachable from both ends (L27).** The user's words: *"la paginacion
  deberia estar disponible arriba y abajo, actualmente esta abajo unicamente lo que obliga al usuario a
  hacer scroll hacia el fondo para poder paginar"*. One snippet is rendered above and below the grid, so
  the two copies cannot drift apart — the alternative, duplicating the markup, is how the second copy
  ends up stale.
- **R10 — Biblioteca is paginated, 10 per page.** The user's number. Page controls state where the reader
  is and how many documents there are, filtering and sorting still apply to the whole listing before it
  is cut into pages, and changing the page does not quietly drop the reader's filters.
  Verbatim: *"Debes agregar paginacion en la pantalla 'Biblioteca', queda feo que esten todos los
  elementos ahi, tal vez deban ser 10 elementos por paginacion."*
- **R11 — The user can browse for the folder instead of pasting a path.** A native folder chooser, with
  the manual entry kept as the secondary option. This is the dialog plugin of the desktop shell, so it
  is **a dependency decision recorded rather than a detail slipped in**. Verbatim: *"Sigue sin
  implementarse el boton para abrir el explorador de archivos para permitir al usuario navegar para
  encontrar la carpeta donde estan sus PDFs para no obligarlo a pegar la ruta."*

### Tasks — round 2

- **U1** R3, R6, R8 — **done** (`a3602f1`, corrected by `ea890ee` and `6d186c3`): "Ajustes", smaller
  paddings and more room above each screen's heading, and the row separation — which turned out to be
  the card overflowing its own grid row and not the gap at all, measured in WebKitGTK 4.1
- **U2** R7 — **done** (`51198aa`): the bar with the number over it, 0 % for a document with no record,
  and the number at `0.85rem` with the pill held inside the bar by construction rather than by a pixel
  inset measured from the text size of the moment. The size refinement and U3 travel together in
  `5fb339c`; the reason is in the note below rather than hidden in a message body.
- **U3** R9, R10 — **done** (`5fb339c`): the containing-folder action is an icon-only button **anchored to
  the cover's top-right corner** — a row of its own above the cover was rejected by the user as looking
  loose there — with the drawing redrawn at 18 px and named through `aria-label`; the card states **no**
  status text in any case; and the library is paginated at ten with the cut taken after filtering and
  sorting, clamped into range, and the controls above **and** below the grid. R9 is pinned through
  `svelte/server`, which the writer had reported as untestable without adding a dependency, and it is
  testable with what is already installed. **The dead space the user reported twice was never the gap**:
  a flex item does not grow by default, so the card's width followed its own text, a shorter title made a
  narrower card and its cover shorter still (measured 186.2 px against 146.0 px in one row). `flex: 1`
  fixed it; measured after, every card 209.0 wide, every cover 245.5 tall, the largest hole 28.6 px.
  **And one thing is deliberately NOT clean**: with both texts gone, the cross-file `status` chain
  (App → LibraryScreen → BookCard) is now dead code. It is left in place on purpose, as its own pending
  task, because removing it touches `App.svelte`, which has no tests, and a hurried refactor there at the
  end of a long session is the pattern that produced the last six layout bugs. Only what was local and
  demonstrably dead — the `.status` CSS, with no element using it — is gone.
- **U4** R2, R4 — **done** (`b7c5991`): Inicio keeps only its heading, and the three carousels (`B1`–`B4`)
  with their own screens, their definitions on screen and a card-sized place kept when empty, backed by
  the pure `categoryEntries` selector and one shared `BookGrid`. The card's shape took four attempts and
  the last one is L31; the four category decisions (30 days, never-opened excluded, unknown total
  excluded, categories independent) are recorded in the commit body and confirmed by the user
- **U5** R1, R5 — **done** (`6156f08`): the language is stored in the database (schema V3, a `settings` key/value table), the tray is worded from it at launch and re-worded on a change without a restart, and the window is 430×844. Proven at runtime by reading the tray's own menu over the session bus, both ways
- **U6** R11 — the native folder chooser, with the plugin decision recorded

## Specs — UI adjustments (round 3, L32)

Three items the user listed the moment the tray followed the language (verbatim in `L32`). They are about
the tray's own menu, which is native and therefore belongs to the desktop process, not to the interface's
catalogue.

- **R12 — The tray offers the settings screen.** A tray entry that opens Ajustes. The tray is native and
the screen lives in the webview, so the desktop process has to *ask* the interface to navigate rather
than reaching into it; the request travels as one event naming the screen, and the interface answers it
through the routing that already exists (`hashForScreen`), so a tray click and the navbar land on the
same screen by the same path. Verbatim: *"Lo unico que hace falta en el tray es la opcion para abrir la
pantalla ajustes."*
- **R13 — The visibility entry names the application.** The first entry reads "Show OCT", not "Show".
Spanish keeps the product name and follows the same shape: "Mostrar OCT". Verbatim: *"el primer texto
 deberia decir \"Show OCT\" en vez de solo \"Show\"."*
- **R14 — One visibility entry, not two.** The menu carries **either** the entry that shows the window
**or** the one that hides it, according to whether the window is visible right now, instead of offering
both and leaving one of them doing nothing. The state is read from the window rather than remembered
beside it — **corrected after the user found the defect**: the read is right everywhere except in the
instant right after the application's own action, where the platform still reports the previous state.
Measured by driving the tray's own menu over the session bus (`com.canonical.dbusmenu.Event`, the same
call the panel makes when a person clicks): a hide reads back as visible, a show does not, which left
the entry one click behind and is what the user reported as needing two clicks. There the action's own
outcome is the truth, because an action that failed never reaches the rebuild, and it is passed through
for that one refresh rather than stored.
The menu is rebuilt after every action that can change it. Verbatim: *"otra cosa que me
gustaria es que detecte si esta mostrada o no la app, para que unicamente muestre una de las dos
opciones \"Show\" o \"Hidden\"."*

- **R15 — A line divides the three entries.** Each of the three separated from the next by a divider, so
the menu reads as three groups rather than one list, as the tray menu the user pointed at does.
Verbatim: *"Tambien, debes colocar una linea que divida las tres opciones, algo similar a este menu"* —
with a screenshot of another application's tray menu as the example.

The words are the user's. Where one of them names a state ("Hidden") the entry is written as the
**action** that state makes available ("Hide OCT"), because a menu entry is something a person clicks,
and the pair stays the two ids the shell already owns.

### Tasks — round 3

- **U7** R12, R13, R14, R15 — **done** (`f2464eb`): the tray's menu carries a settings entry that asks the
  interface to navigate (and brings the window up first), names the application in the visibility entry,
  offers exactly one visibility entry chosen by the window's real state, and divides the three with
  lines. Three defects were found and fixed on the way — see `L34`

## Specs — T17: the ingestion loop (L35)

The defect the user reported after following the application's own recipe: reading a document produces
no record at all. Measured before designing anything — the real store holds 42 documents and one watched
folder, with **zero** rows in `progress` and **zero** in `open_events`, so no reading position has ever
been written — while Evince's metadata was present and live on disk at the very moment: the source is
there and being updated. The provider that reads it exists (`T9`) and the open-document detector exists
(`T8`), but **nothing calls either**: the desktop process starts no loop, and `record_progress` has no
caller outside a test. The tracker never tracked.

So the unit is the loop, not another adapter. `S1`–`S6` below, and the user's own four steps (add a
folder, scan, read in Evince, reload) are its acceptance test.

- **S1 — The application polls while it runs.** A tick in the desktop process asks the open-document
detector what is open and records what the providers say about those documents, so no interface action is
required for a position to be recorded: reading a document in a viewer is enough. The interval is one
named constant with its reason in the code — short enough that a position appears immediate, long enough
to cost nothing.
- **S2 — A position that has not changed is not written again.** The store appends a row per record, so a
tick that wrote unconditionally would grow the table forever. Each document's newest recorded position is
compared with the provider's answer, and nothing is written when the two agree.
- **S3 — Silence is not a value.** A provider answering `Ok(None)`, or a document no provider knows,
writes **nothing**: "no record" and "page zero" are different facts, and the interface already has an
honest way to show the first.
- **S4 — A missing capability is reported as one.** `OctError::NotSupported` from a provider or from the
detector is surfaced as a missing capability, never as an empty answer, because an absent provider and a
viewer with nothing to say are the two facts this project refuses to confuse.
- **S5 — Catch up on launch.** Once at startup the loop sweeps **every** known document, not only the open
ones, so progress written while the application was closed is recorded instead of lost. It runs off the
main thread and reports failures rather than failing silently.
- **S6 — The loop is a decision, and the decision is tested.** Which documents to poll, which provider to
ask, and whether to write are plain functions taking their collaborators as arguments, so every rule above
is pinned by a test that needs neither a display nor a running application. The tick itself is the
thinnest possible wrapper over them.

### Tasks — the tracker's loop

- **T17** S1–S6 — **done** (`151ef99`): the ingestion loop. The desktop process sweeps the library once
  at launch and then ticks every three seconds; the detector and the providers finally reach the store,
  and reading a document produces a record. Verified by the parent on the user's own library: 0 → 28
  positions with `evince-gvfs/exact` provenance, and the document open at that moment at its real page
  (`L36`)

## Specs — the carousels' shape (round 4, L37)

Three items: the first two reported by the user once the carousels finally had content, the third found by
the maintainer in the screenshot they attached.

- **R16 — A carousel is short.** It shows a bounded number of tiles rather than every document in its
category, because a strip long enough to hold a whole library is not a carousel. Verbatim: *"hay que
limitar el numero de elementos que puede aparecer en el carrusel, ya que a mi, me aparecieron demasiados,
tantos que molestaba tener un carrusel tan largo"*. The button that opens the category's full list already
exists (`B2`) and is where the rest belong, so the bound is what makes that button mean something.
- **R17 — A strip never scrolls vertically.** Verbatim: *"dentro del contenedor de un carrusel puede haber
scroll vertical y eso no me gusto nada"*. The cause is a CSS rule rather than a size: `overflow-x` set to
anything but `visible` makes the other axis compute to `auto` as well, so the sideways scroll silently
enables a vertical one that then shows for any overflow at all. The strip scrolls sideways and only
sideways.
- **R18 — The page count appears once.** The card renders `{count} páginas` twice, above the title and
again below it, because two identical blocks ended up in the file. Found by the maintainer in the
screenshot of `L37`; the user did not report it because it had been there since the card was rebuilt. The
count stays where it reads as the document's size; the other goes.

- **R19 — The page count sits on the cover.** It moves out of the card's text block and becomes a badge
on the cover's bottom-right, mirroring the folder action that already sits at its top-right. Verbatim:
*"estuve pensando que tal vez, el numero de pagina deberia estar sobre la portada, asi como ya se
encuentra el icono para abrir la carpeta contenedora. Tal vez, colocar el numero abajo a la derecha de la
portada"*. A corner badge cannot hold a sentence, so when the count is unknown nothing is drawn at all
rather than an invented number.

### Tasks — round 4

- **U8** R16, R17, R18 — **done** (`10dae28`): a carousel capped at ten tiles, a strip that scrolls
  sideways and only sideways, and one page count per card. Also the defect the maintainer introduced, and
  the test that was missing — see `L39`
- **U9** R19 — **done** (`667bec5`): the page count as a badge on the cover's bottom-right, mirroring the
  folder action at its top-right, drawn only when the count is known (`L40`)

## Specs — the watched folders (round 5, L41)

Five items the user listed after using the folder chooser, in their order. Two are what they saw and three
are behaviour.

- **R20 — A watched folder can be removed.** Verbatim: *"Se tendria que poder eliminar una carpeta
vigilida."* Removing a watch is not deleting a library: the folder stops being watched and the documents
and their positions stay, because the user asked to stop watching, not to forget what they read.
- **R21 — The watched folder's path is smaller.** Verbatim: *"El texto que indica la URL de las carpeta
vigilada deberia ser un poco más pequeño."* It is the least important thing on that row and currently
reads as the most important.
- **R22 — A subfolder of a watched folder is refused, and the refusal says why.** Verbatim: *"No se tendria
que dejar añadir una subcarpeta de una carpeta que ya esta vigilida. Cuando el usuario quiera hacer eso, se
le tendria que dar una alerta explicando por que se bloqueo y no se agrego la nueva carpeta que
selecciono."* The message names the folder that already covers the choice, because "it was blocked" with no
"by which one" is a half-answer. The rule compares path components, never string prefixes: `/lib/pdfs-old`
is not inside `/lib/pdfs`, and a string comparison would say it is.
- **R23 — The folder section stops looking broken.** Verbatim: *"La UI de la seccion para seleccionar
carpetas es HORRIBLE"*, with two screenshots. The cause is a layout fact rather than a taste: the form is a
flex row, so the chooser button, the disclosure and the scan button were laid out on one line with no
hierarchy, and the scan result broke a path across lines at the same weight as everything else. The section
is reorganised so the order of the task is the order on screen — choose or type, see the path, then scan —
with the path in a secondary size.
- **R24 — A watched folder opens from its path.** Verbatim: *"En la seccion de carpetas vigiladas, se tiene
que permitir hacer clic sobre la URL y que se abra la carpeta en el explorador"*. It opens the folder
itself, which `open_containing_folder` does not do.

### Tasks — round 5

- **U10** R20, R21, R22, R23, R24 — watched folders: removal, a smaller path, a refused subfolder that says
  why, a section that reads in the order of the task, and a path that opens the folder

## Specs — the scan section, the alert, and what removing a watch means (round 6, L42)

Three items from the user's first look at `U10`'s result. The third changes a rule `U10` had already written
down, and the newest word wins.

- **R25 — The scan section's composition.** Verbatim: *"La estructura/composicion de la seccion 'Escanear
una carpeta' no me gusta, es fea esa seccion."* The cause is visible in the screenshot they attached, and it
is composition rather than size: the section is a stack of six blocks of equal weight — two identical dark
buttons, three lines of text, the browser's default disclosure triangle, and a path broken mid-word — with
nothing grouping the things that belong together. The redesign puts the two ACTIONS in one row (choosing a
folder and scanning it), the field-like readout of the chosen path under them, and the manual entry as a
quiet link rather than a disclosure box. Two decisions inside that composition are worth writing down,
because they are taste and the next reader might otherwise "fix" them: the scan action is the prominent one
and stays disabled until there is a path (it is the action the panel exists for, and a disabled submit
button is a shape every form already teaches), and the confirmation states the count rather than listing
document names (the modal already names the folder they are all inside, the count tells the size of the
loss, and a wall of paths in a confirm dialog is noise at the moment a person wants a decision).
- **R26 — The refusal is a modal, not a line under the form.** Verbatim: *"La alerta ... no debe aparecer
debajo, tiene que aparecer en un modal/dialog."* The interface gets a modal of its own rather than the
native dialog plugin's message box, for two reasons: it keeps the words in the catalogue where every other
word of the interface lives, and the same modal is what the removal in `R27` needs to state its consequence
before doing something irreversible. The refusal's own sentence is fixed at the same time
(`#56`): re-adding a folder that is already watched returns the chosen path as its own covering folder, so
the alert read *"that folder is inside <the folder you just chose>, which is already watched"*. A person
learns nothing from being told that a folder is inside itself, so the covering folder being the candidate
is its own sentence.
- **R27 — Removing a watch takes its documents with it.** Verbatim: *"Elimine la carpeta de 'Borges' pero el
PDF que allí estaba, sigue apareciendo en mi biblioteca."* `R20` had said the opposite — that removing a
watch leaves the documents alone — and the user has now said what they actually mean, which is that a
folder they stopped watching should leave their library. **The consequence is destructive and is stated
before it happens**: the documents inside that folder are removed from the library, together with their
positions, and the confirmation names how many. Two guards: documents still covered by ANOTHER remaining
watch stay (the user's own store already holds a nested pair, so removing the outer folder must not empty
a folder that is still watched), and a position is not lost for good even when it is removed — the viewer
still holds it in its own metadata, so re-watching the folder brings it back through the launch sweep.
That last sentence is what makes the confirmation honest rather than scary, and it is the reason to say it.
**The count is not a guess the interface makes**: the same computation that performs the removal can be read
first, so the number the modal states and the number of rows that actually leave are answers to the same
question, given by the same code. What they are **not** is a reserved snapshot — they are two calls, the
second one re-reads the store, and a store that changed in between can make the confirmation stale by the
time it is answered. The sentence used to claim the two numbers "cannot disagree", which is more than the
code does; naming the window is the honest version.

### Tasks — round 6

- **U11** R25, R26, R27 — the scan section rebuilt around its two actions, a modal for the refusal (and for
the self-covered sentence, `#56`), and a removal that takes its documents with it after stating the count
  the same computation will carry out

## Specs — the listing is re-read while the application runs (#46)

Found while closing the category recalculation, and named there as the half that fix does not cover: a
document that FINISHES, or that GAINS ITS FIRST record, is a change to the listing itself rather than to the
clock, and re-reading the time does not re-read the store. `App` loaded the library at mount and after a
scan, so a reader watching their own progress — which the tracker writes every few seconds from the desktop
process — had to relaunch the application to see Inicio change. The application looked dead for exactly as
long as it was working, which is the worst shape a defect can have.

- **R28 — the listing is re-read on a clock of its own, and the re-read is quiet.** While the application
runs, the library is re-read on an interval, so a document that gains its first record, moves through its
pages, or reaches its last one appears where it belongs without anybody touching the interface. The interval
is a named constant with its reason written down, and the re-read does NOT raise the loading state: that
flag belongs to the first load — "reading the library" may only be said when there is nothing on screen
YET — and reusing it would flash an empty grid over a listing that is already there, once per interval. A
failed re-read is reported where the first load's failure is reported, because it is the same inability to
read the library, and it clears on the next success: a transient failure heals itself instead of leaving a
listing that looks current and is not.

### Tasks — #46

- **U12** R28 — the refresh clock beside the category clock, the quiet re-read in `App`, and the tests that
  pin the tick, its cleanup and its bound

## Specs — the carousels' order (L45)

- **R29 — the most recently read document comes first, and the tiles run towards the oldest.** Verbatim:
*"en el carrusel, el ultimo elemento que se haya leido siempre aparezca primero. Se vayan ordenando de
izquierda a derecha de mas reciente a más antiguo"*. The order is part of the category rule rather than a
property of the screen, so it lives in the same pure selector that decides which documents a category holds:
the carousel, a category's own screen and the ten-tile cap all read one order and cannot disagree about it.
Two facts make the rule honest rather than approximate. **What the store knows as "read" is the moment a
source that OBSERVED the reading says it happened** — `progress.read_at` (L46) — and not the moment the
position last changed (`updated_at`), which is when the application looked; nothing writes
`documents.last_opened_at`. This sentence was corrected in L46's round: it named `updated_at` when R29 first
landed, and that is now false by design. A document whose reading time no source recorded sorts to the far
end rather than claiming to be the newest. And **the order must be a function of the data alone**: the launch sweep records a
whole library inside a second, so equal timestamps are the common case, and the fallback is the displayed
name followed by the row id. The listing is re-read every fifteen seconds, and an order that could change
on its own would make ten tiles jump under the reader's hand.

### Tasks — L45

- **U13** R29 — the recency order inside `categoryEntries`, with the tests that pin it, its tiebreak and its
  honesty about "read"

## Specs — cuando lo leíste (L46)

- **R30 — un tiempo de lectura sale de una fuente que observó la lectura, y "no se sabe" es una respuesta en
sí misma.** Verbatim: *"no hay ningun elemento en el carrusel que representa los PDFs que no se abren hace
tiempo. Y no es porque no haya documentos que no se hayan abierto, ya que muchos, la gran mayoria no los abro
hace bastante"*. El mecanismo detrás del carrusel vacío: `record_progress` sellaba cada fila con
`unix_now()`, así que el tiempo de un registro era cuando **la app miró**, nunca cuando la persona leyó — y el
barrido de arranque escribió la biblioteca entera dentro de 25 horas, con el registro más viejo a 1,05 días.
Con el umbral en 30 días, nada *puede* ser viejo. La corrección tiene tres partes:
  1. `progress` gana un tiempo de lectura propio (V4, anulable): NULL significa que la app **no sabe**, que es
     un hecho distinto de "leído hace mucho" y no se puede inferir de cuándo miramos nosotros.
  2. Lo llenan las fuentes que sí observaron una lectura: el historial del propio visor (el
     `recently-used.xbel` de GTK, escrito por el visor al abrir el documento) y un cambio de posición que el
tracker ve ocurrir.
  3. La categoría "hace mucho" y el orden de `R29` leen **ese** tiempo, no el de escritura de la fila.
  **El tiempo de último acceso del archivo se consideró y se descartó con evidencia.** El usuario lo eligió
  como respaldo, y la medición muestra por qué no puede funcionar: los 42 documentos de su biblioteca tienen
  un atime de **~1,1 días**, porque es oct el que lee los archivos (pdfium cuenta las páginas, el barrido los
  abre). Un hecho que escribe el propio escaneo de la aplicación no es evidencia sobre el lector. Y el
  historial cubre 15 de 42 documentos, con uno a 37,8 días: eso ya llena la categoría con la verdad en vez de
  con una suposición. Queda registrado para que nadie lo vuelva a deducir.

  **La marca de agua, y su única excepción (la especificación decía de más; la verificación lo encontró).**
  El tiempo de lectura nunca retrocede: el historial registra el instante de la apertura, unos segundos antes
  de que el tick vea la página moverse, así que un barrido posterior de la misma tarde no puede hacer que un
  documento leído hoy parezca más viejo. La excepción que la frase no decía: un tiempo **guardado en el
  futuro** no se conserva, se reemplaza por ahora. Un valor futuro sólo puede venir de un reloj que se movió, y
  no es evidencia sobre una lectura; en el tick estamos viendo el documento abierto, así que "ahora" sí es una
  observación. El código ya hacía esto; lo que faltaba era el test que lo dice y la frase que no lo ocultaba.

  **Un tiempo que una fuente reporta en el futuro no es creíble (hallazgo real de la verificación).** El
  barrido lo aceptaba y lo guardaba, y como el tiempo de lectura es una marca de agua lo conservaba para
  siempre: un documento que nadie tocó en un año quedaba sellado como leído mañana, y el umbral de 30 días ya
  no podía alcanzarlo nunca. Ahora una sola regla sirve a los dos caminos —el barrido ignora una observación
  futura y conserva lo que ya sabía; el tick, que está observando el documento abierto, responde "ahora"— y cada
  camino tiene su test, además del test de la regla misma.

  **El campo `modified` del historial no es la fecha del archivo, medido en la máquina del usuario.** La
  verificación advirtió que tomar el máximo de `modified` y `visited` puede declarar una lectura que no
  ocurrió, y la medición muestra qué es ese campo acá: coincide con el mtime del archivo en **0 de 15** casos
  (los archivos tienen mtime de 315, 455 y 788 días mientras `modified` va de 0 a 119), y en los **4**
  documentos abiertos ese día `modified` se movió en todos mientras `visited` se movió en 2 — con `visited`
  solo, dos lecturas recientes se habrían perdido. Así que el máximo es la última vez que una aplicación GTK
  registró el documento, que es la apertura. Queda como **residual**: en una máquina donde `modified` signifique
  "el archivo se editó", la regla podría declarar una lectura que no ocurrió, y ningún dato de acá puede
  distinguir ese caso.

  **El orden manda al final a los documentos cuyo tiempo de lectura no conoce nadie.** La clave es infinito
  negativo, no cero: cero no es el final de la escala, es 1970, y un tiempo real por debajo —un historial
  editado a mano, una fecha anterior a la época— quedaba ordenado *detrás* de un documento cuya antigüedad no
  se sabe. La ausencia de una respuesta no es una respuesta, y tampoco puede pretender ser la lectura más vieja.

  **La migración a V4 es atómica, y ahora hay un test que puede fallar.** El paso corre en su propia transacción
  y `user_version` se sella al final, así que un paso que falla deja la base como estaba — pero eso era una
  intención escrita y nunca ejercitada. El test la vuelve un hecho: una base en la versión 3 que ya tiene la
  columna (de modo que el `ALTER` falla con "duplicate column name") se abre, falla, y sigue en la versión 3 con
  su documento y su posición intactos y legibles.

  **Verificado sobre la biblioteca real del usuario, en las dos direcciones.** La base migró de v3 a v4 en el
  lugar: 42 documentos y 51 filas medidos inmediatamente después de migrar (la verificación leyó 55 más tarde,
  las nuevas escritas por lecturas reales), 15 filas ganaron el tiempo en que fueron leídas y 36 siguen
  diciendo honestamente que ninguna fuente lo dijo, y "hace mucho" pasó de 0 a 5 documentos (64,9 · 64,2 · 63,0
  · 49,2 · 37,8 días). El usuario cerró el círculo sin proponérselo: abrió *El extranjero*, que el historial
  fechaba a 63 días, el tick lo selló con la hora de ese momento y el documento salió de la categoría solo.

### Tasks — L46

- **U14** R30, y `R29` viaja con esta unidad — el tiempo de lectura (V4, el historial del visor y la
  observación en vivo del tracker), la regla de "hace mucho" y el orden del carrusel leyéndolo, y la
  definición que la propia categoría muestra en pantalla diciendo qué mide. `R29` viaja acá porque el orden
  que agregó ordenaría por el mismo tiempo distorsionado, y arreglar la fuente sin el orden dejaría la mitad
  del defecto en pie.

**How a unit lands (L26).** A unit is done when the user says they like it, not when the checks pass.
The sequence is: implement → run the checks (tests, build, and the engine measurement where one applies)
→ rebuild the binary and hand the running application over → **wait** → commit with its tests and docs
only after the user has approved what they saw. The working tree therefore holds the finished unit
uncommitted while it waits, and **only one unit is in flight at a time**, so an unapproved change is
never mixed with the next one in a diff the user has to read.


**L47 — la app viene al mundo en el tray, y vuelve al visor por lo que olvidó (R33, y el defecto de reagregar la carpeta).**

**R33, decidido por el usuario en esas palabras:** *"Si, la app jamas debe arrancar visible"*. La respuesta de plataforma vino
primero y es la segunda limitación de su tipo: una ventana creada visible se mapea al arrancar y el compositor la enfoca,
"no enfocar" es sólo una sugerencia que se vuelve a habilitar tras el primer dibujo, y la llamada que lo diría en serio no está
expuesta por la biblioteca — así que "visible pero nunca enfocada" no es entregable y queda REGISTRADO en vez de prometido. Lo
que sí es entregable es que la ventana no se mapee nunca: arranca oculta y el tray la muestra, en una acción del usuario, que es
cuando enfocar está bien. Medido con la lectura honesta que la app hace de su propia ventana: el menú dice "Mostrar OCT" apenas
arranca y pasa a "Ocultar OCT" cuando se la muestra. Y el cambio expuso el cuarto defecto de la forma recurrente: el arranque le
pasaba el hecho "visible" al menú a mano, y eso era CIERTO mientras la configuración hacía nacer la ventana visible; cambiar la
configuración convirtió un hecho verdadero en una mentira y nada podía notarlo — el menú ofrecía "Ocultar" sobre una ventana que
nadie podía ver.

**EL DEFECTO DE REAGREGAR, reportado por el usuario:** *"Quite la carpeta que tenia vigilada y luego la volvi a agregar... Todos
los documentos me aparecen en 0%"*. Quitar una carpeta vigilada se lleva sus documentos y sus posiciones — eso es R27, regla del
propio usuario, y queda como está. Lo que faltaba era la otra mitad: nada volvía a observar después. El barrido de ARRANQUE era
el único lugar donde se lee el historial del visor y se le pregunta a los proveedores por un documento que nadie tiene abierto, y
en el arranque no había nada que observar, porque el reescaneo crea los documentos después. El arreglo: el bucle de seguimiento
vuelve a observar cuando CAMBIA el conjunto de documentos de la biblioteca, y no hace nada cuando no cambió — así el historial se
sigue leyendo una vez por barrido y nunca una vez por tick. Verificado sobre la biblioteca real, doce segundos después de
relanzar y sin que el usuario tocara nada: 0 filas pasaron a **29 documentos con posición**, 16 con fecha de lectura, 27 en curso
y 7 en "hace mucho", **todas las posiciones con fuente evince-gvfs** (observadas, no inventadas), stderr vacío — y los 13
documentos que el visor ya no recuerda quedan SIN registro en vez de recibir un 0% inventado.

**UNA COSA MÁS QUE EL USUARIO ENCONTRÓ, y la introdujo la nota de primera vez de este mismo proyecto:** durante esos segundos
posteriores a un reescaneo la lista está momentáneamente vacía, y la pantalla renderizaba su estado VACÍO — la nota que dice que
todavía no hay nada — mientras el trabajo estaba en curso. Sus palabras: *"por unos segundos, inicio estaba vacia, como si no
hubiera ninguna carpeta vigilada... No pasa nada si tarda unos segundos en cargar, lo que deberia pasar es que se le muestre al
usuario algun tipo de loader para que este no crea que hay errores"*. La regla que se sigue de ahí es la misma que este proyecto
vuelve a aprender: **una pantalla vacía tiene que significar "cargado, y genuinamente vacío", nunca "todavía no cargó"**.


**L48 — el estado vacío dejó de mentir: mientras algo carga, se ve un loader (el defecto que el propio proyecto introdujo).**

El usuario lo reportó así: *"tarda unos segundos en cargar todo, por lo que pase de la carpeta ajustes a inicio y
por unos segundos, inicio estaba vacia, como si no hubiera ninguna carpeta vigilada... No pasa nada si tarda unos
segundos en cargar, lo que deberia pasar es que se le muestre al usuario algun tipo de loader para que este no crea
que hay errores"*. La causa: durante esos segundos la lista está vacía DE VERDAD, y la pantalla renderizaba su
estado vacío — la nota de primera vez que este proyecto agregó unas horas antes — que es verdadera sobre una
biblioteca vacía y falsa en ese momento, porque no falta nada: el trabajo está en curso.

La regla quedó en una función pura y con nombre: **el contenido siempre gana; "vacío" sólo se muestra cuando no hay
nada en curso; y el refresco silencioso de quince segundos NO cuenta como "en curso"** — eso último es lo que
impide que parpadee una pantalla que ya muestra documentos, y es una restricción, no un detalle. El test que
reproduce el caso del usuario (una recarga en curso con la lista vacía muestra el loader y no la nota) falló antes
del arreglo con el cuerpo recibido conteniendo justamente la frase falsa.

Residuos, escritos: la pantalla de una CATEGORÍA quedó fuera de las superficies del trabajador y todavía podía
mostrar su mensaje vacío durante un escaneo (se cierra como seguimiento inmediato); y sólo hay test de pantalla
para Inicio, mientras la biblioteca se cubre con la regla pura más el chequeo de tipos. La lección que se guarda es
la que este proyecto ya escribió cuatro veces y volvió a necesitar: **una pantalla vacía tiene que significar
"cargado, y genuinamente vacío", nunca "todavía no cargó"**.


**L49 — cerrado el último lugar donde una pantalla vacía podía mentir: la pantalla de una categoría.**

El defecto del estado vacío (L48) tenía un tercer lugar: la lista completa que abre el botón de un carrusel seguía
decidiendo por su cuenta, con una bandera que sólo conocía el final de la primera carga — no un escaneo en curso ni
el trabajo de quitar una carpeta — así que durante esos segundos mostraba su propia frase de vacío. Ahora usa la
misma regla compartida que Inicio y la Biblioteca, con la misma bandera real. El test del trabajador reprodujo el
caso del usuario en esa pantalla (el cuerpo recibido contenía la frase falsa) y su falsificación ignoró la bandera,
volviendo al rojo exacto. 127 tests de interfaz y tipos limpios.

Con esto la frase que este proyecto necesitó cinco veces queda sin ningún lugar donde incumplirse: **una pantalla
vacía significa "cargado, y genuinamente vacío", nunca "todavía no cargó"**.


**L50 — las portadas, primera mitad: la primera página de verdad en cada tarjeta (R32/U20a).**

La medición anterior ya había decidido el diseño (≈2 ms por página en caliente, y el costo es POR PÁGINA y no por
libro), así que esta mitad no necesitó ninguna migración: la portada se genera la primera vez que una tarjeta la
pide, se guarda como PNG en la carpeta de datos de la aplicación, y después se sirve del archivo. La interfaz la
recibe como una imagen embebida, porque la alternativa necesitaba configuración nueva y un permiso extra del
sistema, y una portada de 400 px es chica. El codificador fue la ÚNICA dependencia agregada, con su motivo
escrito: la biblioteca de PDF puede dibujar pero no escribir PNG con las funciones que este proyecto dejó apagadas
a propósito, así que entró el codificador más chico que hace el único formato que hace falta. Y como pdfium
atiende de a uno, las portadas se producen de a una y fuera del hilo de la interfaz.

**Lo que el usuario ve, y lo que no**: ve la portada real donde antes había un recuadro, y mientras no exista
sigue viendo el recuadro — nunca una imagen rota. Un PDF que no se puede leer (roto, con contraseña, borrado) deja
el recuadro y su motivo va al registro, no a la pantalla.

**Verificado sobre sus documentos reales y sin tocar su carpeta de datos**: 16 portadas generadas al abrir la
aplicación, todas PNG válidos de 400 px de ancho con la proporción de cada página (518 o 566 de alto, sin
recorte), entre 97 y 333 KB. 238 comprobaciones del proyecto, 134 de la interfaz, clippy y tipos limpios.
Falsificaciones reales: que la portada guardada se reutilice, que un PDF ausente sea "sin portada" y no un error,
y que una respuesta vacía jamás se muestre como imagen. El usuario aprobó el resultado en su pantalla.

**Residuales asentados**: todavía no hay invalidación de la caché (una portada se guarda por documento, así que un
archivo que cambie de contenido sin cambiar de identidad podría mostrar una portada vieja, y eso lo resuelve la
segunda mitad, que es la que sabe que la portada la eligió el lector); un libro ESCANEADO puede costar mucho más
que el paper medido y nada lo acota; y cada pedido recorre la lista de la biblioteca, aceptable con 42 documentos
y anotado por si crece. **Una nota del proceso que vale**: el padre intentó borrar la carpeta de portadas para
medir desde cero y la política de seguridad lo bloqueó; el borrado no se hizo y la medición se hizo igual, que es
la respuesta correcta para datos del usuario fuera del proyecto.


**L51 — portadas, segunda mitad: la elección del lector, un solo editor, y los dos bugs que el usuario encontró usándolo (R32/U20b).**

Lo que entró: el esquema V6 con dos hechos anulables por documento (qué página eligió el lector, y el nombre del
archivo de una imagen suya dentro de la carpeta de portadas), con UNA sola regla de precedencia —su imagen, si no
la página elegida, si no la primera—; el reescaneo no toca ninguno de los dos, como no toca el título; el nombre
del archivo de la portada lleva la página adentro, así que cambiar de página no puede servir la vieja; la imagen
se elige por el diálogo nativo y se COPIA, porque es dato del lector y tiene que sobrevivir a que el original se
mueva o se borre; y un archivo que no es imagen se rechaza como argumento inválido, no como falla de
almacenamiento.

El usuario corrigió el diseño después de verlo: *"No me gusta que se haya añadido otro icono más sobre la portada.
Lo mejor seria que todo lo relacionado a la edicion este sobre el icono del lapiz"*. Tenía razón por dos motivos:
un tercer ícono sobre una portada chica es ruido, y el lápiz ya significa editar el documento. El control salió de
la esquina y todo lo de edición quedó detrás del lápiz, en UN editor del documento. Ese cambio cerró además una
deuda que este documento venía anotando dos veces: la ventanita del editor por fin tiene su propio test.

Después, usándolo, encontró DOS bugs más, y los dos eran de la INTERFAZ y no de los datos — cosa que se comprobó
MIDIENDO la base antes de tocar nada: su título y su página elegida seguían guardados y correctos. Uno: al elegir
una imagen, lo que la pantalla mostraba volvía al título del archivo. Dos: la página elegida no volvía al campo
del editor al reabrirlo. El arreglo cambió la REGLA en vez de parchear el camino: el editor ahora lee siempre el
dato vigente de la lista, y las tres acciones de portada terminan en el mismo lugar. Los borradores —lo que la
persona está escribiendo— se llenan en un solo punto, al abrir el editor, y ningún refresco los pisa. Los dos
tests reproducen el síntoma TEXTUAL del usuario: uno esperaba su título y recibía el del archivo, el otro
esperaba la página 5 y recibía la 1.

**Una honestidad del arreglo, que queda escrita**: el trabajador no logró reproducir el camino exacto por el que
el estado se perdía; lo que hizo fue imponer la regla que lo hace imposible y unificar los caminos. Se dice así
en vez de afirmar una causa que no se probó, y el usuario confirmó que el comportamiento quedó bien.

**Estado final**: 255 comprobaciones del proyecto y 152 de la interfaz, clippy y tipos limpios, la base del
usuario verificada antes y después (42 documentos, su título, su página 5) y la aplicación corriendo con todo.



**L52 — el español con una sola voz, un campo que se declara siempre, y la limpieza de la cadena muerta (con un agujero del propio proyecto anotado en el camino).**

Tres unidades chicas verificadas y commiteadas juntas: el catálogo español deja de hablarle al lector con dos
voces (once valores reescritos al registro neutro, con un test que vigila tres FORMAS —terminaciones verbales,
pronombres informales, imperativos al inicio de oración— y un meta-test que prueba que las palabras regionales
NO se marcan, que es lo que protege la línea deliberada: quien lee esto es rioplatense y neutralizar su dialecto
haría el producto más frío sin volverlo más correcto); un campo que la interfaz declaraba opcional pasa a ser
obligatorio, porque el backend siempre lo manda; y se removió la cadena de estado de la acción de carpeta, que
recorría la aplicación y tres pantallas hasta una tarjeta que ya no dibujaba nada desde que el usuario pidió
sacar esas frases.

**Dos briefs equivocados, en la misma dirección**: en los dos casos el padre eligió las superficies de memoria y
se equivocó. En la cadena muerta pasaba por SEIS archivos y no tres (la aplicación le entregaba el estado a las
tres pantallas, y dos de ellas lo reenviaban al componente compartido que alimentaba la tarjeta); el trabajador
se detuvo en vez de dejar una remoción a medias, el padre aprobó los seis caminos derivados, y la completitud
quedó PROBADA y no prometida: cero rastros en todo el frontend, la regla que la reemplaza en un solo lugar con
sus dos ramas fijadas, y la prueba de pantalla que faltaba ahora existe.

**Y lo que ese trabajador dejó, que vale más que el encargo**: la red de seguridad de este proyecto tiene un
agujero donde viven los bugs de interfaz, y quedó escrito en su propia sección más abajo. Cuatro de los defectos
de esta sesión fueron de cableado entre componentes.


**L53 — el autor editable, y la primera unidad que pasa por la revisión nativa antes de que el usuario la vea (R37).**

El pedido del usuario: *"tambien me gustaria que se pueda editar el autor del PDF o seleccionar uno de los autores
que ya estan cargados"*. Es el gemelo del título y sigue su forma a propósito: el esquema V7 agrega UN hecho
anulable (el autor del lector), el autor propio del documento nunca se sobrescribe, un reescaneo nunca lo borra, y
la regla que decide qué se muestra vive en un solo lugar al lado de la del título. El autor del lector se escribe
en el MISMO editor que abre el lápiz —título, autor y portada— con los autores que ya existen en la biblioteca
ofrecidos como sugerencias, para que nadie vuelva a tipear un nombre y termine con dos formas del mismo autor. Un
autor vacío es legítimo (muchos documentos no tienen) y vuelve al del documento, a diferencia del título, que
rechaza el vacío; eso está comentado donde vive la regla.

**La revisión nativa corrió sobre esta unidad ANTES de que el usuario la viera**, con tres lentes (riesgo,
legibilidad, confiabilidad), nivel alto por el lanzador de procesos, y **aprobó** el candidato; su autoridad quedó
quemada con el acuse exacto. Dejó dos avisos INFORMATIVOS, explícitamente no bloqueantes, cuya prosa el proveedor
no entregó (sólo identificador, lente y ubicación): quedan anotados para leerlos con calma y disponerlos, porque un
hallazgo que nadie disposiciona se convierte en ruido permanente.

**Tres líneas de test quedaron fuera de la lista de superficies del brief** —exactamente el mismo error que ese
brief había cometido dos veces antes—; el trabajador se detuvo, el padre ratificó los tres archivos, y hacer el
campo opcional habría debilitado el tipo justo como se lo había arreglado unas horas antes.

**Estado**: 266 comprobaciones del proyecto y 165 de la interfaz, clippy y tipos limpios, la base real en V7 con
los 42 documentos intactos, y el usuario confirmó en su pantalla: *"funciona todo correctamente. El cambio y
actualizacion del autor no rompe nada."*

## A hole in this project's own checks (found while removing dead code, recorded the same day)

**The type check does not look at the components.** The script runs over the logic files only, the declaration
for component imports types them as an unknown value, and no Svelte-aware check is installed or run — the build
does not check them either. So a component can be structurally broken: a required property nobody supplies, a
property removed while a screen still passes it, a forward to something that no longer exists. Every check the
project runs stays green, because the tests only render the paths somebody chose to exercise and the framework
silently ignores unknown properties.

**Why this matters more than a normal gap**: it is exactly where this session's interface defects lived. Four of
them — the extra icon, the title reverting on screen, the page not coming back into its field, and the empty
state that claimed to be empty — were in component wiring, and the safety net I was leaning on when I said "the
tests will catch it" has its hole precisely there. The worker found it while refusing to leave a half-removal,
because it could not rely on the checks to catch the damage.

**To fix it properly, with the standard this project applies to everything else**: make the components part of
what is checked, and PROVE it by breaking a component on purpose — a required property left unsupplied — and
watching the check fail. A new check that has never been seen to fail is not a check.


**L54 — el agujero del chequeo, cerrado: los componentes ahora se revisan (y el primer defecto que apareció).**

Lo que estaba mal, en una frase: el chequeo del proyecto recorría sólo los archivos de lógica, los componentes se
declaraban como un valor desconocido, y ni el chequeo ni la compilación los miraban. Una pantalla podía pedir algo
que nadie le pasaba, o pasar algo que ya no existía, y todas las comprobaciones seguían en verde — porque los tests
sólo ejercitan los caminos que alguien eligió, y el marco ignora en silencio las propiedades que no conoce. Los
cuatro defectos de interfaz de esa sesión vivieron justo ahí, y la red en la que todos nos apoyábamos al decir "los
tests lo van a agarrar" tenía el agujero exactamente en ese lugar.

Ahora el comando de chequeo incluye un revisor que entiende componentes, y el build reusa ese comando, así que se
revisa sin que nadie tenga que acordarse de nada. La prueba no fue "anda": el trabajador rompió una pantalla a
propósito de TRES formas distintas —una propiedad obligatoria sin pasar, una propiedad de más, un nombre reenviado
que ya no existe— y mostró el error exacto en cada caso, restaurando el archivo byte a byte con su hash.

**Y en su primera corrida real encontró un defecto de verdad**: un import muerto en una pantalla, que el proyecto
arrastraba y que DOS trabajadores anteriores ya habían marcado y dejado sin tocar para no meter cambios fuera de su
encargo. Se borró con autorización explícita del padre, y es una buena primera presa: exactamente la clase de cosa
que el agujero escondía.

**Lo que este chequeo NO hace, medido y no supuesto**: los avisos de accesibilidad se imprimen pero no bloquean
—sólo los errores de estructura—, y no puede ver un componente que estructuralmente está bien y se comporta mal
(una condición al revés, un dato equivocado), ni el diseño visual, ni el contenido de los textos. De los cuatro
defectos históricos, los de cableado quedaron cubiertos y los de comportamiento no.

**Un costo honesto**: para sumar el revisor hubo que bajar TypeScript un escalón (de la 7 a la 6), porque el
revisor todavía no admite la 7. Mismas reglas, misma configuración, y el chequeo pasó de ~0,3 s a ~3,7 s. La
alternativa —una segunda dependencia para no moverse— se evaluó y se descartó, y el porqué quedó escrito donde
viven las dependencias.


**L55 — los dos avisos del review, dispuestos y arreglados: el comentario huérfano, y guardar que podía quedar a medias.**

Los dos avisos venían de la revisión de la unidad del autor, marcados como informativos y no bloqueantes. El
proveedor NO guardó su prosa —en el repositorio sólo quedó el recibo de consumo del linaje—, así que un verificador
independiente los dispuso razonando desde el código y lo etiquetó así, en vez de presentarlo como palabras del
revisor. Los dos resultaron REALES, y el padre los confirmó leyendo el código.

**El importante**: guardar el título y el autor eran DOS escrituras independientes, en dos comandos que abrían el
almacén por separado y ejecutaban dos actualizaciones sin transacción. Si la segunda fallaba después de la primera,
el título quedaba guardado, el mensaje de error daba a entender que no se había guardado nada, y el refresco de la
lista no ocurría. Es de la misma familia que este proyecto viene cazando toda la sesión: un hecho a medias con un
mensaje que miente sobre él. La corrección no parchea el camino: **guardar es UNA operación**, con una sola
transacción que escribe los dos hechos o ninguno, y una sola invocación desde la interfaz — así el mensaje dice la
verdad por construcción, porque ya no hay nada a medias que mentir. Las dos escrituras sueltas se eliminaron del
almacén y del contrato, y con ellas los dos comandos que las exponían. La semántica de "restaurar el del documento"
se conservó intacta: decide el TÍTULO y no toca el autor del lector, y eso quedó fijado por test.

**El otro**: la explicación de "el título que muestra una tarjeta" estaba pegada arriba de la función EQUIVOCADA,
mientras la función que describe quedaba sin ninguna. Se movió tal cual, verificando por el diff que las líneas
agregadas son idénticas a las borradas. Es documentación: no hay falsificación posible y no se inventó ninguna.

**Un riesgo que el trabajador dejó escrito y que vale más que el arreglo**: el lado de la INTERFAZ de ese contrato
es el único eslabón sin test —el suite dibuja del lado del servidor y no puede clickear, y sumar una dependencia de
DOM estaba prohibido en su encargo—, así que una edición futura del editor podría reintroducir una segunda llamada
y las cuatro comprobaciones seguirían verdes. El lado del almacén sí está cubierto, con un test que falla si la
transacción desaparece: *"un guardado cuyo autor fue rechazado no debe dejar el título atrás"*.

**Estado**: 269 comprobaciones del proyecto y 165 de la interfaz, clippy limpio, y el chequeo de tipos —que ahora
también mira los componentes— con 0 errores y 0 avisos.


**L56 — la cola de sugerencias, mitad de atrás: la base, los comandos y el productor (esquema V8).**

Lo que este encargo dejó en claro ANTES de escribir código, y que vale más que el código: **la pantalla de capacidades
ya existe completa** (proveedores, qué puede cada uno, por qué falta el que falta, en los dos idiomas), así que no
había nada que construir ahí; **el override manual no se puede hacer donde se lo había encargado** (necesita el bucle
que observa y el editor de documentos) y hoy un dato manual sería **derrotado en silencio** por el automático; y la
cola de sugerencias era lo único real y faltante, dividida en dos mitades.

**Esta unidad es la mitad de atrás**: el esquema V8, la API del almacén, los cuatro comandos y el productor enganchado
en el tick. La mitad de adelante —que el lector la vea y decida— es su propia unidad y NO se empezó: hoy la función
existe pero es inalcanzable desde la pantalla, y el trabajador lo dijo en vez de disimularlo.

**Un muerto y un vivo, y lo que el vivo encontró**: el primer trabajador murió por un error del proveedor dejando el
árbol coherente (compilaba entero) y las pruebas de los comandos ya escritas. El segundo confirmó lo que había,
completó lo que faltaba (las dos falsificaciones con su rojo exacto y su hash restaurado), y **encontró un defecto
real en el código del muerto**: la operación que decide una sugerencia prometía en su comentario que una decisión no
se puede tomar dos veces, pero la condición que lo garantiza no estaba en la operación — así que una segunda decisión
pisaba la primera EN SILENCIO, y dos tests ya escritos estaban en rojo por eso. Se arregló con una línea. Es el tipo
de cosa que el proyecto persigue desde el principio: una promesa escrita que el código no cumplía.

**Estado**: 283 comprobaciones del proyecto y 165 de la interfaz, clippy limpio, y el chequeo de tipos —que ahora mira
los componentes— con 0 errores y 0 avisos. **Residual declarado y no escondido**: la garantía de "primero persiste,
después decide" está cubierta por comportamiento, pero no es falsificable de forma independiente, porque la guarda
anterior corre antes que las dos escrituras; lo que la sostiene de verdad es el chequeo posterior al escaneo.

## Delivery slices (recorded on request)

**Slice boundary, recorded here because the branch has grown past any PR-sized unit**: `feat/phase-1-core`
carries **74 commits and ~24.500 authored changed lines since `main`**. That is a long-lived feature branch by
design — every unit landed as its own work-unit commit, with its tests and its record, and the user approved
each one before it was committed. The DELIVERY strategy (whether this becomes one pull request, a chain of
reviewable slices, or is merged as-is) is the user's decision and is not inferred here; what this note fixes is
the fact itself, so nobody discovers it from a diff at review time. Everything from L45 onward belongs to the
later rounds and would be its own slice if slices are chosen.

## Open decisions

**Nothing blocking.** Both blocking decisions were made by the user:

- **Product name**: **`oct`**.
- **Git layout**: an independent repository at the project root, default branch **`main`**.
  Bootstrapped in commit `66f299a`; Phase 1 work proceeds on `feat/phase-1-core`.

**RESOLVED: the frontend framework is Svelte 5.** The T4 scaffold deliberately shipped plain
TypeScript + Vite so nothing was locked in, and the choice was held open until a screen actually
existed to judge it against. Decided by the user, who owns it: Svelte 5 over React and Solid, with
their stated experience being React — recorded here because that tension is real and worth seeing,
not because it changes the decision. The measured argument that *doesn't* apply: bundle size is noise
next to pdf.js, so the framework's weight was never the deciding factor; the contributor pool and the
ecosystem were, and the user chose ergonomics and less code for a UI of this size.

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
| T11 | **Done** | `2b8e9da` | Portable POSIX file identity. Reusable `#[cfg(unix)]` module so macOS inherits it; `st_dev`/`st_ino` primary; streamed SHA-256 fallback when either id is zero. 32 tests pass (was 25), clippy clean. Hard links proven to share an identity, and the stub test was replaced rather than deleted. |
| T5 | **Done** | `453a849` + `c2a059d` | `oct-store` is a real persistence layer: six tables, forward-only idempotent migrations, lossless `FileId` round trips and lookup by identity alone, per-platform database location. 47 tests pass (was 32), clippy clean. Two should-fix defects were found by an independent verification pass and fixed before the first commit — see L3 in the log for both, the accepted residual, and the `NULL`-distinct trap the test caught. |
| T6 | **Done** | `57f8c4a` + `a5b3ea5` | Watched-folder scanner with `pdfium` page counting, plus the `folders` API and the project's **first migration (V2)**. 69 tests pass (was 47), clippy clean. Full-library cross-check run independently by the parent *and* by the verifier: **42/42 page counts agree with `pdfinfo`**, 0 documents without a count, 0 extras, whole library in **1.5 s**. The parser was chosen by measurement, not assumption — `spike/parser/FINDINGS.md`, log entries L5 and L6. |
| T7a | **Done** | `e6003e2` + `8355b2d` | The sixth trait `FolderWatcher` (with `WatchTarget`, `WatchEvent`, `WatchEventKind`) plus the Linux inotify backend and the honest `NotSupported` stub. **85 tests pass** (was 69), clippy clean, and both non-Linux targets type-check. Verified twice: attempt 1 found a blocking defect — a directory moved out of a target stayed watched, so events outside the tree were reported under stale paths — plus two should-fix; attempt 2 settled those and found the coalescing window surviving a subtree release. The verification cap was reached there, so the last delta is parent-checked and RED-pinned only — stated plainly in L9. |
| T7b | **Done** | `41e11c6` | `WatchBridge` wires watcher events into the library: added/changed files counted and persisted, removals reported with the row and its reading progress deliberately kept, `Lost` rescanned, `Unwatchable` reported without pretending a recovery. **93 tests pass** (was 85), clippy clean, `oct-scan` still free of any `oct-platform` dependency, and independent verification marked **S1–S6 all satisfied with no findings**. |
| T8 | **Done** | `bfcd234` | The axis-B stub became a real `/proc` snapshot: held-open PDFs are reported with their process id and name, deleted targets skipped, output deduplicated and sorted. **102 tests pass** (was 93), clippy clean, both non-Linux targets type-check, `std` only. Independent verification marked **S1–S7 all satisfied with no findings**. The parent rewrote the determinism assertion (whole-system snapshot equality cannot be reliable) and the verifier confirmed it still fails on duplicates, unsorted output or a dropped controlled entry. |
| T9 | **Done** | `9bc6d9f` | The Evince GVFS adapter and the Linux `Launcher`: reading progress read through `gio`, the off-by-one converted once at the CLI boundary, and a missing page-capable viewer treated as a degraded success rather than an error. **120 tests pass** (was 102), clippy clean, `oct-core` untouched, no new dependency. Verified twice — attempt 1 found a panic path and marked S7 unsatisfied, attempt 2 marked **S1–S7 all satisfied**. Real end-to-end read over the library: **26 of 42 documents hold a position, 26/26 agree with raw `gio` output, 0 errors**. |

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

**Process note — RESOLVED**: T4 was implemented inline because subagent delegation broke
mid-session: creating the repository changed the session's Git identity, and the delegation layer
rejected writes with *"Writer session Git authority changed before admission"* while
`session_worktree_register` reported *"Session Git identity changed; cannot register a worktree"*.
Restarting Pi restored it, and T11 was then delivered by a delegated worker and independently
verified by the parent. The lesson is cheap and worth keeping: **creating or moving a repository
mid-session invalidates the session's Git binding, and only a restart repairs it.**

**Delegation is the default again.** Phase 1 tasks go to a worker with a tight brief and an
explicit list of allowed edit surfaces, and the parent verifies the result before committing
rather than trusting the report.

**Engram memory is still bound to the wrong project — same root cause, different subsystem.**
Restarting Pi repaired the Git binding but **not** the memory binding, because Pi resumed the same
session id (`01a12186-...`) and Engram keys its project to the session. Detection itself is already
correct — `mem_current_project` from this directory resolves `oct` via `git_root` — but writes are
refused with *"Pi runtime session ... belongs to Engram project tomas-cuevas, not oct. Start a fresh
Pi session in oct before capturing memory."* Routing the write with the `cwd` parameter is refused
the same way.

Two consequences worth knowing:

1. **The observations recorded before the repository existed went to the `tomas-cuevas` project**
   (35 observations there versus 1 in `oct`), because with no repository in `oct` the detected
   project was the home-level repository. Those entries — the plan, the Phase 0 results, T1 and T4 —
   are mixed in with unrelated home-directory work.
2. **No information is lost.** This document is the durable record and is committed to the
   repository; memory is a convenience layer on top of it. That is why every task closes by writing
   its outcome here rather than relying on the mirror.

**Fixed for the entries that already existed.** The six `oct` observations stranded in
`tomas-cuevas` were relocated into `oct` by a delegated worker — subagent sessions bind to the
repository project, which is the mechanism that made it possible — and independently verified
byte-for-byte by the parent before the originals were soft-deleted. Full mapping and evidence:
[`engram-migration.md`](engram-migration.md).

**Still required for new writes**: a genuinely **new** Pi session started inside
`/home/tomas-cuevas/Workspace/Proyectos/oct` — not a resumed one. Restarting Pi resumes the same
session id, and Engram keys the project to the session, so writes from this session remain bound
to `tomas-cuevas`.

**Root cause found, and it also explains R6.** An earlier session's own notes record that a stray
`codegraph init` run with `$HOME` as the project root created an empty `~/.git` at
`/home/tomas-cuevas` on 2026-09-19. Engram infers the project from the git root, so `$HOME`
became project `tomas-cuevas` — which is exactly why every `oct` memory written before the
repository existed was misattributed, and the same artifact behind R6. Removing that empty
`~/.git` would restore correct classification for home-directory work, but it is outside this
repository's scope and remains a user decision.

**RESOLVED for new writes (verified 2026-10-12, session `01a15ee9-…`).** A Pi session started
*inside* `oct` writes to Engram project `oct`. Proven directly, not inferred: two observations held
by `oct` (`5970`, the write-target probe, and `5977`, the migration summary) were deleted from this
session and the deletion was confirmed by read-back returning "observation not found". The earlier
blocker was never about detection — `mem_current_project` had always resolved `oct` from `git_root`
— it was that Engram keys the project to the **Pi session**, so a resumed session keeps the binding
it was created with. The fix is a new session, not a restart of the same one.

## Log

**L1 — user request, verbatim (2026-10-12, session `01a15ee9-…`):**

> Estuve trabajando en otra sesion anteriormente, necesito que retomes el trabajado, la sesiona anterior me dio para esta sesion lo siguiente:
>
> Lo que queda pendiente para la sesión nueva
>
>  1. Las escrituras nuevas: una sesión nueva arrancada desde oct es lo único que ata los writes al proyecto correcto. Reiniciar Pi no alcanza porque reanuda el mismo session id.
>  2. Limpiar dos artefactos míos en oct: la 5970 (sonda) y la 5977 (resumen de la migración). No puedo borrarlas yo — son escrituras a oct, que es justo lo bloqueado. Se borran en un segundo desde allá.
>  3. La duplicación 5951 / 5975 sobre R12 es deliberada: la 5951 es el resumen en inglés, la 5975 trae los números crudos y la lista de huecos residuales. Si preferís un solo registro, se saca la 5975.
>
> Cuando abras la sesión, alcanza con decirle que lea odd/tasks/pdf-progress-tracker.md y continúe desde la próxima tarea sin terminar.

**L2 — decisions taken in this session:**

- Item 1 verified, not assumed: the two deletions below succeeded **from this session**, which is the
  evidence that writes now bind to `oct`. Recorded under the Engram section above.
- Item 2 done: `5970` and `5977` soft-deleted (`hard_delete: false`, so the operation is reversible),
  confirmed by read-back. No information was lost: the migration audit record survives in
  `odd/engram-migration.md`, which is committed.
- Item 3: **both `5951` and `5975` are kept.** They carry different `topic_key`s and neither is a
  copy of the other: `5951` is the English summary, `5975` adds the raw per-page numbers, the
  residual-gap list and the process-verification lesson. Deleting `5975` would drop the residual
  gaps — the part that says R12 is *downgraded, not retired*. Remove it only if the user asks.
- Next unfinished task per the reconciled list: **T5**. Checked against the T4 record's own
  "Next in order" line, which names T5 first and notes that it needs T11's portable identity to be
  honest — T11 has since landed, so T5 is now unblocked.

**L3 — T5 delivered and verified (2026-10-12):**

- `crates/oct-store` is a real persistence layer: the six tables, forward-only idempotent migrations
  recorded with `PRAGMA user_version`, lossless `FileId` round-tripping and lookup by identity alone,
  and the minimum query surface needed to prove S1–S8. `OctError::Storage` was added as the single
  new error variant; queries with no caller yet are deliberately absent and listed as such in the
  crate docs.
- **Deviation found and corrected by the parent, not by the worker:** the worker stored the content
  hash in a column named `sha1` while T11's fallback is SHA-256. Renamed to `content_hash` before the
  first commit, because after data exists a rename is a migration and the wrong name would have been
  permanent. Recorded in the Data model section above.
- Two worker-added columns beyond the original sketch (`file_id_kind`, numeric identity as TEXT) were
  reviewed and accepted for the reasons in the Data model section.
- Open, low-severity notes: `path_normalized` currently holds the same bytes as `path` (R7 forbids
  Unicode normalisation, so a future cross-platform matcher will need its own fold rule), and
  `ProgressSource::External(&'static str)` is reconstructed through a small process-global string
  interner because the domain type carries a `&'static str` — T9's provider registry should replace
  that lookup with the registry's own ids.
- **Independent verification (a separate `gentle-ai-verify` pass, not the implementer's own checks)
  found one should-fix defect**: `upsert_document` matched by identity *or* path but never both, so a
  document whose identity became known later forked into a second row, splitting its reading history.
  Fixed by the parent before the first commit, with the matching rule now explicit in the crate docs:
  identity first (it survives renames); a row found by path that has no identity yet **adopts** the
  supplied one; a path that now holds a **different** file gets its own row, because attributing the
  older row's progress to the replacement is the silent lie the provenance columns exist to prevent.
  Path lookups resolve to the newest row for the path. A second, related hole was closed at the same
  time: an upsert never erases a stored value with an unknown (`NULL`) one, so a caller that does not
  know the identity, page count or metadata cannot destroy what an earlier caller established.
  Those semantics are the parent's decision on a question the plan left open; they are recorded here
  because T6 is the first caller and will depend on them.
- **TDD evidence for the fix**: the two added tests (`a_document_that_gains_an_identity_keeps_its_row_and_its_progress`,
  `an_upsert_never_erases_known_values_with_unknown_ones`) were each falsified by temporarily reverting
  the corresponding fix — both failed, the other 12 passed — then the file was restored to a byte-identical
  hash. The S1 test now also asserts the **documented columns** of all six tables, not only their names.
- **Open, unverified**: the declared MSRV (`rust-version = "1.80"`) was never exercised against
  `rusqlite 0.32` / `libsqlite3-sys 0.30`; the build is green only on the installed toolchain.
- **A second verification round found one more should-fix**: the read-then-write in `upsert_document`
  is not a transaction, so two connections to the same database could both see an identity as absent
  and both insert it. The invariant is now enforced by the **schema** — a partial unique index over
  the identity columns — so the loser of that race gets an honest `OctError::Storage` instead of
  silently forking a document. The residual is deliberate and recorded: writers are not serialised
  with an immediate transaction and the failure is not retried. The honest statement of why: `Store::open`
  is called by tests only today, so **no production caller exists that could race this**, and inventing
  a retry policy before the first writer exists would be guessing. The moment a second writer does exist,
  this line is the reminder that it was accepted deliberately.
- **A trap worth remembering, caught by the test rather than by review**: a plain composite unique
  index over nullable identity columns constrains **nothing**, because every row leaves the other
  kinds' columns `NULL` and SQLite treats each `NULL` as distinct — two identical `posix` rows would
  both be "unique". The index therefore normalises `NULL` to an empty string inside the index
  (`COALESCE`) and is partial (`WHERE file_id_kind IS NOT NULL`) so identity-less rows stay
  unconstrained, as the path-supersede rule requires. The test that proves the constraint is live did
  fail against the naive version before this was understood.
- **T5 correction during T6 (2026-10-12), and the first real migration.** T6's first caller made a
  second invariant worth enforcing in the database: a watched folder is recorded once. The first
  attempt edited `SCHEMA_V1` in place, and independent verification caught the real problem with that:
  **V1 is frozen**, so a database an earlier build had already created would report version 1 and never
  gain the constraint, while the code claimed it had. The constraint is now `SCHEMA_V2` and
  `CURRENT_SCHEMA_VERSION` is 2, applied by an explicit one-step-per-version ladder, each step in its
  own transaction so an interrupted migration retries the same step rather than finding half a schema.
  A unit test builds a genuine version-1 database from `SCHEMA_V1` itself, migrates it, and asserts the
  constraint is live and the older build's folder row survived — which also proves the migration
  mechanism every future schema change will use.

**L4 — T6 started (2026-10-12):**

- Specs S1–S7 written above before any code, because the previous task's stale spec was the thing that
  nearly sent a worker in the wrong direction.
- **Fixture reconnaissance done by the parent first, not handed to the worker as an unknown.** `qpdf`,
  `mutool` and `pdftk` are absent; `ghostscript` is present but its `pdfwrite` device produces **no
  object streams** at all (`CompatibilityLevel=1.7`; 0 occurrences of `ObjStm`, `/Type /Page` still
  plain text), so it cannot produce the R2 fixture. The fixture is therefore built directly and
  committed alongside its generator, with its page count confirmed by `pdfinfo` (poppler) — an
  independent implementation, the same cross-check discipline used for T1 and R12.
- **Empirical evidence planned for T6**: scan the maintainer's real library into a throwaway database and
  compare every page count against `pdfinfo`. Unit tests prove the parser path; the corpus proves it on
  real documents, including the pathological ones. That means reading outside the repository, so it was
  asked for explicitly rather than assumed.
- **Consent and scope, as given**: the user authorised a **sample of about 20 PDFs**, not the whole tree.
  The sample is drawn from `/home/tomas-cuevas/Documents/Mega/Libros` (42 PDFs there) and is deliberately
  loaded with the worst cases — `pdfinfo` ground truth already captured for a 436-page/142.8 MB file, a
  708-page/64.1 MB file and a 313-page/93.6 MB file, because that is where a page-counting bug would
  hide. The "192 PDFs" figure elsewhere in this document came from T3's wider sweep of the machine, not
  from this root; the sample is what was authorised and the evidence will say so rather than implying
  full coverage.
- **Fixtures validated twice, by two independent implementations**: `pdfinfo` (poppler) and `lopdf 0.36.0`
  both report 3 pages for `pages-in-object-stream.pdf` and 4 for `with-info-metadata.pdf`, and `lopdf`
  extracts `A Known Title` / `A Known Author` from the second. The R2 fixture is therefore known to be
  both valid and defeating: a naive byte scan of it finds **0** pages against a true count of 3.
- **Independent verification of T6 found three should-fix issues, all fixed before the first commit**:
  (1) the in-place `SCHEMA_V1` edit above, replaced by a real V2 migration; (2) symlinks were skipped
  silently while S5 required reporting what was skipped, so `Discovery` and `ScanReport` now carry a
  `skipped` list, which is deliberately **not** the failure list because nothing went wrong;
  (3) S4's wording claimed a failed document "stays `None`", which is false for one that had been
  counted before — the behaviour (preserve the last known count) is right and the sentence was wrong,
  so the spec was corrected and a test now pins the interaction with T5's `COALESCE`. Two further
  verifier notes were accepted as documented limitations rather than changed: an identity-provider
  failure stores the document without identity and resolves it by path (a document that moves while
  its identity is unavailable becomes a new row), and `Store::document_count()` exists mainly to make
  "no duplicates on rescan" provable through the public API.
- **RED evidence for the parent's fixes**: each new test was falsified by reverting its fix — the
  folder constraint test failed with `got Ok(1)` against the unconstrained schema, and the
  unrepresentable-name test failed because the file was skipped — then both files were restored to
  byte-identical hashes.

**L5 — T6 blocked on its parser, and the plan changed (2026-10-12):**

The empirical run found something the fixtures could not: `lopdf` is **not viable** for a real
library. The spike then measured it properly, in release builds, against `pdfium` — full method,
tables and reproduction steps in [`spike/parser/FINDINGS.md`](../../spike/parser/FINDINGS.md):

| Document | Size | `pdfinfo` | `lopdf` (release) | `pdfium` |
|---|---|---|---|---|
| Learning Patterns (Quartz/macOS) | 142.80 MB | 436 | 26.3 s / 217 s CPU / 290 MB | **0.0 s / 6.2 MB** |
| gentleman-programming-book-es (pdf-lib) | 93.58 MB | 313 | **TIMEOUT (60 s)** | **0.1 s / 22.5 MB** |
| Cracking the Coding Interview (Quartz) | 64.11 MB | 708 | **TIMEOUT (60 s)** | **0.0 s / 9.5 MB** |
| Historia del Pensamiento III | 34.66 MB | 1015 | 16.8 s / 110 s CPU / 202 MB | **0.0 s / 7.9 MB** |
| Carl G. Jung - El Hombre y sus Simbolos | 34.60 MB | 314 | 0.9 s / 5.6 s CPU / 76 MB | **0.0 s / 6.7 MB** |
| Grokking Data Structures (Adobe) | 17.65 MB | 282 | 1.9 s / 12.8 s CPU / 66 MB | **0.0 s / 6.6 MB** |
| El extranjero - Albert Camus | 0.77 MB | 66 | 0.0 s / 5.6 MB | **0.0 s / 6.1 MB** |

**Both parsers returned the exact `pdfinfo` count wherever they returned one.** The difference is cost
and completeness, not accuracy: `pdfium` is correct on **7 of 7** in ≤ 0.1 s with a flat 6–22 MB
footprint, while `lopdf` **never finishes on 2 of 7** and costs up to 26.3 s of wall time, 217 s of
CPU and 290 MB of memory on the rest.

**A correction, recorded on purpose.** An earlier sweep in this session reported *four* documents as
"> 60 s, killed", and the live scanner stall was real, but **both ran debug builds**. Re-measured in
release, two of those four finish (26.3 s and 16.8 s). So the accurate statement is **2 of these 7
exceed 60 s**, and the "4 of 42 documents never finish" figure this entry first carried is
**pessimistic and must not be inherited**. The conclusion is unchanged — a parser that cannot finish
two documents out of seven, and costs three orders of magnitude more on the ones it does finish, is
the wrong foundation — but the number had to be corrected rather than passed along.

What it is **not**: not size (34.66 MB fails where 34.60 MB takes 0.9 s), not a broken cross-reference
(all carry a valid `startxref` and `xref`), not an encrypted document, and not a scanner bug —
`pdfinfo` reads every one of them instantly. The shape of the problem is that `Document::load` builds
the whole object graph — every image stream included — while this product only needs the page tree and
the Info dictionary. `pdfium` parallelises nothing and still wins by doing only the required work;
`lopdf` already uses ~8× parallelism and loses anyway.

**Decision (user): spike `pdfium` against `lopdf` before committing to either.** The plan named
`lopdf` for T6; that is now a measured question rather than an assumption, so T6 was **not closed** at
that point (it was closed by L6 below). `pdfium` is the FFI escape hatch this document already
reserved, it is the engine Chrome renders with, and it is BSD-3-Clause (GPL-3 compatible).

**Spike bounds, because this is the user's machine:** one bounded measurement per (parser, document),
per-document timeout of 60 s enforced by killing the **process group** — a stopped job does not kill
its grandchildren, which was learned the hard way here when a probe kept burning 6.6 cores after its
job was stopped. Total wall budget around ten minutes, and the heavy run happens only after the user
says go. Ground truth stays `pdfinfo` (poppler), an independent implementation.

**Interim state, recorded so it is not mistaken for finished work**: the T6 scanner exists,
63 workspace tests pass, clippy is clean, and independent verification's three should-fix findings
are fixed (a real V2 migration instead of editing `SCHEMA_V1`, a `skipped` list so symlinks are not
invisible, and corrected S4 wording plus a test pinning that a known page count survives a later
read failure). It is deliberately **uncommitted**, because its parser may be replaced.

**L6 — T6's parser replaced, and the swap verified (2026-10-12):**

- `oct-scan` now counts pages and reads metadata with `pdfium`, loaded at runtime from
  `$OCT_PDFIUM_LIB` → `vendor/pdfium/lib` → the system library. `lopdf` survives only as a
  **dev-dependency that builds the encrypted test fixture**; it never counts pages. The resolver is a
  pure function taking the explicit value as a parameter, so resolution is tested without mutating
  process environment.
- **Full-library cross-check, run by the parent and not accepted from the worker's report**: all 42
  PDFs under `/home/tomas-cuevas/Documents/Mega/Libros` scanned into a throwaway database, every page
  count compared against `pdfinfo` — **42/42 agree, 0 mismatches, 0 documents without a count, 0
  extras**. The whole library indexes in **1.5 s** in a debug build; the same library stalled for
  roughly ten minutes under `lopdf`.
- **Two corrections to the parent's own brief**, both surfaced by the worker and both fixed by the
  parent before verification:
  1. The brief required `discover()` to surface a missing library. That was wrong — discovery is a
     filesystem walk and must keep working when page counting cannot run at all. `discover()` no
     longer touches pdfium; `scan()` and `count_and_persist()` do, before anything is recorded.
  2. The brief said to report a missing library as `OctError::NotSupported`, which mislabels it: page
     counting **is** implemented on every platform `oct` targets, it is the native component that is
     absent, and telling a Linux user that Linux is unsupported is exactly the kind of lie this
     project refuses elsewhere. A new additive variant,
     `OctError::ComponentUnavailable { component, message }`, carries the loader's own cause, so "the
     configured path does not exist" and "the library is the wrong architecture" are different
     messages instead of one shrug. Both remedies stay in the message.
- **RED evidence for both fixes, independently**: restoring the `discover()` coupling failed
  `discovery must not need the pdfium library`, and reverting the variant failed with `expected
  OctError::ComponentUnavailable, got NotSupported`. Each file was restored to a byte-identical hash
  afterwards.
- **Independent verification of the whole T6 change set: S1–S7 all `satisfied`**, and it reproduced
  the headline number on its own rather than accepting the parent's — 42 documents scanned, 42
  agreements with `pdfinfo`, 0 mismatches, 0 without a count, 0 extras. Two should-fix findings, both
  documentation drift caused by this session rather than by the code, both fixed:
  1. T6's task line and spec S2 still named `lopdf` while the implementation uses `pdfium`; the
     acceptance text and the status now match the delivered code.
  2. The test suite needs the native library and a fresh clone would not know it. That is now stated
     in the crate docs, the test module, and the README, together with the one command that fixes it.
     The alternative — skipping those tests when the library is absent — was rejected on purpose: a
     test that silently checks nothing is worse than one that says it cannot run.
- Also noted by the verifier and left as accepted: `OCT_PDFIUM_LIB` set to a bad path fails rather than
  silently falling back to the vendor or system library, which is the honest reading of an explicit
  override, and `pdfium` is bound once per parsing call rather than held, which serialises concurrent
  scanners through pdfium's own process-global lock instead of racing it.

**L7 — T7 scoped and split (2026-10-12):**

- T7 was the least specified task in this plan — one line, no contract — so S1–S7 were written before
  any code, including the parts the plan left open.
- **Split into T7a (the mechanism) and T7b (the wiring into the library)**, deliberately: one delegation
  doing both is a change nobody can review in one pass, and the mechanism is independently testable
  without the store.
- **Decisions taken here, recorded rather than assumed:**
  1. A **sixth trait**. "React to a watched folder changing" is an OS-specific capability exactly like
     the other five, and this architecture says every OS-specific source lives behind a trait rather
     than scattering `#[cfg]` through the product.
  2. The **`inotify` crate**, not a cross-platform wrapper. The plan's own wording is "Linux; per-OS
     backend later"; a wrapper would hand the mechanism to a dependency and quietly make the per-OS
     backends unnecessary — which may be the right call later, but it should be a decision, not a
     side effect.
  3. **A file is reported when it is settled, not while it is being written.** Scanning a half-copied
     PDF would report a parse failure and leave a document that is perfectly fine one second later
     sitting in the library with no page count.
  4. **A removal does not change the library.** Deleting the row would destroy the reading history that
     is the entire point of the product, and representing "the file is gone" needs `documents.status`,
     which this document already defers with the other scanner metadata columns. The watcher reports
     the event; representing absence is a later task with its own migration.
- Left to T7b's spec on purpose: how the application consumes events. T7a's constraint is only that the
  loop is blocking and belongs on the caller's thread, because the workspace has no async runtime and
  T7 does not introduce one.

**L8 — T7a delivered, and its review found the same defect class twice (2026-10-12):**

- T7a landed: the sixth trait `FolderWatcher` (with `WatchTarget`, `WatchEvent`, `WatchEventKind`) in
  `oct-core`, a real inotify backend in `oct-platform`, an honest `NotSupported` stub elsewhere, wired
  into `Backends`. 77 tests pass (was 69), clippy clean, and the non-Linux stub type-checks for
  windows-gnu and aarch64-apple-darwin.
- **The worker deviated from the spec on purpose, and was right.** S3 listed `IN_CREATE` as a settle
  trigger; emitting there announces a file while its writer still holds it open, which the same spec
  forbids two clauses earlier. `IN_CREATE` is observed only to classify the following close as `Added`
  rather than `Changed`, and to install watches for new subdirectories. The trade-off is recorded: a
  file created and never written or closed is not reported. **The spec was wrong, not the code**, so S3
  is corrected above.
- **A licence in the task brief was wrong**: `inotify` is **ISC**, not MIT. Permissive and GPL-3.0
  compatible, and now recorded in the workspace manifest and in the licensing table above.
- **The parent review found the same defect class twice, both of them residuals the worker had flagged
  rather than hidden**: the watcher had no way to report its *own* blind spots. A subdirectory it could
  not read was skipped in silence, and `IN_Q_OVERFLOW` — the kernel dropping events — was ignored.
  Either leaves the library stale with nobody told, which is the failure this project exists to
  prevent. Both are being closed by reporting them: `WatchEventKind` gains `Lost` (events were dropped;
  rescan this path) and `Unwatchable` (this path cannot be watched, so changes inside it are
  invisible). `ENOSPC` stays **fatal** on purpose — it means *nothing more* can be watched, so one
  honest error beats a silently partial watch — while one unreadable corner is reported and the rest
  keeps updating.
- The worker also reported honestly that the parent's own phrasing — "a burst of several files produces
  fewer events than files" — cannot hold for *distinct* paths under per-path coalescing. The tested
  property is the stronger and honest one: repeated settles of the same path collapse to one event, and
  every distinct path still appears.

**L9 — T7a verified twice, and the verification cap was reached (2026-10-12):**

- T7a landed: the sixth trait `FolderWatcher` with `WatchTarget`/`WatchEvent`/`WatchEventKind` in
  `oct-core`, the Linux inotify backend in `oct-platform`, the honest `NotSupported` stub, wired into
  `Backends`. **85 tests pass**, clippy clean, and both non-Linux targets type-check
  (`x86_64-pc-windows-gnu`, `aarch64-apple-darwin`).
- **Verification attempt 1 found a blocking defect**: a watched directory moved *outside* a target
  stayed watched. inotify watches the **inode**, so after `mv root/sub /elsewhere/sub` the loop kept
  watching `/elsewhere/sub` and composed its events with the **old** path — monitoring outside the
  nominated tree and reporting paths that no longer exist. Two should-fix findings came with it:
  per-entry traversal errors were dropped in silence, and a symlink supplied *as a target* was
  followed. All three were fixed.
- **Verification attempt 2 settled the previous round's S2 and S5**, and found the defect that closes
  this story: `release_subtree` emptied watches, `seen` and `created` but **not the coalescing
  window**, so a settled event inside a subtree that had just left could still be delivered. Fixed by
  the parent — the window is emptied for the released subtree — with a test whose RED run delivered
  exactly that stale event.
- **The two-attempt verification cap was reached here, and this is the honest state of the record:**
  the last delta (the pending-window fix) has the parent's own checks and a RED-pinned test, but **no
  third independent pass**. It is one `retain` and one test, both falsified. A reader should know that
  rather than assume the whole artifact was re-verified after the fix.
- **Accepted residual, with reasoning**: `created` — the set that distinguishes `Added` from `Changed`
  — keeps an entry for a file created and never closed, so it can grow with the number of such files.
  Left as is deliberately: the alternative, expiring entries, would report a slow copy that takes
  longer than the window as `Changed` instead of `Added`, and **a wrong fact is worse than a small set
  of paths**. Revisit this line if a second writer of the same folder ever appears.
- **A small evidence gap, recorded rather than papered over**: Rust's test harness has no "skipped"
  state, so the unreadable-directory test that cannot create its condition (running as root) passes
  with its reason visible only under `--nocapture`. A green run does not prove that branch executed.
- **Also learned**: a verification subagent ran `codegraph init` inside the repository and left
  `.codegraph/` behind. It was removed by hand. A read-only verifier that writes to the tree it is
  verifying is a contradiction, and the brief for the next one says so explicitly.

**L10 — T7b scoped (2026-10-12):**

- T7b is the half of T7 that touches the library, and it was specified only once T7a's mechanism
  existed, so the contract it wires is real rather than imagined. S1–S6 above.
- **Decisions taken here, recorded rather than assumed:**
  1. **A removal does not touch the store.** The event is reported and that is all. Deleting the row
     would destroy the reading history that is the product's whole point, and representing absence
     needs `documents.status`, which is deferred. This deliberately repeats L7, because it is the
     decision a future contributor is most likely to "fix" without having read why.
  2. **`Lost` is rescanned; `Unwatchable` is not.** Dropped events are recoverable by looking again; a
     path that cannot be watched is not, and rescanning it would only produce a second identical
     failure.
  3. **No batching layer.** Events already arrive coalesced per path and a document costs about a
     millisecond to count (T6 measured 1.5 s for 42 real documents), so a second buffering window
     would be complexity with no bottleneck to justify it.
  4. **Scan first, then watch** is the caller's order, written into the spec because T7a cannot see a
     change that happens before its watches are installed — the startup race it documents.

**L11 — T7 closed (2026-10-12):**

- T7b landed: `WatchBridge` consumes settled watcher events and updates the library, with one sink for
  every outcome. **93 tests pass** (was 85), clippy clean, `oct-scan` still has no `oct-platform`
  dependency, and independent verification marked **S1–S6 all satisfied with no findings** — the first
  clean pass in this task, after T7a needed two rounds.
- The four per-event decisions were verified as behaviour rather than as comments: `Removed` reaches no
  store call at all, and its test asserts the row, the page count **and** the recorded progress all
  survive, so a future edit that deletes a row fails loudly; `Lost` reports and rescans while
  `Unwatchable` reports and does not, because reading a path cannot make it watchable.
- **T7 is complete**: the watcher exists, it reports its own blind spots, and the library is wired to
  it. What remains of Phase 1 is T8 (`/proc` open-document detection, axis B), T9 (the provider registry
  plus the Evince GVFS adapter) and T12 (`recently-used.xbel`, axis A) — then the tray and the UI.
- **The residual from T7a still stands and is not quietly absorbed by this entry**: the watcher
  mechanism's last delta is parent-checked and RED-pinned but was never independently re-verified,
  because the two-attempt cap was reached. Everything built after it — this task — was verified
  normally.

**L12 — T8 closed (2026-10-12):**

- T8 landed: `ProcOpenDocuments::open_documents` is a real `/proc` snapshot. **102 tests pass** (was
  93), clippy clean, the non-Linux stub still returns `NotSupported` and type-checks for both non-Linux
  targets, and independent verification marked **S1–S7 all satisfied with no findings**.
- **The tests can be deterministic because the test process can hold a file open itself**: the detector
  is asked for a snapshot while the test holds a PDF fixture, and must report *its own* path and pid.
  Containment, not equality — other applications on this machine may legitimately have PDFs open, so an
  exact-vector assertion would have been a flake dressed up as a test.
- **The parent rewrote one assertion, and the reason is worth keeping**: the determinism test compared
  two whole-system snapshots. That cannot be made reliable — sibling tests in the same binary hold PDFs
  open while they run, and so does anything else on the machine. Deduplication and ordering are now
  asserted over the **full** snapshot (both deterministic), and stability over the paths the test
  controls. The verifier confirmed this did not weaken it: duplicates, unsorted output, or a dropped
  controlled entry all still fail.
- **Decisions recorded rather than left implicit**: a deleted descriptor target is skipped, because
  reporting it would suggest a path that no longer exists; the noise is reported rather than filtered
  (R4); a `.pdf` name must also resolve to a regular file, so a symlink called `report.pdf` pointing at
  a directory cannot slip through; and an unreadable `/proc` root is an error while per-process failures
  are skips, because an empty snapshot is indistinguishable from a system with nothing open.
- **Phase 1 has two tasks left**: T9 (the provider registry plus the Evince GVFS adapter — the first
  real `ProgressProvider`, `Live`, and the first caller of `Launcher`) and T12 (`recently-used.xbel`,
  axis A). Then the tray and the UI.

**L13 — T9 scoped, and the UI order changed at the user's request (2026-10-12):**

- **Reconnaissance ran before the specs, and it settled two things.** `gio` is present and reads
  `metadata::evince::page` at **8.7 ms/file** — 0.37 s for the whole 42-document library, consistent with
  T3's 10.0 ms/file — and **26 of those 42 documents carry a real position**. So the subprocess
  mechanism the plan assumed is viable at library scale, and the trait's per-document shape is
  affordable. Parsing the GVFS store directly was rejected: that format is undocumented and
  version-fragile, which is exactly the adapter fragility this project treats as a liability (R10).
- **The provider id is frozen at `evince-gvfs`.** Stored progress rows reference it, and the trait docs
  and T5's tests already use that exact string; changing it later would orphan real rows.
- **The off-by-one from T1 gets a round-trip test, not a comment.** `read` keeps the 0-based index, and
  `resume` converts to the 1-based CLI argument exactly once, at the boundary.
- **The Linux `Launcher` (axis C) is implemented in T9**, because the Evince adapter is its first
  caller: `xdg-open` for "just open it", a viewer CLI for "open at this page", and an honest error when
  no viewer is present.
- **UI order changed, at the user's request.** The plan put the UI in Phase 4, after the tray. The user
  wants to judge the architecture early, and there is a concrete reason to agree: **the store has no
  library-listing query at all** — T5 deferred it explicitly because nothing called it — so the first UI
  screen is what will force that contract into existence. A thin vertical slice therefore moves to
  immediately after T9, ahead of T12 and the tray. T9 still goes first because it is what makes the
  screen show the *product* (real reading progress) rather than only the plumbing.
- **A decision the user owns, not this session**: the frontend framework. T4 deliberately left the web
  scaffold as plain TypeScript + Vite, and the plan says React / Svelte / Solid must be settled before
  the UI phase because it changes who can contribute to an open-source project. It will be brought with
  concrete trade-offs rather than decided unilaterally.
- **A constraint discovered while scoping T9, worth writing down**: the only way to *create* a GVFS
  metadata record is `gio set`, which writes to the user's own metadata store. The tests must not do
  that, so they exercise the real subprocess against a file with no record (`Ok(None)`), cover the
  value path with the pure parser, and the real end-to-end read is checked by the parent against the
  maintainer's library instead of being faked.

**L14 — T9 closed, and the product's headline claim is real (2026-10-12):**

- T9 landed: the Evince GVFS adapter and the Linux `Launcher`. **120 tests pass** (was 102), clippy
  clean, both non-Linux targets type-check, `oct-core` untouched, no new dependency. Verified twice:
  attempt 1 found a panic path (`std::thread::spawn` in the child reaper, which panics when the OS
  cannot create a thread) and marked S7 unsatisfied; after the fix, attempt 2 marked **S1–S7 all
  satisfied with no blocking or should-fix findings**.
- **The empirical check, run by the parent with the user's explicit consent and metadata only**: 42
  PDFs, **26 with a recorded position**, the adapter read all 26 with **0 errors**, and **26/26 agree
  exactly with raw `gio` output** — 0 mismatches, 0 missing, 0 extra. That also pins the *read* side of
  the off-by-one: had the adapter added one, all 26 would have failed. Three documents legitimately
  store page `0` and come back as `Some(0)`, never as `None`.
- **The spec was wrong again, and the worker caught it again.** S5 said a missing page-capable viewer is
  an honest error; the `Launcher` trait's own documented contract says the opposite — a launcher that
  cannot honour a page must still open the document, because landing on page one is a degraded success.
  The worker followed the spec and **flagged the conflict instead of quietly choosing**, which is the
  behaviour this project wants. The fallback was restored with a test whose RED run showed it missing,
  and S5 was rewritten.
- **A limitation recorded rather than hidden**: `LaunchOutcome` cannot yet say "opened, but not at the
  page you asked for" — the viewer name is the only signal. Adding a variant is additive and cheap, and
  should happen whenever a caller needs to tell the user their viewer cannot be asked for a page.
- **The tests cannot create a real GVFS record without writing to the user's own metadata store**
  (`gio set`), so they exercise the real subprocess against a file with no record, cover the value path
  with a pure parser, and the real end-to-end read is the parent's check instead of a fake.
- **Phase 1 is complete**: all six platform traits now have their Linux implementations. What remains
  before a first usable product is the UI slice — next, ahead of T12 and the tray — and then T12 itself.

**L15 — The framework is decided, and the UI slice is specified (2026-10-12):**

- **The user chose Svelte 5**, and their stated experience is React. Both are recorded, because the
  tension between them is real: the plan says the contributor pool matters for an open-source project,
  and the person maintaining this code has not used Svelte in anger. The decision is theirs and it
  stands; it is written down so that a later session sees it was a deliberate choice rather than an
  accident.
- **The argument that did *not* decide it**: bundle size. A framework's few tens of kilobytes are noise
  next to pdf.js, which this product will ship anyway for the first-party reader. The real criteria
  were the contributor pool, the component ecosystem, and ergonomics for a UI of this size — and the
  user weighed them toward less code.
- **The UI slice moves ahead of T12 and the tray**, for the reason given in L13: the store has no
  library query, and the first screen is what forces that contract into existence. Writing it now means
  discovering whether the schema and the domain types are enough, while the cost of changing them is
  still small.
- **What the slice deliberately leaves out** is as important as what it includes: filters, the
  capabilities screen, manual override, the suggestion queue, the tray and the reader. The slice proves
  the spine end to end — store, provider, IPC, pixels — and nothing more.

**L16 — The first UI slice is built, and the user can look at it (2026-10-12):**

- The slice landed: `Store::library()` — the listing T5 deferred until a caller existed — three Tauri
  commands over plain testable functions, and one Svelte 5 screen. **129 workspace tests plus 13 vitest
  tests pass**, clippy clean, the frontend builds to ~19 KB gzip, and the app launches.
- **The product-critical rule is enforced in code, not in a comment**: the screen states progress with
  its provenance in words — "p. 12 of 80, recorded by Evince" — and no path can render a bare
  percentage or a fabricated `0%`. A row with no total renders without an "of" clause rather than
  inventing a denominator, and the stored 0-based page becomes 1-based only for display.
- **Verification found one should-fix, and it was this project's own principle aimed at a screen**: the
  provider list started empty, so the UI announced "no progress providers are available" **before the
  registry answered** — asserting something it did not know. `null` (still checking) and `[]` (genuinely
  none) are now different facts, each with its own message, and a test asserts both. The second
  verification round settled it.
- **S5 is recorded as unverifiable, and the cause is this session's own doing**: the parent launched the
  app for the user to look at *while the verifier ran*, so the verifier could not tell its own launch
  apart from the pre-existing process. The worker's isolated launch check had established it beforehand
  (exit 124 under `timeout`, empty stderr, no process left behind, and a database created only by the
  mount-time IPC invoke). The lesson is cheap and general: **do not run the artifact while its
  verification is running.**
- **A caveat worth knowing**: outside Tauri — a plain browser — `onMount` returns before the provider
  request, so the screen keeps showing "checking…" rather than falsely claiming there are none. That is
  the right direction to fail in.
- **What the slice deliberately leaves out** stays out: filters, the capabilities screen, manual
  override, the suggestion queue, the tray and the reader. What it proves is the spine, end to end.

**L17 — The user judged the slice, and asked for eight changes (2026-10-12):**

Verbatim, because these are requirements and not a wish list:

> 1. No me gusta que el usuario tenga que pegar la URL de la carpeta. Deberia abrirse un explorador con un boton, ahi es donde el usuario navegaria hacia su carpeta y pondria "ok". El colocar la URL quedaria como una opcion secundaria que se desplegaria cuando el usuario haga click en algun boton que indique "Colocar URL manualmente" o similar.
> 2. Hay archivos que no muestran correctamente su titulo y en su caso, aparece la URL completa del archivo: /home/tomas-cuevas/Pictures/Screenshots/Screenshot from 2026-10-13 12-39-52.png
> 3. El listado de libros deberia ser una grilla de 3 columnas.
> 4. No se muestran las portadas de los libros.
> 5. Tendria que haber un boton en la card que te lleve directamente a la carpeta donde esta el PDF en cuestion.
> 6. Se tendria que poder filtrar, ordenar.
> 7. Tendria que haber varias pantallas. Una inicial donde se podrian mostrar los 3/4/5/6 libros que uno tenga en progreso junto con otros datos como libros que hayas completado al 100%, libros que hace tiempo no se leen y asi.
> 8. Debe haber un navbar inferior para navegar entre las diferentes pantallas (Inicio, Biblioteca, Configuracion)

**Diagnosis, done before designing anything (reproduce first):**

- **Item 2's first half is a real defect, found in the code**: the card renders `{entry.title ?? entry.path}`, so a document whose PDF carries no embedded title — **12 of the maintainer's 42** — shows its **absolute path where a title belongs**. A path is not a title, and the honest rendering is the file's own name, marked as having no embedded title.
- **Item 2's second half could not be reproduced, and is not being assumed either way**: the app's database holds **42 documents, all PDFs under `Libros`** — no PNG, and the scanner filters by `.pdf` extension, so a `.png` cannot enter through a scan. The example path is a GNOME screenshot taken today, so the likely explanations are something typed or pasted into the folder field (the error message quotes the path) or a screenshot of the app itself. **The user's repro is needed before this half is called a bug**, and it is asked for rather than guessed at.
- **The round is a requirement change, not a polish pass**, and it is tracked as such: several items need capabilities that do not exist yet (item 1 a native folder chooser, item 4 page rendering to images, item 7 queries for completion and staleness), and item 2 is a defect to fix first.

**Sequencing, and why:**

1. **The defect first (item 2's first half)**: a path shown as a title is a lie about the data, and it is cheap.
2. **The structural UI (items 3, 5, 6, 8 and the screens of item 7)**: a three-column grid, a card action that opens the containing folder, filtering and sorting, and a bottom navbar across Inicio / Biblioteca / Configuración. This forces the store's listing to grow — the plan deferred filtering and sorting "because nothing called them", and now something does.
3. **The home screen's numbers (item 7)**: what is in progress, what is finished, what has not been read in a long time. Note what the data can and cannot say today: `progress.updated_at` is real and written, so "not read in a long time" is answerable; `documents.last_opened_at` **exists but nothing ever writes it**, so it must not be used as if it meant something.
4. **Covers (item 4) last, and measured before it is built**: rendering page one needs pdfium's *render* path, which this project has never used — R12 measured pdf.js in a browser, not pdfium's render API. So it gets the treatment every uncertain capability has had here: a bounded measurement first, then the feature. Caching and where the images live are part of that task.
5. **The folder chooser (item 1)**: a native picker needs Tauri's dialog plugin, which is a new dependency and therefore a decision to record rather than slip in. The manual path entry stays as the secondary, collapsed option the user asked for.

**Not forgotten**: the string catalogue must gain every new label in both languages, and each new screen must keep the rule that reading progress is stated with its provenance rather than as a bare percentage.

**L18 — The user's clarifications and two new requirements (2026-10-12):**

- **The PNG mystery is closed, and it was not a bug.** The user clarified that the path in their
  report was a screenshot they took *to show* the URL-as-title defect. That matches what the evidence
  said — 42 documents in the database, all PDFs, and a scanner that filters by extension — and it is
  why the second half of item 2 was asked about instead of being assumed into a fix. **Reproduce
  first, and when the evidence says "not a bug", ask rather than patch.**
- **Item 2's fix, as the user specified it**: a title with no embedded value comes from the file's own
  name — everything after the last separator, minus the extension. Shipped with three tests pinning
  the cases that bite: dots in a *directory* name, dots in the file's own name, no extension, and a
  leading dot. The fallback is deliberately not badged on the card: the file's name is the ordinary
  convention, and "this document has no title of its own" belongs in the title editor where the reader
  can act on it.
- **New requirement 1 — user-editable titles.** The reader can set a title from inside the app. This is
  not a text field: a title extracted from the PDF and a title a person typed are **different facts**,
  exactly like progress from Evince versus progress typed by hand. So it needs a migration (V3) and
  the same provenance discipline the `progress` table already applies — otherwise the interface cannot
  say whether it is showing the document's title or the reader's, and the first time it guesses wrong
  the reader stops trusting every other field.
- **New requirement 2 — covers, expanded.** The default cover is the document's first page, and the
  user must be able to choose another page or upload an image of their own. That makes it three things
  rather than one: a **rendering path** (pdfium's render API, never used in this project — R12 measured
  pdf.js in a browser, not this), a **storage decision** (rendered and uploaded covers live somewhere,
  and an uploaded image is user data that must survive a rescan), and another **migration** for the
  per-document choice. The bounded measurement comes first, as it did for every uncertain capability
  here.
- **Sequencing unchanged**: the title defect first (done), then the structural UI (grid, folder action,
  filtering, navbar, screens), then the home screen's numbers, then covers, then the folder chooser.
  The two new requirements slot in as: editable titles with the structural UI (it is a per-card action),
  and covers last with their measurement.

**L19 — The UI shell is specified, and one open question is decided (2026-10-12):**

- Items 3, 5, 6 and 8 of L17 are one work unit: the app shell. The navbar and screens, the library as a
  three-column grid, the per-card action that opens the containing folder, and a Configuración screen
  that shows real state instead of a settings list nobody has implemented.
- **Decided, because the plan left it open: filtering and sorting happen over the listing the screen
  already holds, not in SQL.** The plan deferred store-side filtering "because nothing called it"; now
  something does, and the honest answer is still that 42 documents — or a few thousand — do not need a
  query language. **The limit is recorded with the decision**: the listing is not paginated, so the day
  a library is big enough to matter, the store grows and that is what triggers it. A decision that
  states its own expiry is worth more than one that pretends to be permanent.
- **The home screen is split on purpose.** The navbar needs an Inicio destination now, but its numbers
  (in progress, finished, long unread) need queries and a decision about `last_opened_at` that nothing
  writes — so Inicio ships as an honest placeholder saying what it will show, and item 7 becomes its own
  unit. **A screen that says "not yet" is better than one that shows invented statistics.**
- **Covers keep their placeholder in the grid** rather than a stand-in image, so the layout is real and
  nothing pretends to be a cover that is not.

**L20 — The user asks for a mobile-shaped shell, a home of carousels, and the tray (2026-10-12):**

Verbatim:

> 1. En la pantalla principal, se deben colocar 3 carruseles con PDFs para cada seccion "Documentos en curso", "Documentos terminados" y "Documentos sin abrir durante mucho tiempo". Aparte, debe haber un boton arriba que envie a una pantalla que liste todos los PDFs de la categoria.
> 2. La UI de la aplicacion deberia tirar más para una UI tipo mobile, ya que para este tipo de aplicacion lo veo más convincente. Por el momento, no se podra abrir la aplicacion en pantalla completa ni expandir, tendra un ancho y alto maximo que respeten la proporcion.
> 3. En la parte superior, tendra el boton para minimizar, esconder en el tray y cerrar la aplicacion, no esta el boton para expandir a pantalla completa.
> 4. Las portadas viviran en la carpeta que haz recomendado.

**The trap, said before anything is built**: "esconder en el tray" needs a tray, and this project has
none — the tray is Phase 3 and unbuilt. **A hide button with no tray makes the application vanish with
no way to bring it back**, which is not a feature but a trap. So the minimal tray (icon, show/hide,
quit) lands with the window chrome, in the same unit. It is not decoration; it is what makes the
requested button honest. Phase 0 verified that the reference host supports app indicators, so this is
feasible here.

**Other implications, recorded rather than discovered later:**

- **A custom title bar.** Native decorations carry a maximise button, which the user explicitly does
  not want, so the window is undecorated and the application draws its own top bar. "Not maximisable by
  any route" is part of the criterion, because a window that refuses the button but still maximises on
  a double-click has not honoured the request.
- **A fixed, phone-shaped window.** No fullscreen, no resize: a size that respects a phone's
  proportions. This is a product statement — a phone-shaped companion in the tray, not a desktop window
  that happens to be small.
- **Covers live in `~/.local/share/oct/covers/`** (item 4, the recommendation accepted): inside the
  application's data directory, so a rescan cannot touch them and moving the PDF does not lose them.
- **Inicio stops being a placeholder.** Its three carousels need exactly the queries item 7 always
  needed, and each category gets its own screen. Note what the data can and cannot answer:
  `progress.updated_at` is real and written, so "not opened for a long time" is answerable;
  **`documents.last_opened_at` exists but nothing ever writes it**, so a query built on it would return
  confident nonsense.
- **Every category states its own definition on screen**, because "finished" and "in progress" are
  readings of a stored position rather than facts the document carries.

**L21 — Unit A verified, and one criterion was over-promised (2026-10-12):**

- Unit A landed (`1f60948`): the 390x844 undecorated fixed window, the custom top bar with exactly
  minimise/hide/close, the minimal tray, and one quit path. **138 workspace tests, 49 vitest tests**,
  clippy clean.
- **Verification marked A1 and A3 satisfied, A4 unverifiable, and A2 unsatisfied — correctly.** The spec
  said the window "must not be maximisable by any other route", and on Linux that is not something the
  application can deliver: Tao's GTK backend implements `set_maximizable` as an **empty function**, so
  `maximizable: false` takes effect on Windows and macOS only, and a window-manager shortcut such as
  GNOME's Super+Up still maximises the window. **The spec promised what the platform cannot give, so the
  spec is what changed** — the criterion now covers the application's own affordances plus the flag
  where it is honoured, and the limitation is recorded instead of restated as a guarantee.
- **A claim that stayed single-sourced, and is recorded as such**: the implementer's runtime probe
  reported `inner == outer == 390x844` and `is_resizable() == false`. The verifier did not reproduce it
  and said so plainly, and the configuration plus the runtime re-application are not a substitute for a
  live probe. Its status is therefore *reported, not independently reproduced* — which is the honest
  label rather than promoting a single report to evidence.
- **A stale comment, fixed**: the library grid's comment still said "Three columns" while the rule below
  it is responsive and renders two at phone width. The prioritisation — the later phone-shape request
  wins over the earlier three-column one at 390 px — is now recorded where the rule lives.
- **What still needs a human's eyes**: the tray icon actually appearing, that the shape reads as a
  phone, that the graceful close and the tray's Quit leave no process behind, and whether Super+Up
  maximises. None of that is testable from inside this repository, and saying so is part of the record.

**L22 — The round-2 UI adjustments, in the user's words (2026-10-13):**

> Voy a escribirte algunas cosas que no me gustan y deberian cambiarse:
>
> 1. Cuando coloco el idioma en español, las opciones del tray siguen en ingles.
> 2. No me gusta todo el texto que hay en el header de la pantalla "Inicio".
> 3. En español, no me gusta que diga "Configuraciones" prefiero "Ajustes", ya que es mas corta.
> 4. Falta implementar los carruseles en la pantalla inicio.
> 5. El viewport de la aplicacion debe ser un poco más ancho.
> 6. Debes adaptar los padding tambien, tienes que reducirlos y no hacerlos tan grandes como estan actualmente.
> 7. No me gusta que diga "No hay progreso registrado", prefiero que muestres el porcentaje, osea, tiene que haber una linea que represente el porcentaje y superpuesta a esta debe estar el numero que indica el porcentaje en la posicion que le corresponda.
> 8. No hay separacion vertical entre los elementos en la pantalla "Biblioteca", puedes verlo en esta captura: /home/tomas-cuevas/Pictures/Screenshots/Screenshot from 2026-10-13 16-44-17.png
> 9. El boton para ir a la carpeta contenedora solamente debe tener un icono y debe estar situado arriba en alguna parte, no me gusta donde esta actualmente.
> 10. Debes agregar paginacion en la pantalla "Biblioteca", queda feo que esten todos los elementos ahi, tal vez deban ser 10 elementos por paginacion.
> 11. Sigue sin implementarse el boton para abrir el explorador de archivos para permitir al usuario navegar para encontrar la carpeta donde estan sus PDFs para no obligarlo a pegar la ruta.
>
> Hay otras cosas mas por ajustar de la UI, pero por el momento, esto es todo.

Recorded as `R1`–`R11` and split into units `U1`–`U6` (specs above). Two notes from the same round:
**the application stealing focus when it launches is now its own task**, reported by the user as the reason
they kept closing it ("me molestaba que saltara la aplicacion sobre lo que estaba haciendo"), and **the
self-exit defect is withdrawn** — with the exit paths instrumented the application survived 120 s
untouched with zero exit-path logs, and the user confirmed they were the one closing it. The screenshot
cited in item 8 was checked before any code changed: it is from the running build (binary written
16:36:21, process started 16:36:22, screenshot taken 16:44), so the feedback is about the current tree
twice over — and it shows the rows of the library grid touching, which is `R8`.

**L23 — Round-2 feedback on the first fix, and the cause finally measured (2026-10-14):**

> 1. Ahora por lo menos, las cards no se superponen pero sigue sin haber espacio vertical entre las filas como puedes ver en esta captura: /home/tomas-cuevas/Pictures/Screenshots/Screenshot from 2026-10-14 08-49-20.png
> 2. Si, ahora dice "Ajustes"
> 3. Si, ahora hay menos padding en general, me gusta más. Una cosa que tal vez debas agregar es padding-top sobre el titulo de cada pantalla, ya que actualmente esta muy pegado sobre la barra superior: /home/tomas-cuevas/Pictures/Screenshots/Screenshot from 2026-10-14 08-51-07.png
>
> Respondiendo a las decisiones, el ancho acepto unos 430px y 0% para los documentos que no tengan registro de lectura.

Three things settled here. **R3 and R6 were accepted** — the Spanish label and the reduced padding —
with one refinement on R6: the screen heading needs *more* room above it, not less, and that space is
stated once in the shell rather than in each screen. **R8 was not fixed by the first attempt, and that
attempt's suspect was wrong**: changing `height` to `min-height` stopped the cards overlapping but the
rows still had no space, because the real cause was the card overflowing its grid row by its own padding
and border under `content-box`. It was found by driving WebKitGTK headless — the engine the application
renders in — instead of reading pixels off a screenshot, which is what made the difference after two
wrong guesses. **The two open decisions were answered**: the window goes to **430 px** (accepting two
columns, per the arithmetic) and a document with **no reading record shows 0 %**, which is the user's
explicit call, recorded as theirs, taken against the maintainer's stated preference to leave the number
off in that case. The rule that stands is the one they asked for, with the reasoning on record rather
than applied silently either way.

**L24 — R8 confirmed, and the two gaps made equal (2026-10-14):**

> Si, ahora por fin hay separacion entre las filas, una cosa mala es que no es la misma separacion que hay horizontalmente entre los elementos de una misma fila

**R8 is finally confirmed by the user's eyes**, which is the only verification that counted for a visual
criterion they had already reported twice. Their follow-up is the refinement now inside the same
criterion: the row gap I had widened to 1.25rem was visibly larger than the 0.75rem between columns,
so the grid takes one gap for both axes again — the original design, which the overflow bug had been
hiding. Measured in WebKitGTK 4.1 with the committed rules: **column gap 12 px, row gap 12 px**. The
sequence worth remembering is the one that closed this: two theories argued from a screenshot were both
wrong, and driving the engine the application actually renders in found the cause in one run.

**L25 — The number gets bigger, and the priority is stated (2026-10-14):**

> Obviamente, la UI actual deja MUCHO que desear, es bantante fea, pero por el momento no quiero centrarme en hacer la UI bonita si no que funcional. Lo unico que cambiaria seria que haria el numero un poco más grande, actualmente es muy pequeño como puedes ver en la siguiente captura: /home/tomas-cuevas/Pictures/Screenshots/Screenshot from 2026-10-14 09-14-20.png

Two things to take from this, and the second one governs the whole remaining queue. **The progress
number is now `0.85rem`** instead of `0.7rem`, re-measured in WebKitGTK 4.1 at the new size (all four
cases inside the bar; 100 % clears the edge by 0.1 px). **And the user's priority for now is
functional, not pretty** — they said the interface is ugly and that they do not want to spend this
round on making it beautiful. That is a standing instruction, not a one-off remark: it means unfinished
appearance is not a defect to raise, polish is not to be slipped into feature work, and what matters is
that the thing does what it says. Improving the look is a later phase, and this line is where that
becomes explicit rather than assumed.

**L26 — The commit gate belongs to the user (2026-10-14):**

> Pero hermano, no hagas commits si yo no verifique que me gusta el cambio realizado

**Standing instruction, and it corrects how the work was being landed.** The protocol this document
followed said a tracked task closes with a work-unit commit, and those commits were being taken after
the maintainer's own checks — green tests, clean builds, the application relaunched — but **before the
user had looked**. That made their review retrospective: the commit existed, so the verdict could only
change it afterwards. The gate is theirs and it comes first. The sequence is now **implement → checks →
hand the running application over → wait → commit only on approval**, and its two consequences are
stated above rather than left implicit: the unit waits in the working tree, and only one unit is in
flight at a time.

What does not change: the checks still run **before** the handover, because handing over something
untested wastes their time; file records are still written, and travel in the commit the approval
unlocks. What happens to commits already made under the old order — the changes are in the running
application and were handed over for judgement, and those commits are on `feat/phase-1-core` and
**unpushed** — is the user's call, not the maintainer's: they can stand as they are, with any
displeasing change fixed by a new commit, or the user can ask for them unwound so nothing is committed
before their eyes. Neither is done unasked, and history is never rewritten silently.

**L27 — Two rejections and one addition on U3 (2026-10-14):**

> De U3 hay algunas cosas que no me gustan:
>
> - El icono escojido no me gusta. Tampoco me gusta la posicion ya que queda MUY RARO ese icono ahi arriba suelto como puedes ver en la captura /home/tomas-cuevas/Pictures/Screenshots/Screenshot from 2026-10-14 09-38-49.png
> - Tampoco me gusta que al cliquearse el icono aparezca debajo el texto en verde indicando que se ha abierto la carpeta. Quita ese texto.

> Tambien, la paginacion deberia estar disponible arriba y abajo, actualmente esta abajo unicamente lo que obliga al usuario a hacer scroll hacia el fondo para poder paginar

**What this round is worth remembering.** The user's verdict arrived against a committed unit, which is
exactly the situation the gate in L26 exists to prevent — the commit (45013b0) had been taken on the
maintainer's own checks before they had looked. The three corrections are in the specs above: the action
anchored to the cover's corner instead of a row of its own, a redrawn icon, no success sentence, and the
page controls at both ends.

**Two things were decided by the maintainer rather than the user, and are stated here so they can be
overturned.** (1) The **error** text stays: the request removed the green "folder opened" sentence, and a
failure is the one case where saying nothing would be a lie of omission. (2) The pager is **previous and
next** with "Page X of Y" rather than numbered page buttons, which follows from L25's functional-first
priority; numbered controls are a look decision for the later appearance phase.

**And one honest note about the verification itself**: the placement was measured in WebKitGTK 4.1 rather
than eyeballed (the button sits inside the cover's box, 4.8 px from its top and right — the committed
rule), and the new test for "an opened folder says nothing" failed on its first run **because the test was
wrong, not the code**: it compared raw server-rendered HTML and tripped over Svelte's own comment
markers. It now compares with those stripped, which is what the rule is actually about.

**L28 — The dead space inside a short card, in a row with a longer title (2026-10-14):**

> Hay un pequeño bug que encontre en algunas filas y es que uno de los dos no completa todo su espacio. Segun lo que pude apreciar, esto pasa en filas donde hay uno que tiene el titulo más largo lo que requiere más renglones para mostrar el texto. Pasa cuando los dos elementos de la fila no tienen los mismos renglones en el titulo como puedes ver en la siguiente captura: /home/tomas-cuevas/Pictures/Screenshots/Screenshot from 2026-10-14 09-14-20.png /home/tomas-cuevas/Pictures/Screenshots/Screenshot from 2026-10-14 09-38-49.png

**Their diagnosis was right; the mechanism was not what it looked like, and measuring first is what
told the two apart.** Reproduced in WebKitGTK 4.1 with a two-column row: the card boxes already
stretch **equally** (439.1 px each), so a card was never failing to fill its *box*. What it left empty
was **119.7 px of dead space at the end of its own content**, in the row whose neighbour's longer title
needed more lines — and 50.7 px in the next row, where the titles were one and two lines.

**The fix spends that difference instead of hiding it**: the progress block is pushed to the card's
bottom (`margin: auto 0 0` on `.progress`). Re-measured: **every bar in a row now sits at one height**
(405.7 and 405.7, then 787.8 and 787.8, with 12.2 px below each). That alignment is the point — a
progress screen exists so a reader can compare how far along several documents are, and a row whose
bars sit at different heights cannot be read that way. **The alternative was declined**: letting each
card keep its own height removes the empty space but gives up the alignment and leaves the rows ragged.
**Left as a known consequence**: the space has moved to the gap between the last line of text and the
bar, so a one-line title still shows a gap — smaller than before, and now doing work.

**L29 — The hole was the card's width, not the gap (2026-10-14):**

> No, no se arreglo, el problem sigue ahi y puedes verlo en la siguiente captura: /home/tomas-cuevas/Pictures/Screenshots/Screenshot from 2026-10-14 10-04-53.png

**Two attempts failed before the cause was measured, and both failures are worth keeping**, because they
show what arguing from a screenshot costs:

- **First attempt (L28)** — pushing the progress block to the card's bottom. The user photographed the
  result: the hole had only **moved**, from after the last line of text to above the bar.
- **Second attempt** — clamping the title to two lines, so two cards could differ by at most one line.
  **Measured, and not enough**: the shorter card still carried a 68.8 px hole while one line explains
  only ~24 px. The measurement said so before the CSS was touched again.

**Measuring element by element then found it, and it was not the gap.** The card is a flex item, and
**a flex item does not grow by default**: its width followed its own text. A card with a shorter title
came out **narrower**, and because the cover's height comes from its width (`aspect-ratio: 3/4`), its
cover came out **shorter** — measured in a single row as **186.2 px against 146.0 px**, a 40.2 px
difference that had nothing to do with the gap, pushed every line above the bar and left the hole.

**The fix is one declaration**: `flex: 1` on the card, so it fills its grid column. Measured after:
every card 209.0 px wide, every cover 245.5 px tall, the largest hole **28.6 px** (down from 113.1), the
bars aligned within a row and each bar 12.2 px from the card's bottom.

**The title clamp stays**, and it is a decision the user may veto: it bounds the remaining difference to
one line by clipping a long title visually — the element still holds the whole text for a screen reader,
and the full title is offered to the pointer through `title=`. Without it the residual hole grows to
about two lines.

**What this round teaches, and it is the third time in one session**: a screenshot shows *that*
something is wrong, never *why* — and two plausible mechanisms were both wrong, while the per-element
measurement in the engine the application actually renders in found a cause in a different part of the
layout than either guess.

**L30 — The last text goes, and the commits taken before the gate are unwound (2026-10-14):**

The user answered the three open questions, in their own words as the options they chose:

> 1. **"Sí, commiteá todo lo que está en el árbol"** — the U3 revision, the dead-space fix and the record
>    go in together.
> 2. **"Deshacelos (soft reset)"** — for the two commits taken before their commit gate.
> 3. **"Sacalo también"** — the failure text: *"Ninguna frase: la carpeta se abre o no se abre, sin texto
>    en la ficha"*.

**The failure sentence is gone**, so the card now states nothing about the folder action in any case:
neither the green "folder opened" line nor the red "could not open" one. The `card.openedFolder` and
`card.openFolderError` keys are removed from both catalogues, the `.status` CSS with them, and the test
that used to require a failure to be stated now requires silence in every status — with the reason
recorded as the user's decision, because silence on failure is the kind of thing a maintainer should not
quietly hold the opposite opinion about.

**The unwind, done with `--soft` so nothing was lost**: `e1b1fc3` and `45013b0` were the two the user
named, and two further commits had to go with them — `78416d2` and `9557d18`, both document records, the
second of which claimed U3 as landed **citing `45013b0`**, a commit that would no longer exist. HEAD went
back to `51198aa` and the work was re-committed as `5fb339c`. Only local, unpushed history was rewritten,
and only because the user asked for it.

**One thing went wrong while doing it, and it is worth recording because it was caught by checking**:
`git reset --soft` leaves the *index* as it was, so the first re-commit `git commit` swept in a stale copy
of this document — 35 lines of it — inside what was supposed to be the code commit. Found by reading
`git show --stat` instead of trusting the command's silence, then fixed by reusing the saved message and
committing with an explicit `-- web/src` pathspec. **The size refinement of `R7` therefore lives inside
`5fb339c` rather than in a commit of its own**: splitting one stylesheet across two commits by hunk was
not worth the risk of a botched partial stage, so it travels there and is named in the commit body.

**L31 — Inicio's carousels, and the card's shape (2026-10-14):**

The user's requests, in their own words:

> Procede con U4

> Bien, los cambios se han aplicado correctamente. Hay varias cosas que quisiera corregir: Los mensajes indicando que no hay documentos deberian ser mas cortos. No me gusta que este todo demasiado junto cuando no hay elementos, se debe dejar espacio, una especie de placeholder si no hay elementos en los caruseles... Cuando voy a la pagina dedicada para cada categoria, no hay ningun boton para volver atras, tengo que utilizar el bottom-nav para volver

> Tanto el placeholder como la tarjeta real deben tener las mismas dimensiones. He probado y el aspect-ratio que me gusta es 9/11, aplicalo

> Quedo rota la UI de la pantalla inicio... Las card se ven estiradas, demasiado. Deberian ser un poco mas anchas y menos largas. El texto "Nothing here yet" deberia estar mas pegado al placeholder que simula la portada y no centrado en el contenido de debajo

> Como esta ahora me gusta, por el momento no quiero más iteraciones sobre esto

**What landed** (`b7c5991`): R2 (Inicio keeps only its heading) and R4 = `B1`–`B4` — the three carousels,
their own screens, their definitions on screen, and a card-sized place kept when a category is empty —
backed by the pure `categoryEntries(entries, category, now)` selector, one shared `BookGrid` so the
carousels, the category screens and the library cannot render different cards, routing for
`#/category/<name>`, and a back control that reuses it rather than inventing a second navigation.

**The shape took four attempts, and each one is the reason for the next:**

1. A card-sized placeholder at 18rem, **derived** by arithmetic from the card's CSS. Measured in
   WebKitGTK it was 44 px short of a real card, so it was raised to 21rem.
2. The user found it stretched — a card's silhouette is 136 × 332 — and asked for something less tall
   and a little wider: 11rem by 12.5rem.
3. Then: *the placeholder and the real card must have the same dimensions, at 9/11*. **A whole card at
   9/11 is geometrically impossible**: the text block takes about as much height as the ratio leaves, so
   the cover would be a 30–38 px stripe. Implemented as the **cover's** ratio in both, with a fixed text
   block so a filled and an empty tile matched — which produced a 176 × 391 tube.
4. The user reported the card *estirada, demasiado*, the message floating in the space the reservation
   created, and the empty slot drawn as a frame inside a frame. The fixed block was removed (it was the
   cause of both), the tile widened to 13rem, and the empty slot given the card's own chrome — white,
   solid border, same radius — instead of a dashed translucent frame.

**The honest note**: the residual height is not a bug but arithmetic the user chose. At 9/11 a wider tile
makes a *taller* cover, so "wider and less long" cannot both hold; the options went in front of them with
numbers (a 224 px cover at 9/11 against 184 at 1/1 and 138 at 4/3) and they chose to keep it as it is.

**Two process notes worth keeping.** The command that made the first shape fix **skipped the
dist → binary → process check** and hid `Compiling oct-desktop` behind `tail -1` — the exact trap this
document already records one entry earlier — and it was re-run properly rather than assumed. And when the
fixed text block was removed, a test that pinned it went red and **was updated to pin the new rule rather
than weakened**, while a comment still claiming that the reservation kept every card the same height was
corrected, because a comment that lies is worse than no comment at all.

## L32 — U5: the language is stored, and the tray follows it

> Si, el tray funciona perfectaente, se sincroniza con el idioma seleccionado. Lo unico que hace falta en el tray es la opcion para abrir la pantalla ajustes. Tambien, el primer texto deberia decir "Show OCT" en vez de solo "Show". Tambien, otra cosa que me gustaria es que detecte si esta mostrada o no la app, para que unicamente muestre una de las dos opciones "Show" o "Hidden"

The verdict is the first sentence, and it is the one that mattered: the mechanism nobody could prove
without a click — the tray changing with the language while the application stayed open — **is confirmed
by the user**. The three requests after it are round 3, specified above as `R12`–`R14`.

**What decided the size of the unit.** R1 says the tray's words come from *"the stored language setting —
the same single source the interface reads"*, and there was **no stored language at all**:
`web/src/App.svelte` held `let locale = $state<Locale>("en")`, in memory only. The choice was lost on every
launch, and the desktop process had nothing to read when it built the tray. U5 therefore landed the
source as well as its readers.

**What landed** (`6156f08`):

- `oct-store` schema **V3**: a `settings(key, value)` table with `get_setting`/`set_setting`, written as
  its own recorded migration because V1 and V2 are frozen. The table is generic on purpose — the next
  setting should not need a fifth schema.
- `oct-desktop`: `get_language` and `set_language` as plain functions taking the store, so they are
  testable without a display or a running application. A locale this build does not know is **refused**
rather than stored, and a stored value that is not a language is reported as **no language**, because a
  typo must never reach a caller as one. The tray's words live in one place, `shell::tray_labels`, keyed
  by the stored language, and the comment claiming the menu "is built once" was corrected instead of left
  lying.
- The tray is built from the stored language at launch and re-worded through `tray_by_id` + `set_menu`
  when the language changes. If the database cannot be read at launch, the failure is reported and English
  stands in — **because hiding the window with no tray would leave no way back**.
- `R5`: the window is 430×844 — width, `minWidth` and `maxWidth`. The height and the undecorated fixed
  shape did not move.
- The interface reads the stored language when it starts and writes the chosen one, through
  `web/src/lib/settings.ts` with `invoke` injected so the rules are testable without a Tauri application.
  `isLocale` refuses any value this build has no words for, and `Locale` is now derived from a runtime
  `LOCALES` list so the check cannot drift away from the type.

**Proven at runtime by the parent, not taken from a report.** The tray's own declared menu was read over
the session bus (`com.canonical.dbusmenu.GetLayout` on the item the `StatusNotifierWatcher` announces):
store `es` → Mostrar/Ocultar/Salir, store `en` → Show/Hide/Quit, store `es` again → Spanish. That is the
launch path observable end to end, and it proves the id `oct-tray` names a real object —
`/org/ayatana/NotificationItem/tray_icon_tray_app_oct_tray` — which is exactly what `tray_by_id` +
`set_menu` need. `cargo test` (every suite) and `clippy -D warnings` were re-run by hand, and the diff was
read in full: the V2→V3 fixture is built from the real schema constants so it cannot drift, and the tests
that already existed **gained** assertions instead of losing them.

**One false alarm, recorded so nobody repeats it.** The parent first grepped the bus names for the tray
and concluded it was missing. It was not: app-indicator items register under a unique connection name and
are announced to the watcher, so the only correct question is the watcher's
`RegisteredStatusNotifierItems`. The error was in the check, not in the code.

**The live rebuild was the user's click, and it worked.** The mechanism is source-verified besides: Tauri
documents that on Linux only `None` fails to take effect, and `tray-icon`'s GTK `set_menu` calls the
app-indicator's own on `Some(menu)`.

**Left as debt, on purpose.** `set_language` refuses an unknown locale by returning `OctError::Storage`,
because `oct-core`'s error enum has no variant for invalid input. The worker flagged it instead of hiding
it, and it is its own task rather than an edit inside a unit that was mid-verification: a bad argument
reported as a storage failure is a small lie, and a log that lies sends the next person looking in the
wrong place.

## L33 — the tray menu's entries are divided by lines

> Tambien, debes colocar una linea que divida las tres opciones, algo similar a este menu /home/tomas-cuevas/Pictures/Screenshots/Screenshot from 2026-10-14 12-37-40.png

A fourth item for the same menu, arriving while `U7` was still being built, and recorded as `R15` above.
The screenshot the user attached is another application's tray menu: it shows the application named in
the visibility entry ("Show MechvibesDX", which is the same shape as the "Show OCT" of `R13`) and a
divider line between each group of entries.

The menu therefore reads: the visibility entry, a line, the settings entry, a line, quit. Only the
dividers are new; the words are `R13`'s and `R12`'s.

## L34 — U7: the tray's menu, and three defects that only measurement could find

> Si, funciona todo perfecto en mis pruebas manuales tambien. Podemos avanzar con la siguiente tarea

**What landed** (`f2464eb`): the settings entry (`R12`) — native tray, webview screen, so the desktop
process emits one `open-screen` event carrying the screen name and the interface answers it through the
same hash routing the navbar uses; the application's name in the visibility entry (`R13`); exactly one
visibility entry chosen by the window's real state (`R14`); and a line between each of the three
(`R15`). The menu's shape — entry, line, entry, line, entry — is a pure function `shell::tray_menu_entries`
(language, visible) that a test pins for both languages and both states, so the dividers are a decision
rather than syntax sprinkled on at the builder.

**Three defects, and the reason each one took measurement instead of reasoning:**

1. **The entry sat one click behind** ("tengo que hacer 2 clic sobre el boton para que cambie de
   estado"). The menu *was* rebuilt every time — the item id changed on every action — but the window's
   own visibility answer lags a hide: measured over the session bus, a hide is still read back as
   visible while a show is not. So the outcome of an action that already returned success is passed
   through for that one refresh instead of being read back, and it is never stored. **`R14`'s own
   sentence was corrected**, not the code quietly patched: "the state is read from the window" became
   "and where that read races the platform, the action's outcome is the truth".
2. **The settings entry did nothing** ("no abre la pantalla de ajustes, este o no abierta la
   aplicacion"). Two causes: the project had **no capabilities file at all** — an application's own
   commands need no grant, which is why every command had worked until now, but `listen` is a core API
   that **rejects** without `core:event:allow-listen`, and the promise had no `.catch`, so it failed in
   silence; and the entry navigated without bringing the window up, so with the window hidden it changed
   a hash nobody could see. Both fixed, with `core:event:allow-listen` and `allow-unlisten` as the
   narrowest pair, and the window shown before the request.
3. **The launch read was flaky**: two launches of the same binary worded the tray differently over a
   window that was visible both times. At launch the fact comes from the configuration, because the
   window is created visible by it and the platform's answer at that instant is not yet trustworthy.

**How it was verified without touching the user's session.** The tray's menu is readable and clickable
over the session bus (`com.canonical.dbusmenu.GetLayout` and `Event`), which is exactly what the panel
does when a person clicks. That is how the defects were reproduced, and how the fixes were confirmed:
launch, hide, show, and the settings entry bringing back a hidden window. One false alarm is recorded so
it is not repeated: grepping the bus names for the tray makes it look absent, because app-indicator items
register under a unique name and are announced to the `StatusNotifierWatcher` — the only correct question
is the watcher's `RegisteredStatusNotifierItems`.

**Process notes.** A background worker timed out mid-unit and delivered no report; its diff was read in
full and its RED evidence was recovered by **falsification** (breaking each rule and watching the tests
fail), which is stronger than the report that never arrived — and that is also why the live path had
never been exercised, which is where two of the three defects lived. `review start` rejects a long
`lensesReason` with `lenses-invalid` and creates no lineage: 530 characters failed twice, the 205-character
reason that had already worked succeeded.

**The review, and its debt.** The RDD review on `review-becc52b5cf107481` approved this work in the
revision before these fixes and burned its authority on acknowledgement; two of its six non-blocking
findings were exactly defect 2, which it flagged before the user hit it. Those six are `R3-1`, `R3-2`,
`R3-3` and `R4-tray-hide-no-way-back` besides, and they are their own triage task rather than a reason to
re-review.

**Left honestly open**: the menu is refreshed on the application's own paths, so a window restored or
hidden by something outside the application can leave it stale until the next in-app action. The window
has no `skipTaskbar` setting, so that case is reachable.

## L35 — the tracker never tracked: reading produced no record

> No, he hecho los 4 pasos que me haz dicho varias veces con diferentes PDFs y no paso nada, sigue tanto el porcentaje en 0% como asi no aparece nada en la pantalla inicio

This is the gravest defect the project has had, and the first one that is not a mistake in a part but the
absence of the connection between them: the user reported it instead of a hundred other things, which is
what makes it worth stating plainly.

**What was measured before any theory was formed.** The real store: **42 documents**, one watched folder,
and **zero** rows in `progress` and **zero** in `open_events`. So the documents arrive and their cards
render at 0% — which is the recorded rule "no reading record -> 0%" working correctly over a record that
never came. Meanwhile `~/.local/share/gvfs-metadata/home` was present and had been modified the same
afternoon, while the user was reading: the source the provider reads is real, live and being written.
`record_progress` appears twice in non-test code and both calls are inside `mod tests`; `crates/oct-desktop/src/main.rs`
starts no detector, no watcher and no refresh; and the interface's calls are only the library, the
providers, the watched folders, the scan, the folder action, the window controls and the language pair.
The adapter was never connected to anything.

**Why no test caught it.** Every test in this project verifies a part: the adapter reads the metadata
correctly, the detector lists what is open correctly, the store writes and reads correctly, and the
interface renders correctly. A loop that does not exist cannot fail a test, and the omission lived
exactly in the gap between four green suites.

**The unit this became** is `T17` above, and the user's four steps are its acceptance test.

## L36 — T17: the tracker tracks

> Si, con estos cambios ahora funciona todo de maravilla, es mas, ya me sirve ya que pude ver todos los PDFs que habia leido previamente.

**The second sentence is the one that matters**: for the first time the user can see every PDF they had
already read. That is the product's promise arriving, and it arrived because the loop was the only thing
missing — the adapters had been correct all along.

**What landed** (`151ef99`): `crates/oct-desktop/src/track.rs` holds the decision as plain functions that
take the detector, the provider registry and the store as arguments, and `main.rs` runs one catch-up sweep
at launch and then a three-second tick off the main thread. The interval's reason is in the constant, and
so is the reason an unchanged position is never rewritten (the table is an append-only log), why `Ok(None)`
writes nothing ("no record" and "page zero" are different facts), why a document the detector sees but the
store does not know is ignored rather than added (the watched folder stays the source of truth), and why a
missing capability and a failure are logged apart.

**Verified by the parent before the user saw it, on the user's own library**: `progress` went from 0 to
**28 rows** in 14 seconds, all `[evince-gvfs/exact]` — Cosmos at 72/575, Historia del Pensamiento II at
524/823, La Rebelión de Atlas at 18/1058 — and the document the user happened to have **open** at that
moment was recorded at **page 34 of 314**, which is the same page the provider had reported independently
minutes earlier. Twenty-five seconds of further ticks over that open document added **no row**, which is
`S2` holding under the live path rather than only in a test, and stderr stayed empty.

**Why no test had caught the omission**: every suite in this project verifies a part, and a loop that does
not exist cannot fail a test. The gap lived between four green suites, and it was found by a person
following the application's own recipe and reporting that nothing happened.

## L37 — the carousels are too long, and they scroll sideways *and* vertically

> Si, con estos cambios ahora funciona todo de maravilla, es mas, ya me sirve ya que pude ver todos los PDFs que habia leido previamente. Es más, esto me hizo ver que hay que limitar el numero de elementos que puede aparecer en el carrusel, ya que a mi, me aparecieron demasiados, tantos que molestaba tener un carrusel tan largo. Otra cosa que note, es que dentro del contenedor de un carrusel puede haber scroll vertical y eso no me gusto nada

Both items are consequences of the same success: once `T17` filled the carousels, the strip held far more
than the handful of documents it had been designed against, and assumptions that had never been exercised
showed themselves.

**A third defect the user did not report**, found by the maintainer in the attached screenshot: the card
prints the same page count **twice**, above the title and again below it. It is recorded as `R18` rather
than quietly fixed, for the same reason this log exists — nobody reported it, and it had been on screen
since the card was last rebuilt.

## L38 — the recalculation, finally committed

> Respondiendo a la pregunta sobre #45, dale nomas

The fix for a defect recorded earlier in this log: a screen that stays open could lie about its
categories, because `categoryEntries` received the clock once, when the screen mounted. It landed as a
shared helper in `library.ts`, with its interval as one named constant and one cleanup, used by Inicio
and by a category's own screen. Committed as `f0a9a8e`.

**Why it waited so long, and what that says.** The user could not approve it by eye because there was
nothing to see: the carousels were empty for exactly the reason `L35` records — nothing was ever
ingested — so the fix sat uncommitted through three exchanges while the tracker itself was repaired. It
was finally accepted on the strength of its four tests rather than of a look. Its rule is "a screen that
stays open must not lie", and the half it does not cover is still its own task: a document that finishes,
or that gains its first record, changes the listing rather than the clock.

## L39 — U8: the carousel's shape, and the test that was missing

> Los carruseles siguen teniendo más de 10 items, por lo menos en la seccion "Documentos en curso" que puedo verificar

Three criteria (`R16`–`R18`) and one defect of the maintainer's own. **The defect**: the cap was computed
and never used — `shown` sat in `BookGrid`'s script while the markup still walked `entries`, so the limit
was calculated and thrown away. **Why every check missed it**: the build was green, 77 tests were green
and clippy was clean, and the suite stayed green because **no test covered the rule**. A cap with no test
cannot fail. It is the second time in this work that the user's eye caught what a green suite could not,
and both times what was missing was a test of a rule rather than of a part.

**What was done about it**: `BookGrid.test.ts` renders the component through `svelte/server` — the
technique `BookCard.test.ts` already used — and counts the cards actually drawn: a strip gives ten however
many the category holds, and a grid gives every entry, so the cap cannot leak into a full list. The RED
was observed before the fix (`expected 25 to be 10`, with the grid case passing), and the one word that
fixed it was `{#each shown}`. The bundle's content hash changed with the rebuild, which is the proof that
the change really reached the binary — the thing that had been in doubt.

**A predicted defect that turned out not to exist.** While fixing the duplicate page line, the maintainer
argued from the same arithmetic that the fixed height does not count the optional author line, and opened
a task for it. The user checked: authors **do** show and the cards look clear. So it was dropped rather
than fixed, and the reason is the same one this log keeps repeating — reasoning from a formula lost to
looking at the screen.

## L40 — U9: the page count moves onto the cover

> U9 esta bien hasta donde pude ver. Comitea y prosigue con la siguiente tarea

**What landed** (`667bec5`): the page count left the card's text block and became a badge on the cover's
bottom-right, mirroring the folder action already at its top-right — the same inset, the same border
radius, the same white pill, so the two corners read as one design instead of two unrelated marks. It
renders only when the library knows the count.

**Two consequences, both stated to the user before they looked.** An unknown count now shows no badge at
all, because a corner pill cannot hold "Número de páginas desconocido" and a card that cannot state a
number must not invent one; the two catalogue entries that sentence used went with it. And the text block
keeps its 11rem height on purpose: shrinking it would make the empty placeholder's identical declaration
wrong, and the slack that leaves is the headroom the optional author line already needed.

**A correction the maintainer owed, and paid.** Reading the height comment in full for the first time
while doing this, he found that it counts an author line — "two title lines (2.94rem), an author line
(1.23rem), the page line (1.23rem)". His earlier claim that the fixed height ignored the author line came
from quoting that comment only in part, and the task he opened for it (`#53`) was dropped rather than
fixed. The code was right and the reading was not. That makes three times in this work that looking beat
reasoning, and the first where the wrong side was the maintainer's own reading of his own comment.

**Debt avoided**: the `.pages` rule went with the markup that used it, so nothing was left dead behind
this change — the same discipline that produced its own task for the dead `status` chain instead of a
hurried removal.

## L41 — five things about watched folders

> Hay algunas cositas de UI y de funcionalidad que deberiamos pulir, ya que me han molestado un poco:
>
> - Se tendria que poder eliminar una carpeta vigilida.
> - El texto que indica la URL de las carpetas vigiladas deberia ser un poco más pequeño.
> - No se tendria que dejar añadir una subcarpeta de una carpeta que ya esta vigilida. Cuando el usuario quiera hacer eso, se le tendria que dar una alerta explicando por que se bloqueo y no se agrego la nueva carpeta que selecciono.
> - La UI de la seccion para seleccionar carpetas es HORRIBLE (dos capturas de pantalla)
> - En la seccion de carpetas vigiladas, se tiene que permitir hacer clic sobre la URL y que se abra la carpeta en el explorador

The list arrived within an hour of the chooser landing, and it reads the way the most useful feedback in
this log reads: three items about behaviour that was missing and two about what the screen looks like. One
of them — refusing a subfolder of a watched folder — is a rule the application should have been able to
enforce on its own from the beginning. All five are specified as `R20`–`R24` above.

**The layout item has a cause rather than a taste**, and the user's screenshots are what makes it fixable
without guessing: the section's form is a flex row, so "Elegir carpeta", the manual-entry disclosure and
"Escanear" landed on one line as if they were all the same kind of thing, and the scan result broke a path
across lines at the same weight as everything else around it.

## L42 — the section, the alert, and what removing a watch means

> Hay cosas para corregir aun:
>
> - La estructura/composicion de la seccion "Escanear una carpeta" no me gusta, es fea esa seccion.
> - La alerta [captura] no debe aparecer debajo, tiene que aparecer en un modal/dialog.
> - Elimine la carpeta de "Borges" pero el PDF que allí estaba, sigue apareciendo en mi biblioteca.

The first two are the same section seen twice, and the third is a rule this log had already recorded the
wrong way round.

**The composition, read from their screenshot.** After `R23` the section stopped being a flex row, but it
became a column of six blocks of equal weight — two identical dark buttons, the path, the reread line, the
browser's own disclosure triangle, and the alert at the bottom. Making a list vertical is not the same as
composing it. `R25` is therefore about grouping: the two ACTIONS belong in one row, because choosing a
folder and scanning it are the two things a person can do here; the chosen path belongs under them as a
field-like readout rather than as prose; and the manual entry is an alternative route to the same field, so
it reads as a link and not as a box.

**The modal is also the honest place for a destructive decision.** The user asked for the alert to be a
dialog; the removal in `R27` needs exactly the same thing, so one modal serves both instead of two shapes
doing one job. That is why the interface gets its own modal rather than the native message box the dialog
plugin could open: the words stay in the catalogue with every other word of the interface, and the modal
can state a consequence with a count in it.

**`R27` reverses a decision, and the reversal is the user's to make.** `R20` said removing a watch leaves
the documents alone; the user has now seen what that means in practice and said what they meant. The
dangerous half is worth naming plainly: this deletes rows from `documents` and their positions from
`progress`. Two guards make it safe enough to offer — a document still covered by another remaining watch
is kept, and the positions are recoverable because the viewer's own metadata still holds them, so re-watching
the folder brings them back through the launch sweep. The confirmation states the count and that sentence
before it does anything, and a destructive operation needs an independent check afterwards, not only the
author's own tests.

**The verification found two things worth writing down.** The first is a correction to my own words in
`R27`, and the correction is the honest half: the sentence claimed the number the modal states and the
number of rows that actually leave *cannot disagree*, because both come from the same computation. They come
from the same computation, yes — but they are two calls, and the second one re-reads the store. The
verifier demonstrated it: it read an empty plan, inserted a document, and watched the confirmation's own
number become stale. The sentence now says what is true — same question, same code — and names the window
as the residual. This is the fourth time in this document that the measurement corrected a sentence rather
than the code (`A2`, `R14`, `#53`), and the pattern is worth keeping in view: a spec sentence is a claim
like any other, and a claim nobody can falsify is the one that rots.

The second is a test that was missing for a claim the code already made. `forget_folder` deletes the
documents and the watch row in one transaction, and *nothing in the suite could tell that from two
statements in a row*, because every test exercised a removal that succeeded. The test added now injects a
failure with a SQLite trigger and asserts the first delete came back — and it was falsified by replacing
the transaction with two plain statements, which left the document deleted and the test red
(`left: 0, right: 1`). The cascade onto `open_events` got a test in the same pass, for the same reason: it
was true, and nothing would have noticed it stopping being true.

**Residuals this unit leaves, named rather than hidden.** (1) *The confirmation is not a reserved
snapshot* — the plan and the removal are two calls, described above. Single-process today, so nothing can
change in that window; the window is real all the same. (2) *`suggestions` rows are not touched*: they
carry a path and no document foreign key, so a suggestion pointing inside a removed folder stays where it
is. The criterion is about library documents and this is outside it, but no policy is stated for a
suggestion whose folder is gone, and this is where that is recorded. (3) *Documents are identified from the
snapshot the plan read*, and the store keeps its single-writer assumption, unchanged by this unit. (4) *The
tracking loop cannot resurrect a removed document*, and that is verified rather than assumed:
`track_open_documents` and `sweep_library` iterate `store.library()` and insert nothing, and the desktop
process starts no folder watcher. When the live watcher is wired into it (a later task), it must not re-add
what a removal took out. (5) *The recoverability sentence assumes the viewer is what kept the
position.* It says the position is kept by the viewer and comes back, which is true for the positions this
build records — every stored row today is `evince-gvfs/exact`, read from the viewer's own metadata. A
position recorded by hand would not come back that way, and `ProgressSource::Manual` exists in the store
with no writer yet, so the sentence is honest about the source it describes and would need to change if a
manual source ever lands.

## L43 — U10 and U11 land together, and the number the modal promises is the number it deletes

Two units, one commit, and the reason is a fact rather than a shortcut: `U6`, `U10` and `U11` meet in
`crates/oct-desktop/src/main.rs` and in one screen, and `U11` changes a signature `U10` had just written —
`remove_folder` answered `bool` and answers the plan it carried out now. Splitting them would mean
committing a revision that never ran; the revision that did run is the one this log already described at
11:42. The user reviewed the three in one round, approved the result in their own words, and that is what
lands.

What the round cost and what it bought: **one defect of my own design**, found by the worker's flag rather
than by a test — the confirmation said a folder had nothing in the library when in fact another watch
covered everything in it, which is one sentence for two different facts and is now two sentences. **A spec
sentence that claimed more than the code does**: `R27` promised the count shown and the count removed
*cannot disagree*, and they are two calls against a store that is re-read, so the sentence now names that
window as the residual it is. And **two claims that were true with no test able to notice them stopping
being true**: the removal's transaction, and the cascade onto `open_events`. Six falsifications in total,
each restored byte-identical, each reported with the test that went red and its message.

The honest count as it stands: **182 Rust tests** (was 173), **104 tests in the interface** (was 85), clippy
clean, `tsc` clean. And the chain that proves the running application is this code rather than last hour's:
dist 12:17:48 < binary 12:22:39 < process 12:23:10, pid 130761, stderr empty, tray registered.

One decision from the worker's handover deserves to be here rather than only in the task list, because it is
the rule a future reader is most likely to "fix": removing a **nested inner** folder keeps its documents,
because the outer watch still finds them and deleting them would be undone by the next scan of that folder.
The guard asks "does any *remaining* watch cover this document", and an ancestor watch is a remaining watch.

The test the user ran is worth recording because **their store is what shaped it**: `Borges` and
`Programación` had already been removed with the OLD rule, so neither was in the watched list and there was
no "Quitar" to press — the orphaned PDF sitting in their library was the proof of the old behaviour. The
round trip therefore became: add Borges back, remove it, and watch the modal say one document and the PDF
leave Biblioteca.

## L44 — #46: the listing is re-read while the application runs

> Si, funciona perfectamente

A defect that was invisible, which is the worst kind and the reason it survived four rounds: `App` read the
library at mount and after a scan, while the tracker writes a reading position every few seconds from the
desktop process. So the one thing this application exists to show — a document moving as you read it — only
appeared after a relaunch. It looked dead for exactly as long as it was working.

**The load-bearing decision is not the clock but the quietness.** `LIBRARY_REFRESH_MS = 15_000` and
`startLibraryRefresh` sit beside the category clock, and 15 s is argued in the code: the tracker's own tick
is three seconds and it writes only when the position CHANGES, so four small reads a minute buy all the
freshness a carousel can show, while matching the tick would read for facts that cannot have changed and a
whole minute would leave a finished document sitting in "in progress" where a reader can see it. What
matters more is that the re-read is a separate function from `loadLibrary` and not a flag on it: that one
raises the loading state, and raising it every interval would flash an empty grid over a listing that is
already on screen — a fix that would have looked worse than the defect.

A failed re-read is reported where the first load's failure is reported and clears on the next success, so a
transient failure heals instead of leaving a listing that looks current and is not. Three tests pin the tick,
its cleanup and its bound, and falsifying the cleanup — a timer that outlives the application — turned the
tick test red with `expected 5 to be 2`, which is a leaked interval caught rather than described.

The verification of the rebuild also got sharper in this round, and that is worth keeping: Tauri embeds
`web/dist` compressed, so grepping the binary for frontend content finds nothing — but the asset manifest
keeps each file NAME uncompressed, so the new bundle's name is present inside the binary and the previous
one is absent, which is direct evidence instead of an inference from timestamps.


## Specs — round 9: the rest of the list (L47)

The user's instruction for this round, verbatim: *"Procede con todas esas tareas"*, given after the six
candidates were listed to them. Six units, ordered so the cheap and the decision-free go first and the two
that need a measurement or a platform answer wait for it. The migration numbers moved: `U14` took **V4**, so
the two units that need a schema change are **V5** (titles) and **V6** (covers) — an earlier note in this
document said V3 for titles, written before the reading time existed.

- **R31 — the reader can set a title, and the application says where each title came from.** From L18: *"a
  title extracted from the PDF and a title a person typed are different facts, exactly like progress from
  Evince versus progress typed by hand"*. So it is not a text field bolted onto a card: a typed title is a
  fact with a source, stored as one, so the interface can say whether it is showing the document's own title
  or the reader's — and the first time it guesses wrong, the reader stops trusting every other field. Schema **V5**
  adds **`documents.user_title`**, nullable, and NOT a `title_source` enum: this sentence first demanded an
  enum beside the title, and the implementation — which the worker built against the corrected contract and
  flagged rather than hid — made the better choice. The reader's title is a **separate fact in its own
  column**, so the document's own title is never destroyed, which is the only reason "restore the document's
  own title" can exist at all; overwriting it would have made that action impossible. Provenance is then
  DERIVED rather than stored, from two columns that cannot disagree: `user_title` set means the reader's,
  otherwise the document's own, otherwise the file's own name — and that precedence lives in ONE place,
  `Document::display_title`, never in a screen. The migration must leave every existing row readable, every
  title it already had intact, and every existing document with no reader's title (NULL = "not set", not
  "set to nothing").
- **R32 — covers.** Default the first page; the reader can choose another page or supply an image of their
  own. Storage was decided in L20: `~/.local/share/oct/covers/`, because an uploaded image is user data that
  must survive a rescan. Three things, in this order: the **rendering measurement first** (pdfium's render
  path has never been used here — R12 measured pdf.js in a browser, not this), then the default page, then
  the reader's choice with schema **V6**. If the measurement says a whole library cannot be rendered at
  launch, the covers are produced lazily and cached, and that becomes the design rather than a compromise.
  **What is already known about the API, so the measurement's own failures are not repeated** (all of it read
  from `pdfium-render` 0.8.37 and confirmed by getting the probe to compile): rendering needs **no new
  binding, no second crate and no raw FFI** — `PdfPage::render_with_config` → `PdfBitmap::as_raw_bytes` is
  the whole path, and the deliberately-disabled `image` feature only gates the PNG/JPEG conveniences, so
  writing a cover to disk needs a small encoder (`png`) instead of re-enabling the stack the workspace kept
  out. Two borrow traps cost three failed builds and are worth writing down: `PdfRenderConfig`'s setters
  **consume** the config and return it (`let config = PdfRenderConfig::new().set_target_width(400);`), and
  `pages.get(i)` returns a page that borrows from a temporary of its own, so the **page** must be bound to a
  name before it is rendered. pdfium is also single-threaded behind this binding — `thread_safe` serialises
  the calls — so generating covers in parallel will not scale across cores, and ~875 KiB per 400×560 BGRA
  bitmap means the design must render one, encode it and drop it rather than hold a library of bitmaps.
  **THE MEASUREMENT IS DONE, and it changes the design from "lazy, as a compromise" to "at scan time, because
  it is cheap".** Measured with a probe built against this project's own pinned binding and its own pdfium
  library, three runs per page, on a real 2,2 MB typeset paper (arXiv 1706.03762, figures included) and on the
  repository's synthetic fixtures as a floor:
  loading pdfium 0,6 ms and loading the document 1,1 ms; rendering page 1 into a 400-px-wide bitmap **7,3 ms
  cold and 1,8–2,3 ms warm**; page 2 **7,6 ms cold and 1,9–2,5 ms warm**; **828.800 bytes** per bitmap; peak
  resident memory **~10 MB**. The fixtures, which have almost nothing to rasterise, come in at 0,0–0,5 ms —
  which is why they could never have answered this question and why a real document was needed.
  The fact that decides the design is the third number: **page 2 costs what page 1 costs**, so the cost is
  per page and not per book, and a 300-page book's cover costs exactly what a 15-page paper's cover costs. The
  whole library of 42 documents is therefore **0,1–0,3 s**. And the scanner ALREADY opens every document with
  pdfium to count its pages, so rendering the cover there is ~2 ms of marginal work per document in a pass
  that already happens — no second pass, no queue, no placeholder state to design. The honest residual, not
  measured: a SCANNED book, whose page is one large image, may cost far more than this paper's pages, and
  nothing here bounds that case; the design must still not block the UI thread while it renders.
- **R33 — launching the application must not steal focus.** Verbatim, the report that arrived with the
  withdrawn self-exit defect: *"me molestaba que saltara la aplicacion sobre lo que estaba haciendo"*. A
  tray-resident companion must be available without interrupting. **The platform answer comes before the
  code**: an earlier spec in this document had to be corrected because tao's `set_maximizable` is an empty
  function on Linux, and this IS the second such limitation, now recorded with its measurement. **DECIDED AND
  IMPLEMENTED** after the user's answer — *"Si, la app jamas debe arrancar visible"*: the window is created
  with `"visible": false`, so it exists, loads and works, and is never mapped; an unmapped surface cannot
  take focus on X11 or on Wayland, and the tray's Show entry already calls show + focus, which is
  user-initiated and therefore correct. What is NOT deliverable, and is written down so nobody promises it
  or "fixes" the config back: "visible but never focused" is unreachable with this stack as pinned —
  `focus: false` only sets the GTK accept-focus hint, tao re-enables `accept_focus` after the first draw,
  `gtk_window_set_focus_on_map(false)` is not exposed by tao or tauri, and on Wayland the compositor decides
  with no `_NET_WM_USER_TIME` and no activation tokens available to the application. The line is pinned by a
  test, because the build only validates the config's shape and a window that started visible again would be
  reported by nothing except a person being interrupted.
- **R34 — the first run explains itself.** Found in the user's own words while closing R29: *"No se realmente
  si se recalculan los carruseles, ya que no forma de agregar items al carrusel como tal"*. Nothing was
  broken — a document with no reading record sits in no category, and `oct` never opens a document itself, it
  observes reading done elsewhere — but a person who installs this sees three empty carousels with no
  explanation of why they are empty and no route to filling them. Inicio says what the carousels wait for and
  what the person can do about it, in both languages, without inventing data and without turning the screen
  into a manual.
- **R35 — an invalid argument is reported as an invalid argument.** `commands::set_language` refuses an
  unknown locale with `OctError::Storage`, because `oct-core`'s error enum has no variant for invalid input:
  a bad argument is reported as a storage failure, and anyone reading a log later looks in the wrong place.
  It is not reachable from the interface today (the selector offers en/es), which is why it waited for its own
  unit instead of riding along inside one that was mid-verification. `oct-core` stays dependency-free; this
  is a variant, not a dependency.
- **R36 — the four advisory findings get a disposition.** `R3-1` (tauri.ts:34-35), `R3-2` (main.rs:403-406),
  `R3-3` (HomeScreen.svelte:47) and `R4-tray-hide-no-way-back` (main.rs:124) were left by the approved review
  of an earlier revision, whose own words are that they are **not** blockers and not a reason to re-run it.
  Each is judged against the code as it stands today — real, already handled, or no longer applicable — and
  the answer is written down, because a finding nobody dispositions becomes permanent noise.

### Tasks — round 9

- **U15** R35 — one error variant for an invalid argument, `set_language` using it, and the tests that pin
  `Storage` updated.
- **U16** R34 — what Inicio says when it has nothing to show, in both languages, with the catalogue keys
  pinned by a test.
- **U17** R36 — the triage itself: a verdict per finding written into this document, and a bounded fix for
  whichever turns out to be real.
- **U18** R33 — the platform answer on focus, then either the change or a recorded limitation.
- **U19** R31 — schema V5, the provenance, and the card action that sets a title.
- **U20** R32 — the render measurement, then the default cover, then the reader's choice with schema V6.


### Dispositions — the four advisory findings (U17/R36)

Three of the four were already handled, and the fourth's named path was too; what remains is one residual the
log had already parked. Verified against the code as it stands, NOT against the review's locations, which have
moved since (R3-3's line went from 47 to 56).

- **`R3-1` (the event wrapper) — ALREADY HANDLED.** The wrapper returns its promise and the ONE call site
  handles rejection: `App.svelte` catches it and reports it ("the tray's screen requests will not be
  received"), and the capability whose absence the silence originally hid now exists. The finding is the same
  observation as the defect fixed in L34, seen one function earlier.
- **`R3-2` (the tray's emit) — ALREADY HANDLED.** The window is SHOWN before the request (so navigation
  cannot happen behind a hidden window), the webview can listen (`core:event:allow-listen`), and an emit
  failure is logged rather than discarded.
- **`R3-3` (the category tick) — ALREADY HANDLED.** `onMount`'s cleanup IS `startCategoryRefresh`'s
  `clearInterval`, and a test pins that a stopped clock stops ticking. One real but tiny gap is noted for
  later: while the window is hidden, the tick can leave "a long time" up to a minute out of date.
- **`R4-tray-hide-no-way-back` — ALREADY HANDLED for the named path.** The action's own outcome is passed
  into the tray rebuild on every hide and minimize path, which is what the defect was. Its residual — a
  visibility change made from OUTSIDE the application (the window has no `skipTaskbar`) is not observed, so
  the menu can be stale until the next in-app action — stays as the log records it. In practice that is a
  wasted click and not a trap: the second click re-words the menu to the other state.

**What the four have in common is worth more than the four verdicts**: three of them are one missing habit at
three points of the same native↔webview boundary — *every action that crosses a boundary must name its failure
path and its teardown, and the side that must react has to re-read or be told the real state instead of
assuming the action took effect*. The two defects the user actually hit were the same question asked of the
same boundary, so it is recorded as a review-time question rather than as four patches.

**One hardening is real and is added**: nothing in the suite would notice the capability file losing the grant
that makes the tray's settings entry work at all — the original defect was exactly "there was no capabilities
file, and nothing anywhere said so", and `cargo build` validates the file's SHAPE rather than its contents. A
test now reads it and asserts both grants are present.

**L45 — the carousels' order: the most recently read document first (U13/R29).** The user asked that
*"el ultimo elemento que se haya leido siempre aparezca primero … de mas reciente a más antiguo"*. The order
lives in the same pure selector that decides which documents a category holds, so the carousel, a category's
own screen and the ten-tile cap read ONE order and cannot disagree about it. The tiebreak is the displayed
name lowercased and then the row id — never `localeCompare`, because a tiebreak must not depend on the
machine's locale, and raw code units put `Zebra` before `apple`. Two falsifications: removing the sort failed
all four ordering tests, and reducing the tiebreak to the row id failed the stability one. It never stood
alone, on purpose: the time it sorted by was the distorted one L46 fixed, so fixing the source without the
order would have left half the defect standing — and the user approved both in one verdict. The unit's own
spec sentence named `progress.updated_at` as what "read" means; that is false by design now and was corrected
above, because leaving it would send the next reader to the wrong field.

**L46 — the app now knows when you read: the viewer's history, a separate time, and a high-water mark
(U14/R30).** The user reported an empty "hace mucho" carousel: *"no hay ningun elemento en el carrusel que
representa los PDFs que no se abren hace tiempo, y no es porque no haya documentos… la gran mayoria no los
abro hace bastante"*. The diagnosis found TWO symptoms of ONE defect. `record_progress` stamps every row with
`unix_now()`, so a record's time was always *when oct looked*; the launch sweep writes the whole library
inside a second, and the oldest row in their store was 1,05 days old — with a 30-day threshold, nothing
*can be* old. The same distorted time fed R29's order, which is why their tiles came out looking
alphabetical. The fix is a second, SEPARATE fact: `progress.read_at` (migration V4, nullable), where NULL
means "no source said" — a different fact from "read long ago", and the one thing no amount of looking by us
can establish. `updated_at` keeps its old meaning because it was honest; it was simply never the answer to
this question.

TWO SOURCES FILL IT. The desktop's GTK history (`~/.local/share/recently-used.xbel`) is read once per sweep
by `crates/oct-platform/src/recent.rs`: a tolerant reader — attributes read by name, percent and entity
decoding, ISO-8601 with microseconds, `file://` only, a missing file meaning an empty history and an
unreadable one an honest error — whose date arithmetic was derived independently (`date -u` and Python's
`calendar.timegm`, including a leap day). And the live tick: a document the tracker watches being read is
stamped with the moment it saw it. The stored value is a HIGH-WATER MARK, and it had to be: the history
records the moment of OPENING, seconds before the tick sees the page turn, so a later sweep finding the
earlier time would have walked the fact backwards and made a document read today look older than the store
already knew it was.

WHAT WAS CONSIDERED AND REJECTED, WITH THE MEASUREMENT. The user offered the file's atime as a fallback. All
42 documents show an atime of ~1,1 days because **oct itself reads the files** — pdfium counts pages, the
sweep opens them — so a fact written by the application's own scanning is not evidence about the reader.
Recorded so that nobody derives it again.

THE HANDOVER SAID THE UNCOMFORTABLE PART. The history knows 15 of their 42 documents; 36 rows say honestly
that no source said when they were read, and 26 documents gain nothing from this round until the person
opens them or the viewer registers them. The category's on-screen definition was rewritten to say what it
measures and that a document no source ever recorded is not claimed there, because an empty carousel with no
explanation is exactly what cost the user time. On their real database the migration ran in place: 42
documents and 51 rows immediately after (55 measured later — the difference written by real reading), 15 rows
gaining a time, and "hace mucho" going from 0 documents to 5 (64,9 · 64,2 · 63,0 · 49,2 · 37,8 days) with 5
more crossing the threshold in the days that follow. The user then closed the loop without meaning to: they
opened *El extranjero*, which the history dated 63 days back, the tick stamped it with that moment, and the
document left the category on its own.

THE INDEPENDENT VERIFICATION EARNED ITS KEEP. Five claims went in; it refused to call three of them
satisfied, and one of those was a real defect: the sweep accepted a reading time from the FUTURE and stored
it, and because `read_at` is a high-water mark it would have been kept forever, leaving a document nobody had
touched in a year permanently fresh for the stale rule. The fix is one predicate (`credible_reading_time`)
used by BOTH paths, and it judges the value the store ALREADY knew as well — which is what heals a row a
previous build could have left in the future, instead of waiting for somebody to open that document. It also
found that my ordering sent "no source said" to `0`, which is 1970, not the end of the scale, so a time
before the epoch sorted BEHIND a document nobody knows anything about (it is negative infinity now); that the
high-water sentence claimed more than the code did — a stored time in the future is replaced, because in the
tick we are watching the document open — so the SPEC was corrected rather than the code; and that no test
proved a FAILING migration leaves the database alone, which is now pinned: a database whose `progress` table
already has the column makes the V4 step fail, and the version stays 3 with every row intact. It also refused
to confirm what it could not test: there is no pre-migration snapshot of that database, so preservation rests
on the counts and the integrity check rather than on a comparison, and that is written down as a residual.

ONE MORE DEFECT CAME OUT OF THE FIX ROUND, in the test harness itself. The fixture that writes a history file
named it after the PROCESS, and every test runs in the same process, so two tests overwrote and deleted each
other's file. It failed a DIFFERENT test on each run and passed on the third — the worst kind of failure,
because it had nothing to do with the code under test. The name is unique per call now, and the evidence that
it was the cause is that three-run sequence itself: fail, fail, pass. Five falsifications in this round, all
real RED and all restored byte-identical with their hashes: the sweep writing `unix_now()` into `read_at`,
`with_read_at` ignoring its argument, the stale rule reading the row's write time (5 tests red), the order
reading it (3 red), the null key back at `0`, and the sweep no longer judging the value it already held.

THE HEURISTIC THAT WAS KEPT, WITH ITS RISK WRITTEN DOWN. The history's `modified` field was suspected of
meaning "the file was edited" rather than "the document was opened", which would let a change to a file claim
a reading. Measured on the user's machine: `modified` equals the file's mtime in **0 of 15** cases (those
files' mtimes are 315, 455 and 788 days old while `modified` ranges 0–119), and of the 4 documents opened
that day `modified` moved in ALL of them while `visited` moved in only 2 — so with `visited` alone, two recent
readings would have been lost. The maximum of the two is therefore the last time a GTK application registered
the document, which is the open. The residual stays written down: on a machine where `modified` means a
content edit, the rule could claim a reading that did not happen.

**The user's verdict on the running build: *"Si, todo funciona correctamente como deberia"*** — with the five
stale documents, their real day counts and the corrected definition sentence on screen.

**L57 — la cola de sugerencias, mitad de adelante: la tarjeta, y el contador que el usuario no pidió.**
The queue is shown as cards in Configuración, beside the watched folders. Each card carries the file's own
name as its heading — the app never read the file, so it never claims a title it does not know — the path
under it, and the two decisions as icons inside the cover's space: a check and an X, with accessible names,
because a decision offered as a bare glyph is a decision a screen reader cannot take. Nothing enters the
library on its own; accepting is the reader's action and the library is made current through the mechanism
that already exists.

**L58 — the user's two verdicts on the card, both taken as corrections rather than as taste.**
First: "Sigue sin tener la portada bien colocada" — the only honest reading of "not placed right" was
"not the same as the others", so the cover now uses the library card's own CSS: the same dashed border, the
same corner radius, the same striped stand-in. Second: "Los botones deben estar en la parte inferior del
medio de la portada" — they moved from the middle to the bottom, centred. The path became a small control
that opens the document's folder, because a path a person cannot use is a dead line on the card. The
screenshot was read twice and cost more context than it returned; the user's words were the specification.

**L59 — the observation count removed, at the user's word.**
"Se abrió 239 veces" was this project's addition, never a request, and the user does not want it: the
sentence, the two i18n keys and the helper that chose singular from plural are gone. The test count says so
without being asked — 185 became 183 — and a new assertion pins the absence rather than leaving it to
memory.

**L60 — the cover: the document's FIRST page, which the interface could not produce.**
The user reminded me that the default cover is the first page, and they were right that explaining the limit
was not the same as fixing it. The interface cannot render a PDF, so the piece belongs in Rust:
commands::suggestion_cover takes a PATH, not a row id, because a suggestion is not in the library yet.

The design decision worth writing down is that it does NOT cache. document_cover names its cache file with a
row id and a page, and a suggestion has neither. A name invented from the path would be either a hash — which
can one day serve one document's page for another — or a sanitised path, which has no useful length bound. So
the preview is rendered to a temporary file, read back and discarded: nothing stale, nothing misattributed,
one render per card, and a card asks once on mount. The cleanup is best effort and is not tested; the two
user-visible rules are.

THE TEST FOUND A REAL DEFECT IN MY OWN FIRST VERSION, in one line: it deleted the temporary file BEFORE
reading the page it had just rendered — "could not be read back: No such file or directory". The order is
read first, clean up last, and the comment says so where the mistake would be repeated.

Three falsifications, all real RED and all restored byte-identical with their hashes (commands.rs
84444723…, suggestions.ts 5867733d…): the render returning None (red on the first page), the unreadable
branch returning an error instead of no cover (red on the placeholder rule), and the frontend reading a
failed request as an empty image (three reds: no cover, failed call, empty string).

Verification: the workspace suite green with oct-desktop at 83 tests (81 plus these two), clippy clean over
all targets, the interface at 187 tests, and the component-aware type check at 0 errors and 0 warnings. The
package was rebuilt before the binary, because the binary embeds it, and the application was relaunched.

RESIDUAL, stated rather than hidden: that the first page appears ON SCREEN is the user's to confirm. The
evidence is a test that renders the repository's four-page fixture through pdfium and comes back with a data
URL, plus one live command registered next to document_cover; a card whose file vanished keeps its
placeholder and writes the reason to stderr.

**L61 — el hueco que yo mismo dejé marcado: la cola no podía distinguir "no hay nada" de "no sé nada".**
El productor de sugerencias ya propagaba NotSupported y su propio comentario exigía que quien llama pudiera decir
que la capacidad falta; el registro de proveedores, en cambio, sólo listaba los de progreso, y
observe_open_documents no estaba expuesto. En una máquina sin detector la cola se veía vacía, que es
exactamente la buena noticia que el proyecto no se permite inventar.

suggestion_detector PREGUNTA en lugar de leer una declaración, y eso es una decisión, no un atajo: el trait
OpenDocumentDetector sólo ofrece un id, una plataforma y los documentos que ve, así que no puede declararse. La
pregunta además responde lo único que la pantalla necesita saber: ¿se puede creer en una cola vacía acá?

La respuesta se clasifica en tres y no se aplana en dos: unsupported es una capacidad que la plataforma no
implementa y la cola nunca se llenará; unavailable es un detector que falló esta vez y puede contestar en la
próxima. Las dos cosas —y no sólo la primera— tienen que impedir que la pantalla diga que la cola está vacía.
Ninguna de las dos es un error que haya que levantar, porque ninguna es un defecto de esta aplicación.

Del lado de la interfaz la regla quedó en la función pura: con la lista vacía y la capacidad en falso, la
oración habla de la máquina; con elementos, ganan los elementos, porque un elemento es un hecho y un informe de
capacidad es contexto; y unknown NO es falso, porque una respuesta que no llegó no es una capacidad que falta.
Se pregunta una vez por corrida y no en cada vuelta del sondeo: la respuesta no cambia mientras la aplicación
vive, y preguntar recorre la tabla de procesos.

Tres falsificaciones, todas con rojo observado y ambos archivos restaurados idénticos (commands.rs 0a35ba53…,
suggestions.ts 65d770bb…): el detector inexistente reportado como instalado, el detector fallado reportado como
funcionando, y la oración de la capacidad que nunca se elige — el rojo fue literal, "No document is waiting for
a decision" donde correspondía hablar de la máquina.

Verificación: oct-desktop en 86 tests (83 más los tres de acá), las trece suites del espacio de trabajo en
verde, clippy limpio, 190 tests de interfaz, chequeo de tipos en 0 y 0, el paquete reconstruido antes que el
binario, y la aplicación relanzada. Que el detector funcione en la máquina del usuario es la evidencia de que
la oración nueva no aparece donde no debe: su cola sigue hablando de documentos.

DECISIÓN DE ENTREGA, tomada por el usuario: seguir trabajando en la rama, sin pull request por ahora; la
estrategia se decide más adelante. Queda anotado que en L57–L60 el registro viajó dentro del commit del código
en vez de en su propio commit como indica la convención del repositorio; desde L61 vuelven a ir separados.

**L62 — el override manual queda postergado al lector propio, por decisión del usuario.**
Preguntado dónde escribe el lector su posición —en el editor de la tarjeta, en la tarjeta misma, o cuando exista
el lector de primera parte— el usuario eligió esperar al lector. Es la respuesta de fondo de R5: los navegadores
y LibreOffice no exponen progreso en ninguna plataforma, así que la posición manual es de carga, y el lector
propio es la superficie donde escribirla deja de ser un parche.

LO QUE QUEDA CONGELADO MIENTRAS TANTO, y es la parte que no se puede perder en una compactación: hoy un dato
manual sería DERROTADO EN SILENCIO por el paso automático. El vocabulario ya existe —ProgressSource::Manual y
Confidence::Manual, y una oración que lo dice—, pero el bucle que observa compara página y total mientras la
biblioteca toma la fila más nueva, así que una lectura manual puede quedar sepultada por una observación
posterior sin que nada lo advierta. Cuando se implemente, la regla es la misma que ya rige el nombre, el autor y
la portada: la posición del lector es la última palabra, y el automático no la pisa.

Lo que la pieza necesita, para cuando llegue: el bucle que observa (crates/oct-desktop/src/track.rs) y la
superficie donde el lector escribe. No es un control suelto: es precedencia, y la precedencia se prueba en el
bucle, no en la pantalla.

**L63 — las revisiones nativas, por fin: aprobadas, y con un defecto crítico real en el medio.**
El usuario pidió reintentar y esta vez el relay del host funcionó: los tres revisores corrieron —34 KB de prompt y
unos 2 KB de resultado cada uno— y el refutador también. El refutador devolvió UN hallazgo grave y propio del
candidato, y tenía razón: en la mitad de interfaz de L61 la cola preguntaba UNA sola vez, antes de que existiera
la respuesta, y nunca volvía a preguntar, mientras toda respuesta que no fuera "instalado" se volvía falsa. Un
fallo de lectura pasajero se convertía así en una afirmación permanente de que esta máquina no puede saber qué
documentos están abiertos: exactamente la frase que el backend se escribió para no decir nunca.

La corrección son cuatro creencias en vez de un flag —unknown, installed, unsupported, unavailable— y una regla:
sólo "unsupported", que es una plataforma que no implementa el detector, habilita hablar de la máquina. Se
pregunta hasta obtener una respuesta definitiva, y un fallo nunca degrada una respuesta definitiva. Queda pinneado
por un test que hace lo que el hallazgo pedía: una sonda que falla seguida de una que responde, y la oración que
se recupera en vez de quedarse pegada.

Dos cosas operativas que costaron intentos y quedan anotadas: el plan de corrección se declaró en 114 líneas de
las 121 de presupuesto —la primera versión medía 154 y hubo que recortar prosa, no código— y, como la revisión es
de rango commiteado, la corrección tuvo que commiteárse (46a8b0c) porque el proveedor responde
corrected_candidate_unavailable a un árbol de trabajo sin commitear. El binding del validador dirigido se rechaza
como "stale" si no se envía el objeto COMPLETO del slot, validationRequest incluido; omitirlo parece un problema
de frescura y costó dos intentos.

El validador dirigido aprobó el candidato corregido y el reconocimiento quemó la autoridad (revisión 8eac6d7e,
linaje review-8f90dc0ff360d6b8, 9 archivos y 241 líneas de alcance original). Quedan DOS hallazgos informativos,
explícitamente no bloqueantes y jamás razón para repetir la revisión: R2-001 (legibilidad, App.svelte:69-71) y
R4-transient-capability-cache (resiliencia, App.svelte:188-193).

T12 CERRADO POR DECISIÓN DEL USUARIO, en sus propias palabras: "que no descubra, puede abrumar al usuario con
tantos documentos". La biblioteca sigue siendo lo que el lector eligió vigilar, y nada más. El tray queda
pendiente de una persona, y los tests que clickeen esperan su decisión sobre una dependencia de DOM.

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
- [ ] **T6** Watched-folder scanner with real page counting via `lopdf` — resolves R2
- [ ] **T7** inotify watcher over watched folders (Linux; per-OS backend later)
- [ ] **T8** `/proc/*/fd` open-document detector — Linux backend of axis B
- [ ] **T12** `RecentDocumentsSource` for `recently-used.xbel` — Linux backend of axis A
- [ ] **T9** Progress provider registry plus Evince GVFS adapter — the first real `ProgressProvider` (`Live`), and the first caller of `Launcher`
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
| T11 | **Done** | `2b8e9da` | Portable POSIX file identity. Reusable `#[cfg(unix)]` module so macOS inherits it; `st_dev`/`st_ino` primary; streamed SHA-256 fallback when either id is zero. 32 tests pass (was 25), clippy clean. Hard links proven to share an identity, and the stub test was replaced rather than deleted. |
| T5 | **Done** | `453a849` + `c2a059d` | `oct-store` is a real persistence layer: six tables, forward-only idempotent migrations, lossless `FileId` round trips and lookup by identity alone, per-platform database location. 47 tests pass (was 32), clippy clean. Two should-fix defects were found by an independent verification pass and fixed before the first commit — see L3 in the log for both, the accepted residual, and the `NULL`-distinct trap the test caught. |

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

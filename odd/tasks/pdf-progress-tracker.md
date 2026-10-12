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
- [ ] **T7** inotify watcher over watched folders (Linux; per-OS backend later) — T7a (mechanism) done; T7b (library wiring) remains; specs below
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
| T6 | **Done** | `57f8c4a` + `a5b3ea5` | Watched-folder scanner with `pdfium` page counting, plus the `folders` API and the project's **first migration (V2)**. 69 tests pass (was 47), clippy clean. Full-library cross-check run independently by the parent *and* by the verifier: **42/42 page counts agree with `pdfinfo`**, 0 documents without a count, 0 extras, whole library in **1.5 s**. The parser was chosen by measurement, not assumption — `spike/parser/FINDINGS.md`, log entries L5 and L6. |
| T7a | **Done** | `e6003e2` + `8355b2d` | The sixth trait `FolderWatcher` (with `WatchTarget`, `WatchEvent`, `WatchEventKind`) plus the Linux inotify backend and the honest `NotSupported` stub. **85 tests pass** (was 69), clippy clean, and both non-Linux targets type-check. Verified twice: attempt 1 found a blocking defect — a directory moved out of a target stayed watched, so events outside the tree were reported under stale paths — plus two should-fix; attempt 2 settled those and found the coalescing window surviving a subtree release. The verification cap was reached there, so the last delta is parent-checked and RED-pinned only — stated plainly in L9. |

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

# oct

A cross-platform desktop app that tracks your PDF reading progress and keeps your library within
reach — without making you dig through your folder tree again.

> **Status: pre-alpha.** Planning is complete and the core is being built. There is nothing
> installable yet, and the interfaces below may still change.

## The problem

If you read PDFs you have probably hit this: a folder tree full of documents, a few started, and no
idea which ones or how far into them you got. Nothing on your system records reading position in a
queryable way, so your reading state dies every time a viewer window closes.

On the machine this was first designed against: **192 PDFs, and 27 of them already held real
reading progress that nothing could show.** Recovering it on first launch is the project's demo,
and it is real, not hypothetical.

## How oct approaches it

**A first-party reader is the core.** oct renders PDFs itself, so progress is exact, live, and
identical on every platform. Reading is not delegated to somebody else's viewer.

**External viewers are a compatibility layer, not the foundation.** oct can also read progress back
from viewers you already use — Evince on Linux, SumatraPDF on Windows, and others — through a
published, versioned *provider* interface that anyone can implement. These adapters are
best-effort by contract: they must degrade to "no data" rather than guess, and the UI says which
source a number came from rather than pretending all progress is equally trustworthy.

**Progress carries its provenance.** A page read at Evince's close, a page typed in by hand, and a
page from the built-in reader are not the same kind of fact, and oct stores them as different
kinds of fact.

**Ingestion is hybrid.** Folders you nominate are the source of truth. PDFs discovered elsewhere
land in a review queue instead of silently joining your library.

## Architecture

```
crates/
  oct-core/       domain model, progress provenance, provider traits — no platform code
  oct-platform/   OS-specific implementations, selected at compile time
  oct-store/      SQLite persistence, migrations, portable file identity
  oct-desktop/    Tauri v2 application shell (window + tray)
```

Three axes are abstracted per platform, each behind a trait:

| Axis | Linux | Windows | macOS |
| --- | --- | --- | --- |
| Recent-document ingestion | `recently-used.xbel` | `IApplicationDocumentLists` | `com.apple.sharedfilelist` |
| Live open-file detection | `/proc/<pid>/fd` | handle enumeration | `libproc` |
| Launch / resume | `xdg-open`, viewer CLI | `ShellExecute` | Launch Services |

File identity is abstracted too: `(dev, inode)` on POSIX, `(volume serial, file index)` on Windows,
with content hashing as the fallback when neither is available.

## Building

Requires Rust (1.98+) and [Bun](https://bun.sh) for the frontend. On Linux, Tauri needs
`libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev` and
`libxdo-dev` from your distribution.

```bash
bun install
cargo build --workspace
```

## Running the tests

```bash
scripts/fetch-pdfium.sh   # once per clone; ~8 MB, gitignored
cargo test --workspace
```

Page counting uses the native `pdfium` library (see
[`spike/parser/FINDINGS.md`](spike/parser/FINDINGS.md) for the measurement that chose it over
`lopdf`). It is loaded at runtime, so it is fetched rather than committed. The tests that prove real
page counting parse actual PDFs: without the library they **fail** with a message naming both
remedies rather than passing without checking anything. Set `OCT_PDFIUM_LIB` to point at a specific
library file or directory instead.

## Status and roadmap

| Phase | Scope | State |
| --- | --- | --- |
| 0 | Validation spikes (progress sources, metadata cost, rendering) | **Done** |
| 1 | Rust core: store, scanner, watcher, provider registry | **In progress** |
| 2 | First-party reader (pdf.js) with live progress | Planned |
| 3 | System tray, quick menu | Planned |
| 4 | Library UI, filters, manual override, review queue | Planned |
| 5 | Packaging, Windows/macOS backends, viewer adapters | Planned |

## License

GPL-3.0. See [LICENSE](LICENSE).

Note that GPL-3.0 makes the macOS App Store permanently unavailable for this project. macOS
distribution is a notarised `.dmg`.

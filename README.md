# mirk

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

## How mirk approaches it

**A first-party reader is the core.** mirk renders PDFs itself, so progress is exact, live, and
identical on every platform. Reading is not delegated to somebody else's viewer.

**External viewers are a compatibility layer, not the foundation.** mirk can also read progress back
from viewers you already use — Evince on Linux, SumatraPDF on Windows, and others — through a
published, versioned *provider* interface that anyone can implement. These adapters are
best-effort by contract: they must degrade to "no data" rather than guess, and the UI says which
source a number came from rather than pretending all progress is equally trustworthy.

**Progress carries its provenance.** A page read at Evince's close, a page typed in by hand, and a
page from the built-in reader are not the same kind of fact, and mirk stores them as different
kinds of fact.

**Ingestion is hybrid.** Folders you nominate are the source of truth. PDFs discovered elsewhere
land in a review queue instead of silently joining your library.

## Architecture

```
crates/
  mirk-core/       domain model, progress provenance, provider traits — no platform code
  mirk-platform/   OS-specific implementations, selected at compile time
  mirk-store/      SQLite persistence, migrations, portable file identity
  mirk-desktop/    Tauri v2 application shell (window + tray)
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

The interface is a Svelte 5 app whose visual system lives in one file,
[`web/src/styles/tokens.css`](web/src/styles/tokens.css). Two fonts ship with it, both under the SIL
Open Font License and both self-hosted rather than fetched from a CDN at runtime: **Inter** for the
interface and **Instrument Serif** for screen titles. Their licence texts sit beside them in
[`web/public/fonts/`](web/public/fonts/).

## Running the tests

```bash
scripts/fetch-pdfium.sh   # once per clone; ~8 MB, gitignored
cargo test --workspace
```

Page counting uses the native `pdfium` library (see
[`spike/parser/FINDINGS.md`](spike/parser/FINDINGS.md) for the measurement that chose it over
`lopdf`). It is loaded at runtime, so it is fetched rather than committed. The tests that prove real
page counting parse actual PDFs: without the library they **fail** with a message naming both
remedies rather than passing without checking anything. Set `MIRK_PDFIUM_LIB` to point at a specific
library file or directory instead.

## Developing

```bash
bun run tauri dev
```

One command, and the app updates while you work. The Tauri CLI runs `bun run dev` first (Vite on
`:1420`), then builds and opens the window pointed at that server: a change to a `.svelte` or `.ts`
file reaches the running window without a restart, and a change under `crates/` rebuilds the Rust
side and relaunches it. Both the hook and the URL are declared in
[`crates/mirk-desktop/tauri.conf.json`](crates/mirk-desktop/tauri.conf.json).

Because `devUrl` is set, **a debug build always expects the dev server**: running the binary on its
own (`cargo run -p mirk-desktop`) opens an empty window unless `bun run dev` is already listening.
That is the price of a live loop, and it is why the loop goes through the CLI. For interface work in
isolation, `bun run dev` in a browser is enough — see “Looking at the interface” below.

## Looking at the interface

The desktop window's size is a stored choice (Ajustes → Ventana): two, three or four documents per
row, or full screen. `scripts/capture-ui.mjs` starts the dev server when it is not already up, opens
Chrome headless at the size that layout names with a device scale factor of 2, seeds a small library
and a stored layout, and writes one PNG per screen plus one per scroll of the screen's own container:

```bash
node scripts/capture-ui.mjs                     # full screen (1440×900), the app's own default
node scripts/capture-ui.mjs --layout columns-2  # 2 per row (430×844)
node scripts/capture-ui.mjs --layout columns-3  # 3 per row (634×844)
node scripts/capture-ui.mjs --layout columns-4  # 4 per row (838×844)
node scripts/capture-ui.mjs --out /tmp/ux       # somewhere else
```

The default is the layout the application opens with when nothing is stored (full screen); naming a
preset captures the grid that preset pins, and the seed also answers `get_window_preset` with the
layout a window leaving full screen returns to, so the capture and a launch agree about both facts.

The seed is injected as a fake `window.__TAURI_INTERNALS__` before the app boots. The application's
IPC bridge (`web/src/lib/tauri.ts`) is not modified: the app still renders honestly in a plain
browser, and the fixtures live in the capture script. A JavaScript exception **or a console error**
during capture exits non-zero, because a screenshot of a crashed page — or of one whose data never
loaded — is not evidence about design. That second half is not decoration: a command the app calls
at startup and the seed does not answer renders the error state, and the exit code is what makes that
impossible to mistake for the real screen.

Chrome is close enough to judge layout, hierarchy and spacing; it is **not** WebKitGTK, so a
capture does not prove paint fidelity on Linux. The output directory is gitignored: captures are
evidence for a review, not artifacts the repository ships.

## Status and roadmap

| Phase | Scope | State |
| --- | --- | --- |
| 0 | Validation spikes (progress sources, metadata cost, rendering) | **Done** |
| 1 | Rust core: store, scanner, watcher, provider registry | **In progress** — everything but recent-document ingestion from `recently-used.xbel` (T12) |
| 2 | First-party reader (pdf.js) with live progress | Planned |
| 3 | System tray, quick menu | **Done** — tray, quick menu and the window's own chrome |
| 4 | Library UI, filters, manual override, review queue | **In progress** — library, filters, sort, review queue and the document editor are in; overriding progress by hand is not |
| 5 | Packaging, Windows/macOS backends, viewer adapters | Planned |

## License

GPL-3.0. See [LICENSE](LICENSE). The licences of everything shipped inside a package — pdfium, the two
bundled typefaces, the system libraries a package requires but does not ship, and the Rust dependency
graph by licence family — are listed in [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).

Note that GPL-3.0 makes the macOS App Store permanently unavailable for this project. macOS
distribution is a notarised `.dmg`.

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md) first: it says how to build and test, what a good change looks
like in this repository, and which gates a pull request has to pass.

# Third-party notices

mirk itself is licensed **GPL-3.0** — see [`LICENSE`](LICENSE). This file lists the third-party
components that are shipped inside a released package, plus the system libraries a package depends on
without shipping. GPL-3.0 requires the first group to be documented; the second is documented because a
user is entitled to know what has to be present for the application to work.

Generated on 2026-11-02 from the dependency graph of `v0.1.0`. Regenerate with the commands in
[“How this list is produced”](#how-this-list-is-produced) rather than editing it by hand.

## Shipped inside the package

| Component | Version | Licence | Where it lands |
| --- | --- | --- | --- |
| **pdfium** (native PDF parser) | build supplied, from [bblanchon/pdfium-binaries](https://github.com/bblanchon/pdfium-binaries) | **BSD-3-Clause** | `libpdfium.so` beside the executable, declared under `bundle.resources` |
| **SQLite** (compiled in by `libsqlite3-sys` with the `bundled` feature) | bundled by `libsqlite3-sys` 0.30.1 | **Public domain** | Compiled into the binary; the Rust wrapper is MIT |
| **Inter** (interface typeface) | latin subset, self-hosted | **SIL OFL 1.1** | `web/public/fonts/inter-latin.woff2` |
| **Instrument Serif** (screen titles) | latin subset, self-hosted | **SIL OFL 1.1** | `web/public/fonts/instrument-serif-latin.woff2` |

The licence texts travel with the components: `vendor/pdfium/LICENSE` after `scripts/fetch-pdfium.sh`
(the directory is gitignored, so it is fetched rather than committed), and
`web/public/fonts/OFL-Inter.txt` / `web/public/fonts/OFL-InstrumentSerif.txt` in the repository.

**pdf.js** is *not* part of this release. The first-party reader is a planned component (Apache-2.0)
and will be added to this file, and to the package, the moment it ships.

## System libraries: required, not shipped

| Component | Licence | Platform | Note |
| --- | --- | --- | --- |
| **WebKitGTK** | LGPL-2.1-or-later | Linux | The WebView engine. A system library the package depends on, never bundled. |
| **GTK 3** | LGPL-2.1-or-later | Linux | Toolkit and widgets. |
| **libayatana-appindicator3** | LGPL-3.0-or-later | Linux | The tray icon. Absent on a stock GNOME 46 desktop, which hides the tray until an AppIndicator extension is installed — a documented limitation of the platform, not of the package. |
| **WebView2** | Microsoft proprietary | Windows | Redistributable as a system runtime. Listed now for completeness; mirk does not package for Windows yet. |

## Rust dependencies

479 packages in the graph, including this workspace's own five GPL-3.0 crates. Almost all of them are
permissive, and the list below is by licence family rather than by package, because that is the fact a
notice has to convey.

| Licences | Packages |
| --- | --- |
| MIT OR Apache-2.0 (both spellings, and the Unicode-3.0 variant of the same pair) | 287 |
| MIT alone | 107 |
| Zlib OR Apache-2.0 OR MIT | 22 |
| Unicode-3.0 | 18 |
| Unlicense OR MIT | 11 |
| BSD-2-Clause / BSD-3-Clause, with or without MIT or Apache | 10 |
| Apache-2.0 WITH LLVM-exception | 6 |
| **MPL-2.0** | **5** — `cssparser`, `cssparser-macros`, `dtoa-short`, `option-ext`, `selectors` |
| GPL-3.0 (this workspace) | 5 |
| **ISC** | **3** — `inotify`, `inotify-sys`, `libloading` |
| MIT OR Apache-2.0 OR LGPL-2.1-or-later | 2 — the `gtk` and `webkit2gtk` bindings, which offer a permissive option |
| Apache-2.0 alone | 2 |
| CC0-1.0 / 0BSD | 1 |

**Total: 479 packages.** The counts are computed, not estimated — they come from the command at the
bottom of this file, and they are meant to be re-run rather than trusted.

Two notes on the non-permissive families, because they are the ones worth naming:

- **MPL-2.0** is file-level copyleft: it is compatible with GPL-3.0, and its obligation — publish
  modifications to *those files* — is satisfied by mirk being GPL-3.0 and public. mirk does not patch
  them; they arrive through the WebView's CSS parser stack.
- **ISC** is a permissive licence that is not MIT (an earlier draft of the project plan assumed MIT and
  was corrected). `inotify` is what watches the reader's folders on Linux, so it is load-bearing rather
  than incidental.

Notable direct dependencies, all permissive: `tauri` (Apache-2.0 OR MIT), `rusqlite` and
`libsqlite3-sys` (MIT), `pdfium-render` (MIT OR Apache-2.0), `png` (MIT OR Apache-2.0), `tray-icon`
(MIT OR Apache-2.0), `muda` (Apache-2.0 OR MIT), `serde` and `thiserror` (MIT OR Apache-2.0),
`webkit2gtk` / `gtk` / `soup3` bindings (MIT — the bindings; the C libraries they bind are the system
libraries listed above).

## Frontend build dependencies

Not redistributed as source, but compiled into the interface bundle or used to produce it: Svelte 5,
Vite, Vitest, happy-dom, TypeScript and svelte-check — all MIT. The bundled output contains mirk's own
code plus the Svelte runtime (MIT).

## How this list is produced

```bash
# Rust: every package, its version and its declared licence
cargo metadata --format-version 1 | python3 -c '
import json,sys,collections
packages = json.load(sys.stdin)["packages"]
for licence, count in collections.Counter(p.get("license") or "(none)" for p in packages).most_common():
    print(f"{count:4d}  {licence}")
'

# The native library and the fonts are not in that graph: they are listed by hand above, and the
# pdfium licence text comes with the download scripts/fetch-pdfium.sh performs.
```

If a release ever ships a dependency this file does not mention, the release is wrong, not this list.

# T6 parser spike: `pdfium` vs `lopdf` on real documents

**Question.** The plan named `lopdf` for T6's real page counting (risk R2). The empirical run of the
scanner showed it stalling on real documents from the maintainer's library. Is `lopdf` viable at
library scale, or does page counting need an FFI parser like `pdfium` — the escape hatch this
document's plan already reserved?

**Answer: `pdfium`, without ambiguity.** On these documents it is correct on 7 of 7, completes in
**≤ 0.1 s**, and holds **6–22 MB** of resident memory regardless of file size. `lopdf` is correct on 5
of 7, **never finishes on 2**, and where it does finish it costs up to **26.3 s** of wall time,
**217 s of CPU** and **290 MB** of RSS.

## Method

- **Harness**: `main.rs` in this directory. It parses one document with one parser and prints a single
  JSON line, self-measuring wall time, CPU time (from `/proc/self/stat`) and **peak** RSS (`VmHWM`, the
  kernel's high-water mark, so a spike cannot be missed).
- **Both arms are release builds.** This matters and it corrected an earlier number — see "Correction".
- **Inputs**: seven documents from the maintainer's library, read-only: the four that the first sweep
  had flagged plus three healthy ones of comparable size. Paths are listed in `run.py` rather than
  committed as data.
- **Bounds**: 60 s per document, applied identically to both arms, enforced by killing the **process
  group**. Stopping a job in this project earlier killed the wrapper but left its grandchild burning
  6.6 CPU cores; a measurement rig that can leak a process is worse than no rig.
- **Ground truth**: `pdfinfo` (poppler), an independent implementation, so agreement is evidence and
  not a tautology. `pdfinfo` reads every document here instantly.
- **Reproduce**: `cargo build --release && python3 run.py` (needs `vendor/lib/libpdfium.so`, from
  `bblanchon/pdfium-binaries`, and `PDFIUM_LIB_DIR` set to `vendor/lib`).

## Results

| Document | Size | `pdfinfo` | `lopdf` | `pdfium` |
|---|---|---|---|---|
| Learning Patterns (Quartz/macOS) | 142.80 MB | 436 | 26.3 s / 217 s CPU / 290 MB | **0.0 s / 6.2 MB** |
| gentleman-programming-book-es (pdf-lib) | 93.58 MB | 313 | **TIMEOUT** | **0.1 s / 22.5 MB** |
| Cracking the Coding Interview (Quartz) | 64.11 MB | 708 | **TIMEOUT** | **0.0 s / 9.5 MB** |
| Historia del Pensamiento III | 34.66 MB | 1015 | 16.8 s / 110 s CPU / 202 MB | **0.0 s / 7.9 MB** |
| Carl G. Jung - El Hombre y sus Simbolos | 34.60 MB | 314 | 0.9 s / 5.6 s CPU / 76 MB | **0.0 s / 6.7 MB** |
| Grokking Data Structures (Adobe) | 17.65 MB | 282 | 1.9 s / 12.8 s CPU / 66 MB | **0.0 s / 6.6 MB** |
| El extranjero - Albert Camus | 0.77 MB | 66 | 0.0 s / 5.6 MB | **0.0 s / 6.1 MB** |

Both parsers returned the **exact** `pdfinfo` count wherever they returned one — there is no accuracy
trade-off here, only a cost and a completeness one.

## What this establishes

1. **`pdfium` is complete where `lopdf` is not.** Two of seven real documents never finish under
   `lopdf` within 60 s. For a library scanner that means the index simply never completes; no timeout
   policy makes a missing page count into a correct one.
2. **The cost difference is ~3 orders of magnitude** on the heavy documents (26.3 s → 0.0 s) and the
   memory difference is 10–40× (290 MB → 6 MB). `pdfium`'s footprint is flat: it does not grow with
   file size the way `lopdf`'s does.
3. **The cause is the shape of the work, not the size of the file.** `lopdf`'s `Document::load` builds
   the entire object graph — every image stream included — while this job needs the page tree and the
   Info dictionary. `pdfium` is built to answer exactly that kind of question. This is the same lesson
   as R12: the cost is content-bound, and choosing the right tool is what changes it.
4. **`lopdf` parallelises and still loses.** 217 s of CPU in 26.3 s of wall time is ~8× parallelism.
   The problem is not a missing thread; it is doing far more work than the question requires.

## Correction to an earlier measurement, recorded on purpose

The first sweep reported **4 of 4** suspected documents as "> 60 s, killed". That sweep ran a **debug**
build. Re-measured in release, two of those four finish — 26.3 s and 16.8 s. The scanner's own
10-minute stall was likewise a debug run.

So the accurate statement is: **in a release build, 2 of these 7 documents exceed 60 s under `lopdf`**,
and the earlier "4 of 42 documents never finish" figure is **pessimistic and must not be carried
forward**. The conclusion is unchanged — a parser that cannot finish two documents out of seven, and
costs 1000× more on the ones it does finish, is the wrong foundation — but the number in the plan had
to be corrected rather than inherited.

## Recommendation

**Adopt `pdfium` for page counting and metadata in `oct-scan`.**

- Licence: BSD-3-Clause, GPL-3.0 compatible. The notice ships with the binary (`vendor/LICENSE`).
- Distribution: `pdfium-render` loads a native library at runtime, so each platform bundles its own
  `libpdfium` (~8 MB). This is consistent with the project's native-package decision, and prebuilt
  binaries exist for Linux, Windows and macOS, x64 and arm64.
- It is the same engine Chrome renders PDFs with, which is why it is fast on exactly the documents
  that hurt.

## Not yet verified — the first follow-ups if this is adopted

> **Update (later the same day):** items 1 and 2 below were verified when the swap landed. `oct-scan`
> now reads Title/Author through pdfium's metadata API (with `None` for absent values) and pdfium
> refuses an encrypted document loaded without a password, which surfaces as a reported failure with
> no page count. Items 3 and 4 remain open, and the whole-library cross-check has since grown to all
> 42 documents: **42/42 counts agree with `pdfinfo`**, and the library indexes in 1.5 s.

1. **Metadata extraction through `pdfium`.** This spike counted pages only. T6's S3 (Title/Author from
   the Info dictionary) must be re-verified against `pdfium`'s API before `lopdf` is removed
   altogether; the current tests prove it for `lopdf`.
2. **Encrypted documents.** The spike did not test one; `pdfium` must be shown to refuse or report them
   the way T6's S4 requires (never a page count of 0).
3. **Bundling and loading on Windows and macOS**, including where the library is expected to live
   beside the packaged application.
4. **`pdfium` initialisation cost per scan**, which the per-document numbers here do not show because
   each measurement is a fresh process.

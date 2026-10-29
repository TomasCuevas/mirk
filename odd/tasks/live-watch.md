# Feature: the watched folders are watched while the application runs

Status: **done** — verified by the user on their own library; the commit waits for approval.
Acceptance (the user's own words): a PDF added with the application running was found in the
library and could be filtered by author.
Created: 2026-10-29
Engram mirror: `odd/live-watch/tasks`

---

## Specs

**S1 — the request, quoted verbatim.**
> "Una vez abierta la aplicacion, si agrego nuevos PDFs en mi carpeta vigilada, no se agregan estos
> nuevos PDFs a la aplicacion."

**S2 — what is actually wrong (evidence, not a guess).**

Nothing in the desktop process ever reads a folder again after it is added. `mirk-scan` has carried
a complete, tested watcher bridge for a long time — `WatchBridge` maps a settled watcher event onto
the scanner's own persist path — and `mirk-platform` has a real recursive inotify backend that also
watches directories created after the watch starts. **Neither is ever constructed by
`mirk-desktop`.** The only ingestion path the application wires is the `scan_folder` command, and
that command *refuses* a folder a watch already covers (`PathCoveredByWatchedFolder`), which is
correct for its own purpose and leaves the reader with no way at all to pick up a new file: removing
the watch would take the reading positions with it.

So the defect is a missing wire, not a broken rule.

**S3 — the watcher runs for the life of the process.**

One bridge over every recorded folder, recursive, built from the same backends the ingestion loop
uses. Each event means what `mirk-scan` already documents and tests: `Added` and `Changed` count and
persist the path through the scanner — the same rules a scan applies, including `pages = None` for a
file that cannot be parsed, never a fabricated `0`; `Removed` is reported and the row is kept, because
reading history outlives the file; `Lost` is reported and the path rescanned; `Unwatchable` is
reported and not rescanned.

**S4 — the watch follows the folders the reader has.**

Adding a folder or removing one changes the set of watched trees, so the armed watch is compared with
the store every few seconds and rebuilt when it differs: a folder added while the application runs is
watched from that moment, and a folder the reader stopped watching is no longer watched. Rebuilding is
a stop-and-start of one thread, not a restart of the process.

**S5 — the interface needs no new plumbing.**

The shell already re-reads the listing every fifteen seconds (`LIBRARY_REFRESH_MS`), which is how it
has always shown anything the desktop process changed behind its back — the suggestion queue relies on
the same clock. A document the watcher persists therefore appears on its own, and no event, channel or
listener is added for it.

**S9 — a launch pass picks up what arrived while the application was closed (R9).**

The watcher cannot report a change that happened before its watches were installed — the bridge's own
contract says the caller scans first and only then watches (S6 of `odd/tasks/window-layout.md`'s
sibling rule in `mirk-scan`). The first wiring skipped that half, so a PDF that arrived while the
application was closed stayed invisible forever: the watch only starts at launch, and re-scanning a
covered folder is refused on purpose. At every (re)arm the desktop process therefore walks each enabled
folder with discovery only — no pdfium — and counts **exactly the paths the library does not already
hold**. A full re-scan is not the answer: it would re-parse every document with pdfium on every launch.

**S6 — a watcher that cannot start does not take the application down.**

A missing pdfium, an exhausted inotify watch limit or a folder that vanished is reported to stderr,
the application keeps running, and the next comparison retries. The reader's library is never left
half-built because a kernel resource ran out.

---

## Tasks

| ID | Task | Specs | Route | Commit |
| --- | --- | --- | --- | --- |
| T1 | The two pure rules: the targets of a folder set, and whether the armed set is stale | S3, S4 | inline | uncommitted (awaiting approval) |
| T2 | The armed watch: one thread, one bridge, a stop flag | S3, S6 | inline | uncommitted (awaiting approval) |
| T3 | The loop that compares and rebuilds, and its place in `setup` | S4, S6 | inline | uncommitted (awaiting approval) |
| T4 | An end-to-end test with the real inotify watcher: a PDF written after the watch starts | S2, S3 | inline | uncommitted (awaiting approval) |
| T5 | Documentation: this record, and the README line that called the watcher merely in progress | S1–S6 | inline | uncommitted (awaiting approval) |

---

## Log

**L1 — the request, verbatim.**
> "Una vez abierta la aplicacion, si agrego nuevos PDFs en mi carpeta vigilada, no se agregan estos
> nuevos PDFs a la aplicacion."

**L2 — why this was invisible until it was used.**

Every test the watcher has drives it with an injected fake, which is exactly what the module's own
contract asks for. What no test could say is whether anybody ever *called* it: the composition root
is the one place an unwired dependency is invisible, and `cargo test` was green the whole time. The
end-to-end test in T4 is the answer — it uses the real backend and the real store, so a future
unwiring fails a test instead of waiting for a reader to notice.

**L3 — the wire, the falsification, and what stayed unshared.**

`crates/mirk-desktop/src/watch.rs` holds the two pure rules (the targets of a folder set, and whether
the armed set is stale), the armed watch, and the thread body; `main.rs` gained `start_watching`,
which compares the store's folders every five seconds and rebuilds the watch when the set changed.

The end-to-end test is the point: it uses the real inotify backend, a file-backed store and the
repository's own committed PDF, records the folder, starts the watch, *then* writes `New Arrival.pdf`,
and asserts that a **second connection** to the same database lists it. It was falsified before being
trusted — with the watch thread returning immediately (which is precisely the old behaviour: nobody
ever constructed the bridge) the test fails with "a PDF written after the watch started must enter the
library", and the unmodified code passes it in under a second.

No event, channel or listener was added to the interface: the shell already re-reads the listing every
fifteen seconds, which is how it has always shown anything the desktop process changed behind its back
(S5). The cost of that choice is a listed document appearing within fifteen seconds rather than
instantly, which is the trade the suggestion queue already accepted.

Checks: `cargo test -p mirk-desktop` 111 passed (107 before; three are this feature's), `bun run
typecheck` 0/0, `bun run test` 384 passed.

**R8 — the watcher deadlocked every cover in the process.**
> "Con estos nuevos cambios introdujiste un nuevo bug. Este bug es que no se establecen las portadas
> correctamente. Unicamente estan colocadas las portadas donde se han establecido con imagenes, las
> portadas obtenida de las paginas del documento, no se cargan"

My own defect, one build old, and the record keeps it because the fix established a rule the whole
process depends on.

`pdfium-render` is built with its `thread_safe` feature, which enforces pdfium's single-threadedness
with **one process-wide lock taken by `FPDF_InitLibrary` and released only by the matching
`FPDF_DestroyLibrary`** — that is, held for as long as any `Pdfium` value is alive. The bridge I wired
bound one instance before its watch loop and kept it for the life of the process. So the watcher thread
held pdfium's lock forever, and `document_cover` on any other thread waited on it forever: the page
cover never arrived, the frontend promise never resolved, and the card kept its placeholder. Image
covers still worked because they never touch pdfium. Nothing was logged, because no call ever failed —
they simply never returned. That silence is what made the symptom look like "the covers are missing"
rather than "a thread is stuck".

`WatchBridge::watch` now probes the library once and releases it before installing any watch (keeping
the one-actionable-error contract), then binds one instance per event, uses it, and drops it **before
reporting the outcome and before the loop waits again**. `Removed` and `Unwatchable` never bind at all.
A bind that fails after the probe is reported as that path's `Failed`, so nothing is swallowed.

The regression test is the deadlock itself, not a proxy: while the bridge is watching, a second thread
binds pdfium and must answer within five seconds. Before the fix it timed out; after it, it returns in
under a tenth of a second. That test is the reason this class of bug cannot come back silently.

Checks: `cargo test -p mirk-scan` 31 passed, `cargo test -p mirk-desktop` 111 passed, `cargo check` on
both crates with zero warnings.

**L5 — the missing launch pass, and the log line that found it.**
> "Hermano, sigue sin leer los nuevos PDFs agregados. Agregue un nuevo PDF de una obra de Carl Sagan y
> no aparece, solo aparece Cosmos que esta desde antes"

The watcher was already correct: reproducing with a temporary data directory, a PDF copied into a
watched folder after launch entered the library with its real page count. What the log then showed on
the real profile was `watching: /home/tomas-cuevas/Documents/Mega/Libros (recursive)` — the folder was
watched, so the event was not the problem. The file had arrived between two restarts.

Two defects, both mine, both silence-shaped:

- **No launch pass.** Files added while the application was closed had no path into the library at all.
  `watch::unseen` (discovery only) plus `watch::catch_up` (count the unseen paths) now run after the
  watch is armed, so the watch covers the moment before the pass and the pass covers everything
  before that. On the first launch with it, the log named the user's own missing document and nothing
  else: `catch-up: El mundo y sus demonios - Carl Sagan.pdf entered the library`.
- **A silent "watching nothing".** An empty or disabled folder set armed no watch and said nothing,
  which is indistinguishable from a broken watcher. Arming now prints one line per folder — or one
  line saying there is nothing to watch — so the next report of this kind is answered by the log
  instead of by a guess.

Checks: `cargo test -p mirk-desktop` 114 passed (three are this pass's), `cargo test -p mirk-scan` 31,
`cargo check` zero warnings on both.

# Engram memory migration: `tomas-cuevas` → `oct`

Data-relocation record. Six Engram observations about the **oct** project were written into the
wrong Engram project (`tomas-cuevas`) because Engram inferred the project from a stray empty
`~/.git` at `$HOME` before the repository existed. This document records their relocation into the
correct project `oct`. The originals were removed from `tomas-cuevas` by the parent only **after** the copies were
independently verified byte-for-byte; see "Step 6" for that evidence.

## Step 1 — Destination confirmation and probe

`mem_current_project` (Engram HTTP `/project/current`) invoked with `cwd` set to the repository
root returned, verbatim:

```json
{"available_projects":null,"cwd":"/home/tomas-cuevas/Workspace/Proyectos/oct","project":"oct","project_path":"/home/tomas-cuevas/Workspace/Proyectos/oct","project_source":"git_root"}
```

Resolved project: **`oct`**, source: **`git_root`** — as required.

Probe write: a small probe observation was written from the working session
(`mem_save`, session id `01a1307a-fa9b-7224-b9d1-99fa01216f6a`) and read back. It landed in:

| Probe id | Title | Landed project | Result |
| --- | --- | --- | --- |
| 5970 | Probe: engram write target check before oct migration | `oct` | PASS — session writes bind to `oct` |

Because the probe landed in `oct` and not anywhere else, the migration proceeded.

## Step 2/3 — Source → destination mapping

Six observations were read from `tomas-cuevas` and re-created in `oct`. Content was copied
byte-for-byte (not summarised, shortened, or rewritten); `title`, `type`, `scope` and `topic_key`
were preserved exactly.

| Source id (`tomas-cuevas`) | New id (`oct`) | Title | Outcome |
| --- | --- | --- | --- |
| 5938 | 5971 | Hallazgos de viabilidad para app desktop de seguimiento de progreso de PDFs en este host | copied |
| 5939 | 5972 | Feature plan persisted: PDF progress tracker (Tauri v2, hybrid ingestion, external viewers) | copied |
| 5940 | 5973 | Phase 0 results: 27 PDFs already hold invisible reading progress (max p.524); resume via `evince -i` confirmed | copied |
| 5941 | 5974 | T1 solved: Evince writes page metadata LIVE (not on close) + hard off-by-one between `-i` (1-based) and stored page (0-based) | copied |
| 5956 | 5975 | R12 resolved: WebKitGTK renders pdf.js at parity with Chrome — the cost is content-bound, not engine-bound | copied |
| 5959 | 5976 | T4 landed: oct workspace scaffolded with object-safe platform traits (commit b34ec4f) | copied |

## 5951 vs 5956 reconciliation (R12 spike)

Observation **5951 already existed in `oct`** (written by a subagent, session id ending
`@oct`) and describes the same R12 WebKitGTK/pdf.js spike as **5956**:

- **5951** — English, compact; title "R12 spike: pdf.js on WebKitGTK 2.52.6 is at parity with
  Chrome for PDF rendering"; topic_key `oct/r12/webkitgtk-pdfjs-rendering-spike`.
- **5956** — Spanish, much richer numeric detail; title "R12 resolved: …"; topic_key
  `odd/pdf-progress-tracker/r12-webkitgtk-spike`.

**Decision: copied (5956 → 5975), not skipped.** 5951 is an adequate summary of the spike's
*conclusion*, but it is not a superset of 5956. 5956 carries substantive facts absent from 5951:

- the split "first page render only" metric (WebKitGTK 2827 ms vs Chrome 4004 ms) alongside the
  fetch-inclusive figures;
- the five-item **residual gaps** list (continuous scrolling / text layer / annotation layer never
  measured; range streaming never exercised; ±30 % first-page spread; `hardwareConcurrency` 8 vs 12)
  that downgrades rather than retires R12;
- the `-f` `pgrep` false-positive lesson (verify processes by name, not by full command line);
- the throwaway-profile cleanup note and the "Phase 0 complete / decisions locked" status.

Since the stated purpose of this migration is to preserve detail verbatim, and since the originals
in `tomas-cuevas` are scheduled for deletion, skipping would permanently lose those facts. Copying
is reversible (the duplicate can be deleted later) whereas skipping is not. Both observations now
coexist in `oct` with distinct `topic_key`s, so neither collides; if the reviewers prefer a single
record, the new copy **5975** is the one to remove.

## Verification

Each destination observation was read back from Engram and compared field-by-field against its
source. For all six pairs, `project == "oct"` and `title`, `type`, `scope`, `topic_key` and the
`content` string matched **byte-for-byte** (content lengths identical: 3028, 3850, 2720, 3017,
4614, 4103 chars). A migration-summary observation (id **5977**, type `config`, project `oct`) was
also recorded. Project observation counts after migration: `oct` = 9 (1 pre-existing 5951, 1 probe
5970, 6 copies, 1 summary 5977), `tomas-cuevas` = 35 (unchanged).

## Method note

The copies were written through the Engram HTTP endpoint `POST /observations` (localhost:7437) —
the exact backend call the Pi-native `mem_save` tool makes (`fetch("/observations", {method:"POST",
body:{session_id, title, content, type, project, scope, topic_key}})`). They were created by
round-tripping the source JSON directly rather than retyping the text, which guarantees the
verbatim requirement; `session_id` was the same registered session that produced the passing probe.

## Not done here (parent-owned)

- No observation in `tomas-cuevas` was deleted, updated or otherwise modified.
- No git operation was performed (no stage/commit/push).

## Step 6 — Independent parent verification, then deletion of the originals

The parent did not accept this migration on trust. Every copy was compared against its source over
the Engram HTTP API, on `title`, `type`, `scope`, `content` and `topic_key`:

| Source | Copy | Source project | Copy project | Fields identical | Content bytes |
| --- | --- | --- | --- | --- | --- |
| 5938 | 5971 | tomas-cuevas | oct | yes | 3028 |
| 5939 | 5972 | tomas-cuevas | oct | yes | 3850 |
| 5940 | 5973 | tomas-cuevas | oct | yes | 2720 |
| 5941 | 5974 | tomas-cuevas | oct | yes | 3017 |
| 5956 | 5975 | tomas-cuevas | oct | yes | 4614 |
| 5959 | 5976 | tomas-cuevas | oct | yes | 4103 |

Only after that comparison passed were the six originals soft-deleted (recoverable, not hard
deleted). Project observation counts before and after:

| Project | Before | After |
| --- | --- | --- |
| `tomas-cuevas` | 35 | **29** |
| `oct` | 9 | 9 (6 copies replaced 0 previously stranded entries; the remainder are the pre-existing 5951, this migration's probe 5970 and summary 5977) |

A direct read of each original id now fails, and each copy reads back with `project: oct`.

## Open items

- **5975 duplicates 5951** on the R12 spike, deliberately: 5951 summarises the conclusion, 5956
  (now 5975) carries the residual-gaps list and the raw numbers. Remove 5975 if a single record is
  preferred.
- **5970 and 5977 are process artifacts** from this migration and could be removed for cleanliness.
  The parent session cannot do it: its Engram writes to `oct` are refused, so both require the new
  session.
- The **empty `~/.git` at `/home/tomas-cuevas`** that caused the misattribution is still present. It
  also underlies risk R6. Removing it is a user decision, outside this repository's scope.

# Roadmap — future versions of mirk

Status: **proposal** — nothing in this document is scheduled, authorized or started. It is the
reconciled form of the owner's list (L1), measured against the repository at `26eeead` (v0.1.0
published). It records *what each requested feature actually costs in this codebase* and the order
that cost implies.

Created: 2026-11-03 · Owner decision pending on D1–D10 · Revised after L3 (the reader is now 0.2.0).

---

## Source (L1, verbatim)

> Por el momento no voy a desarrollar en Mirk por unos dias, lo que vamos a hacer ahora es una lista
> de funcionalidades que deberian tener futuras versiones:
>
> - Analiticas sobre el uso de la aplicacion. Tiempo de lectura, documentos leidos, documentos
>   completados, documentos completados varias veces, documentos abandonas y varios casos mas.
> - Pomodoro integrado.
> - Perfiles diferentes, para poder separar mejor los documentos. Por ejemplo, podria tener un perfil
>   para estudios y otro perfil para ocio.
> - Cambio de tema, entre light y dark.
> - Notificaciones.
> - Desarrollar un logo y junto a el, un branding mas marcado y profesional.

---

## What exists today (measured, not assumed)

| Thing | State | Evidence |
| --- | --- | --- |
| Screens | `home`, `library`, `settings` (+ `#/suggestions`) | `web/src/lib/screens.ts` |
| Rendering a PDF | **No.** `open_document` hands the path to an external viewer | `crates/mirk-desktop/src/commands.rs:1177` |
| Rendering evidence | **Yes.** pdf.js 6.4.299 measured in WebKitGTK and Chrome against two real PDFs; Tauri + pdf.js locked, pdfium FFI rejected as premature | `spike/r12/FINDINGS.md` |
| `pdfium` | Used for page counts and cover thumbnails only | `crates/mirk-scan/src` |
| Reading progress | `progress` table: `page`, `total`, `source`, `confidence`, `updated_at` — imported from Evince et al., or typed by hand | `crates/mirk-store/src/lib.rs:179` |
| Open history | `open_events`: `started_at`, `ended_at`, `pid`, `viewer_id` — from live detection of external viewers | `crates/mirk-store/src/lib.rs:191` |
| Persistence | SQLite, forward-only migrations gated by `PRAGMA user_version`, currently V11 | `crates/mirk-store/src/lib.rs` |
| Settings | Key/value table with a whitelist on read (unknown value → "not stored") | `commands.rs:1091–1152` |
| Visual system | One token file, **dark only**, plus a CSS-built aurora ground | `web/src/styles/tokens.css`, `App.svelte:946` |
| App icon | The scaffold's placeholder, with its own note that "mirk has no designer" | `crates/mirk-desktop/icons/generate.py` |
| Release path | CI builds, signed release, updater manifest, `.deb` installed and opened | `odd/tasks/ci-and-release.md` |
| i18n | `en`, `es` | `web/src/lib/i18n.ts:15` |

## Three findings that decide the order

**F-1. Half the list silently assumes a first-party reader that does not exist — and the owner
confirmed it is the thing to build (L3).**
mirk renders no PDF page today. Every "reading" fact it holds came from *another* program's window.
So "tiempo de lectura" is currently unmeasurable, and a Pomodoro has nothing to focus on. It was
absent from L1 because the README already promises it, not because it was optional; the strategy was
already locked (`pdf-progress-tracker.md`: *"First-party reader (pdf.js in the WebView) is the
strategic core"*) and the performance question was already spiked (`spike/r12/`). It is now the
first proposed version — see F0.

**F-2. Facts accumulate once; metrics can be recomputed forever. The reverse is not true.**
Any day mirk runs without recording *reading sessions* is a day of history that can never be
rebuilt. A metric's formula can change in an afternoon; missing rows cannot. That is why session
logging is proposed as invisible plumbing in the *first* version after this one, months before the
analytics screen exists.

**F-3. Visible-and-cheap first, irreversible-last.**
Branding and the theme give the most external feedback for the least data-model risk. Profiles are
the opposite: they re-scope every screen, every number and every query, and a bad profile model is
expensive to undo once real libraries live in it.

---

## Proposed version plan

| Version | Theme | Contents | Why here |
| --- | --- | --- | --- |
| **0.2.0** | **Reader** | The first-party reader (F0) — **reduced in L4 to the read-only half: open, scroll, remember** — **plus** the reading-session facts it is the only honest source of (F1 part 1). Text layer, selection and find are not in this version. | It is the product's own promise, it is already designed and spiked, and it is the only place where "tiempo de lectura" stops being a guess (F-2) |
| **0.3.0** | Identity | Logo + branding (F6), light/dark theme (F4) | Cheap and visible, and it lands after the reader, because the reader is what establishes the page-on-field rule the light theme has to generalise |
| **0.4.0** | Time | Analytics screen over the facts 0.2.0 has been collecting (F1 part 2) | Needs accumulated rows to be worth showing; the screen is presentation over stored facts |
| **0.5.0** | Focus | Pomodoro (F2) + notifications as its delivery channel (F5) | A Pomodoro without notifications is a timer you forget you started; and by now there is a reader to focus on |
| **0.6.0** | Spaces | Profiles (F3) | Last, on purpose: it re-scopes everything and is the least reversible |

Three of these cannot be reordered: session facts before analytics, the reader before the focus
mode, and analytics before profiles. The visual pair (F6 + F4) is the flexible one — it can be
pulled ahead of 0.2.0 if a marketing-visible release matters more than the reading itself, at the
cost of one more public release carrying the placeholder icon and no reader. That is D9.

**A hard warning about 0.2.0's size.** Every feature shipped so far — the UI round, the window
layout, the navigation, the suggestions screen — was a task of one feature document. The reader is
not. Expect it to be its own feature document with five or six tasks, published over more than one
release if it has to be. Do not bundle it with anything else that can wait.

---

## F0 — The first-party reader

**This is the keystone, and the least uncertain of the six-plus-one.** The strategy is locked and the
performance question was answered with measurements against the owner's real PDFs before any code
was written.

Already established (`pdf-progress-tracker.md`, `spike/r12/FINDINGS.md`):

- **Tauri stays, pdf.js renders.** No Electron migration, no pdfium FFI rewrite. pdfium remains the
  escape hatch behind one module if the in-Tauri re-verification fails.
- **WebKitGTK is not the weak link.** On a 136 MB / 436-page PDF pdf.js 6.4.299 rendered at rough
  parity with Chrome 155 (throughput 1.21 vs 1.03 pages/s), with zero console errors.
- **Cost is content-bound, not engine-bound.** Four pages of that book each carry a single
  76-megapixel image and cost 2.7–5.4 s on *both* engines; every other page costs 13–283 ms.

What the spike deliberately did **not** answer, and what makes the reader real work:

1. **Getting bytes to pdf.js.** A Tauri window has no HTTP origin for a local file. Two shapes:
   load the whole file into an `ArrayBuffer` over IPC, or implement a Range-capable custom URI
   scheme in Rust. The measured consequence is not subtle — 2.8 s to render page one, or 4.95 s
   including a fetch that was *localhost*, never a 136 MB file read through IPC; the spike's server
   recorded `rangedRequests: 0` because it was handed a buffer. Range streaming is the difference
   between a reader that opens a big book and one that hangs (D8).
2. **Continuous scroll, virtualisation, and cancellation.** The core reading interaction of this
   product, and explicitly unmeasured. Needs page-slot sizing from the viewport at scale, a render
   window around the visible area, cancellation when the reader scrolls faster than renders finish,
   and a correct re-render on zoom.
3. **Pathological pages.** Without a policy the app is broken on exactly the documents that made
   this project exist. Options: progressive render (a scale-down first, then the real one), a
   placeholder with an honest "rendering" state, or a per-document render budget.
4. **Memory.** Canvases for a 436-page document must be evicted, and WebKitGTK on Wayland is where
   that will surface first.
5. **The text layer.** Selection and search live in a second layer with its own cost and its own CSS
   scaling — and it is the foundation for highlights later.
6. **Tauri's own webview settings.** Every number in the spike is "the engine can do this", not "the
   Tauri app will do this": hardware acceleration, DMABUF and compositing are configured
   differently. **Re-measure inside the real app before anything is built on top of the numbers.**

**Why it must ship with the session facts.** The reader is where page turns, dwell time and
open/close are born — precisely the facts F1 needs and cannot get anywhere else. Building the reader
and `reading_sessions` in one version means reading time is *exact* from the first release that has
a reader, instead of approximated over external viewers and rewritten later. It is also where
`progress.source = "reader"` earns its highest confidence: the provenance model finally gets its
first real citizen.

The design questions the reader owns (D10): what the reading surface looks like on mirk's dark field
(the page needs a shadow or an edge — and that rule is what the light theme later generalises), what
happens to the three-destination navbar while a document is open, and whether a document opens in
place or takes over the window.

Suggested slicing (one feature document, `odd/tasks/reader.md`):

| Task | What it proves |
| --- | --- |
| T1 | Tauri re-verification: pdf.js 6.4.299 vendored, range-capable protocol, one page rendered inside the real window |
| T2 | Continuous scroll with virtualisation, cancellation and zoom |
| T3 | Session + page facts written from the reader; progress with `source = reader` |
| T4 | Text layer, selection, and find-in-document |
| T5 | Interaction matrix: fit, zoom, keyboard, trackpad, jump-to-page, resume where you left |
| T6 | Pathological pages and memory eviction |

Effort: **XL**, the largest thing this project has attempted. Depends on: nothing. Blocks: F2 (a
focus mode with nothing to focus on is a timer), and it upgrades F1 from approximation to fact.

## F6 — Logo and branding

**Why it is not a later nicety.** mirk already publishes: `v0.1.0` has signed artifacts, a tray
icon, a `.deb` icon, `.ico`/`.icns`, store logos, a README, a GitHub repository preview and an updater
manifest. All of it currently carries the scaffold's placeholder — a dark slate tile with a blue
square, whose own generator says the palette is "the one the scaffold chose" and that "mirk has no
designer". It is also the only item on this list that is **already shipping**, at 16px, in the tray.

Deliverables:

1. The mark: one concept, delivered as a single-colour SVG plus a full-colour variant, legible at
   16px and at 512px.
2. The icon set derived from one 1024px source through `bun tauri icon`, replacing `source.png`
    and `generate.py` (that script's palette note must not outlive it).
3. Tray variants: monochrome for Linux panels and a macOS template image.
4. A wordmark and the ownership of `--brand`, which is currently `#e6a3c9` (the aurora's pink
   crest) and must become the mark's own colour rather than a wallpaper leftover.
5. Light **and** dark usage rules — the mark cannot be finished before F4 decides what light is.
6. README hero, 1280×640 social preview, and a short identity section in the design notes.

**The brief has to answer one question before any drawing:** what does the name say? The current
identity is *light in a dark field* (aurora, pink crest, glass). A mark that reads as a lit page
edge, a folded corner with light behind it, or an arc of progress are three different products.

Effort: **S–M**. Right tool: the gentle-design canvas, directions first, one artboard per direction,
then implement the chosen one.

## F4 — Light and dark theme

**The mechanism is cheap; the design is not.** `tokens.css` already names roles instead of hex
values, so the switching machinery is a `data-theme` attribute on `<html>`, a `prefers-color-scheme`
listener, and one `settings` key — the same whitelist pattern the window layout already uses.

What is *not* mechanical:

- **The aurora ground.** The app's darkness is not a token; it is the canvas colour plus a
  four-layer radial gradient in `App.svelte`. A light theme either needs its own ground or loses the
  identity that the whole visual system was built around.
- **Glass.** Translucent white over near-black reads as a pane. Translucent white over near-white
  reads as nothing. Light needs a different recipe (tint, shadow, hairline), not a different opacity.
- **The progress ramp.** In mirk a hue *states a fact about a document* — this is the one saturated
  thing on a screen. Every step must be re-derived for contrast against a light field, and it must
  still read as an ordered ramp.
- **The PDF page.** A rendered page is white. On a dark field it needs a shadow to separate itself;
  on a light field it needs an edge. That is a reader concern (F-1), so 0.2.0 must at least decide the
  rule.
- **The loud control** is white-with-black-text. In light it inverts, and that is a design decision,
  not a variable rename.
- **Contrast must be measured, not eyeballed.** Both themes should be pinned by a test that parses
  the token file and asserts a measured ratio per text role. Cheap, and it is the same discipline
  the rest of the project already applies to invariants.

**Three states, not two:** `system | light | dark`, with `system` as the default. A two-state toggle
is the bug people report in every app that ships one.

Effort: **M** (the token work is hours; the light identity is the real cost). Depends on F6 for the
icon variants and on a decision (D4).

## F1 — Analytics

**This is the big one, and the only one whose cost is mostly *correctness*, not UI.**

What it needs that does not exist:

- A **session fact**: `reading_sessions(doc_id, started_at, ended_at, active_ms, source, viewer_id,
  ended_reason)`. `open_events` looks like this but is not: it tracks a *process*, not reading.
- A **page fact** if per-document depth is wanted: which pages were actually on screen, and for how
  long. Without the reader this can only come from external viewers that record pages — which is
  precisely the provenance problem mirk already models.
- A **completion fact**: `completions(doc_id, at, source)`. "Completed several times" is impossible
  from a snapshot — a `progress.page == total` comparison can only ever be true or false, never
  *how many times*. It needs its own event.
- **Written rules**, not stored numbers: `is_read`, `is_completed`, `is_abandoned` as pure functions
  (Rust in `mirk-core`, mirrored in TypeScript for presentation) with thresholds stated in the
  interface. Metrics are recomputed on read; nothing derived is ever persisted, or the two will
  disagree the day a threshold changes.
- A `#/stats` screen, built the way `#/suggestions` was: pure helpers with tests, then components.

The decisions that define the feature (D1, D2, D3):

- **Completion**: last page reached? a share of the pages actually visited? an explicit "terminado"
  action? For a reference work you read in fragments, "last page" is the wrong measure; for a novel
  it is the right one.
- **Abandonment**: no session for N days *and* under X% — or an explicit state the reader sets.
- **Time**: idle is not reading. A window in the background, or a page that has not turned in 20
  minutes, must not accumulate. `active_ms` has to be defined, or every number on the screen is a
  lie the project cannot defend.

**Risk to guard:** the whole point of the interface is that a number states where it came from. An
analytics screen that reports "3 h 20 min leídas" when half of it is an Evince window left open
overnight breaks the one rule this product has. Every aggregate needs its `source` and a
one-line explanation on screen.

Effort: **L–XL** (the largest item after the reader). Collection in 0.2.0 is M; the screen plus rules
in 0.3.0 is M–L. Storage cost is nil at this library size; no retention policy needed.

## F2 — Pomodoro

**The cheap version is a timer, and a timer is a phone app.** The version that belongs in mirk is a
*focus session bound to a document*: 25 minutes on this PDF, the chrome quieting down, and the
session written into the same fact table as everything else — so it feeds analytics instead of being
a separate toy.

Constraints that are not negotiable:

- **The clock lives in Rust**, not in the webview. A hidden-to-tray window or a suspended webview
  throttles timers; a countdown authored in JavaScript will report 25 minutes after 40.
- **No fourth destination.** The navbar is three destinations by design (`screens.ts`); a Pomodoro is
  a chip in the top bar plus a tray countdown, or a mode entered from a document — not a screen.
- **Overlap with reading time must be decided**: a focus block and a reading session on the same
  document are the same minutes. Either the focus session *is* the session, or the stats screen shows
  them as distinct and never sums them.
- **Recovery**: a focus block interrupted by a crash, an OS sleep or a window close needs a stated
  outcome (counted, discarded, resumed).
- Settings: work/break lengths, cycles, and whether breaks notify (that is F5).

Effort: **M** once F1's fact table exists and F5's channel exists; **L** with the reader-integrated
focus mode.

## F5 — Notifications

On its own this is not a version; it is a channel other features speak through. It has value only
once there is something worth interrupting a person for. Ranked by justification:

1. Pomodoro phase end (0.4.0 — this is the pair that makes F2 real).
2. "N documents are waiting in the review queue" — the suggestions queue already exists and today
   the reader has to go look.
3. A folder scan finished or failed — a scan of a large folder currently reports nowhere.
4. An update is available — the updater works; the notice is in-app only.

Mechanics and traps: Tauri's notification plugin, a permission prompt that must be asked *at the
first thing that wants to notify* and never at launch, a "never notify while the window is focused
and visible" rule, and per-OS reality (Linux needs a notification daemon over DBus; the tray already
proved GNOME is the awkward platform here — see `pdf-progress-tracker.md`'s tray findings). Also: no
notification is a valid, permanent choice, so the switch has to be per-trigger, not one global mute.

Effort: **S** for the channel, plus one small task per trigger.

## F3 — Profiles

**This is the item that changes the shape of everything else, which is why it goes last.**

Three shapes, and the choice is the whole feature:

| Shape | What it means | Cost | Risk |
| --- | --- | --- | --- |
| **A. Scope** | A profile owns folders; a document belongs to a profile; library, suggestions and stats are filtered | `profile_id` on folders/documents/suggestions, one migration, every list query gains a predicate | The same PDF reachable from two folders has no single answer; progress becomes ambiguous |
| **B. Filter** | One library, profiles are saved filters over folders/authors | Weeks, no migration, no ambiguity | Does not do what was asked: it separates the *view*, not the documents or the numbers |
| **C. Separate stores** | One SQLite file per profile | Strongest separation; no query changes; every number is naturally per-profile | Switching = reconnecting the store; watcher, tray, settings and updater all become per-profile |

The elegant variant of A: **membership is derived from folders**, which are already the source of
truth. A profile is a named set of folders, so no per-document membership table is needed and moving
a PDF between folders moves it between profiles — which is exactly the philosophy the ingestion
design already commits to.

**Recommended path: build B first, then promote.** A profile that is a saved filter costs almost
nothing, answers "estudios vs. ocio" in the *view* immediately, and produces the evidence about
whether the owner really wants separation of *facts* (progress, stats, suggestions per profile) or
just of *screens*. If the answer is the former, C is cheaper than A. Note the ordering consequence
for F1: sessions and completions are keyed by document, never by profile, so that whatever profile
model wins later is a scope at read time and never a rewrite of the facts.

Effort: **S** for B, **L** for A, **M** for C.

---

## Open decisions (owner)

- **D1 — Completion rule.** Last page, a share of visited pages, or an explicit action? (Changes what
  "documentos completados" means forever.)
- **D2 — Abandonment rule.** Days without a session, under a depth threshold, or an explicit state?
- **D3 — Does external-viewer time count as reading time?** Mirk can measure it; it cannot vouch for
  it. Counting it quietly contradicts the product's own provenance rule.
- **D4 — Is light theme "the same identity in a light key", or its own look, and does it keep the
  aurora?**
- **D5 — What does pomodoro focus: a document, or the app?** And do focus minutes and reading minutes
  add up or are they the same minutes?
- **D6 — Profiles: separation of views, or of facts?** (Decides B vs. A/C.)
- **D7 — The reader.** ~~Is it next?~~ **Answered in L3: yes, it is key.** ~~What remains is the slicing: does 0.2.0 ship the whole reader?~~ **Answered in L4: the read-only half — open, scroll, remember.** Text layer, selection and find leave 0.2.0; this document does not assign them a version, because doing so would displace the visual pair or the facts they depend on.
- **D8 — How pdf.js gets its bytes.** Range-capable custom URI scheme in Rust (correct, more work) or
  whole-file `ArrayBuffer` over IPC (fast to build, visibly bad on a 136 MB book). Recommend the
  protocol: it is the one decision here that cannot be deferred without shipping a slow reader.
- **D9 — Does the visual pair (logo + theme) go before the reader?** Recommended: after. Cost of the
  other order: one more public release with the placeholder identity and no reader.
- **D10 — What a document does to the shell.** Take over the window, open inside the current screen,
  or a fourth destination; and whether the bottom navbar survives while reading.

## Not on the list, but arriving

Named so they are not a surprise later: full-text search across the library; tags and highlights;
synopsis/annotation export; keyboard-first navigation; EPUB or CBZ; a watch-mode reading session that
respects the OS; and sync between machines (which is the first item that forces a decision about
where facts live, and makes profiles more attractive in shape C).

---

## Log

**L1** — the owner's request, verbatim, quoted at the top of this document. Six features, no
versions named, development paused for a few days.

**L2** — the exploration that produced the ordering: screens, commands, schema, tokens, icon
generator and release record read directly. The decisive finding is F-1 (no first-party reader), which
the owner's list does not mention and which two of the six features depend on.

**L3** — the owner's answer, verbatim:

> Claro, habia olvidado por completo el visualizador de PDF propio. Eso seria algo clave a
> desarrollar

Consequence: the reader moves from "unnamed prerequisite" to **0.2.0**, and everything else shifts
one version with it. The strategy it inherits is already locked and spiked (`pdf-progress-tracker.md`,
`spike/r12/`), so what this document adds is what remains unmeasured and the five engineering
problems the spike did not touch.

**L4** — the owner's answer, verbatim:

> Si, hay que reducir 0.2.0 urgente

Consequence: **D7 is answered — 0.2.0 is the read-only half.** Open, scroll, remember. The text layer,
selection and find are out of the version, and this document deliberately does not decide where they
land: assigning them to 0.3.0 would push the visual pair, and assigning them to 0.4.0 would put them
ahead of the analytics screen that needs their facts. That is a decision for after 0.2.0 ships.

The measured state of the effort this reduces, including what is kept and what is archived, is in
`odd/0.2.0-state.md`; the reduced task tree is `odd/tasks/reader.md`, tasks R1–R4. Nothing in the
F0/F1 analysis above is invalidated by the cut: the reader is still the keystone and the session facts
still have to be born with it, because the read-only half is exactly what produces them.

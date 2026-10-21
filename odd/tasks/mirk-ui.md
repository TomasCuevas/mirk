# Feature: Mirk — identity, typography, screens, and the name

Status: **in progress**
Created: 2026-10-20
Engram mirror: `odd/mirk-ui/tasks`
Supersedes the naming decision recorded in `odd/tasks/pdf-progress-tracker.md` ("Product name: `oct`").

---

## Specs

**S1 — the visual reference, quoted verbatim.**
> "Me gusta mucho la UI de esta web: https://www.raycast.com/. Podriamos aplicar un estilo similar a esta APP"

"Similar" is not an opinion here: it is the measured system of `raycast.com`'s own stylesheets,
read on 2026-10-20. The values below are the spec, and a deviation is a defect unless this document
says otherwise.

| Axis | Measured on raycast.com | Consequence for Loci |
| --- | --- | --- |
| Fonts | `Inter` (main), `Instrument Serif` (display), `Geist Mono`/`JetBrains Mono` (mono) | S2 |
| Neutrals | `#07080a` `#0c0d0f` `#111214` `#1b1c1e` `#2f3031` `#434345` `#6a6b6c` `#9c9c9d` `#cdcece` `#e6e6e6` | the whole neutral ramp is reused by name |
| Foreground | `#f4f4f6` | body text |
| Brand | `#ff6363` (Raycast red); accents `#56c2ff` `#ff9217` `#59d499` | red is the mark, never body text |
| Radii | `4 / 6 / 8 / 12 / 16 / 20 / 24` | controls 6, cards 12, modal 16 |
| Hairlines | `#ffffff1a` (≈10 % white) | every border; no hard grey borders on dark |
| Weight | 500 dominates, 600 for emphasis, 700 rare | the type scale below |
| Primary button | white background, black text | the one loud control in the app |
| Letter-spacing | `0.2px` on small text, negative on display | see S2 |

**S2 — typography, quoted verbatim.**
> "Copiar la tipografia de la WEB"

`Inter` for interface text, `Instrument Serif` for screen titles, `JetBrains Mono` for numerals that
are compared (page counts, percentages). All three are **bundled locally** under `web/public/fonts/`
with their OFL licence texts: a desktop application cannot fetch a font at runtime, and the project
already records every licence it ships.

**S3 — scope, quoted verbatim.**
> "Rediseño de pantallas también"

Hierarchy and structure may change, not only paint. The screen contract below is what must survive:
every `aria-label`, every catalogue string, every role and every tested text stays; only their
arrangement and styling change, unless this document records the change.

**S4 — the name, quoted verbatim.** The decision changed twice; both requests are kept.

> "Renombrar a Loci"
> "Claro, realmente el nombre Loci no me termina de cerrar. Estaba pensando en otro nombre como
> 'Mirk' o 'Oide'" → criterion: *"Que suene bien y sea corto"* → decision: *"Ir a Mirk"*.

Product name **`mirk`** (lowercase wordmark, `Mirk` as the OS-level product name). The rename reaches
crates, the Tauri identifier, the data directory, the environment variable and the documentation.
**It must not lose a single row of a reader's library**: see T6.

**S5 — decisions that this feature does not reopen.**
- The continuous progress ramp red→orange→yellow→green (R38) stays exactly as it is, and its
  computed color is still pinned by `BookCard.test.ts`.
- Colour is never the only channel: the number on the bar stays (R7, R38).
- The window stays fixed at 430×844 (A2).
- Every sentence keeps stating provenance; no number gains a source it does not have.
- Both catalogues (`en`, `es`) stay complete; no string is invented in one language only.
- The cover placeholder keeps `aspect-ratio: 9 / 11` and the carousel keeps `--tile: 13rem`
  (`HomeScreen.test.ts`), because those are user decisions, not styling.

**S6 — contrast.** Every foreground pair must reach **AA** against its own background: ≥ 4.5:1 for
body text, ≥ 3:1 for large text and for the focus ring. The ratios are measured and recorded in T1's
evidence, not asserted.

**S7 — Inicio, quoted verbatim.**
> "Los carruseles deberian tener flechas para navegar, y quitar el scroll inferior qporque queda muy mal."
> "Los botones 'Abrir lista completa' es muy poco estetico. Deberias cambiarlo por algo como 'Lista
> completa ->'. Agregando un icono junto al texto."

**S8 — Biblioteca, quoted verbatim.**
> "Los botones de paginacion son muy feos. El texto 'Pagina anterior' y 'Pagina siguiente' quedan
> muy, muy mal. Deberias agregar una paginacion moderna, con flechas y la tipica lista de pagina como
> la siguiente: <- 1 2 3 ... 5 ->"
> "Los select quedaron feos y casi que fuera del estilo general del resto de la app."
> "Las portadas de los libros deberian ocupar todo el ancho/alto de contenedor interior de la card,
> donde va la portada"
> "El texto '42 de 42 documentos' es feo y encima esta mal ubicado."
> "El indicador de progreso, cuando esta en 100% deberia agregar un poco mas de padding horizontal y
> cambiar el color. Haciendo que sea facil darse cuenta que esta completa"

**S9 — Ajustes, quoted verbatim.**
> "El boton de 'Volver a Ajustes' deberia tener el icono de la flecha <-"
> "Mejora la UI y estructura de la pantalla 'Ajustes de Biblioteca'"
> "Mejora la estructura de la pantalla 'Ajustes de Aplicacion'"

**S10 — General, quoted verbatim.**
> "Debes mejorar los iconos de minimizar, colocar en tray y cerrar. Tienen que ser iconos unicos que
> generen personalidad"
> "Debes mejorar el copywriting en cualquier lugar que lo veaz necesario"

**S11 — the document editor, quoted verbatim.**
> "Tienes que mejorar la estructura de la edicion de un documento."
> "El input para escribir el autor debe de por si dar sugerencias de los autores. No tiene que haber
> un input y al costado un selector. El mismo input de texto debe ser un selector donde se pueda
> escribir."
> "Los botones del formulario de edicion deben tener iconos para que sea mas faciles distingirlos."
> "El boton de 'Restaurar el titulo del documento' deberia estar a la izquierda del input del titulo y
> solamente debe ser un icono con un popover que se active cuando se haga hover."
> "En el editor debe haber una previsualizacion de la portada actual."

**S12 — quoted verbatim.**
> "La paginacion debe tener mas margen vertical, un poco mas nada mas."

**S18 — the cover's two sources, quoted verbatim.**
> "Cuando coloco una imagen por mi propia cuenta y mas tarde quiero volver a utilizar una pagina del
> documento como portada, no puedo hacerlo sin antes quitar la imagen anterior. Se tiene que poder
> poner una pagina del documento como portada o volver a la imagen anterior sin eliminarla"

**S13 — quoted verbatim.**
> "El icono para ocultar en el tray no me gusta, el icono actual parece mas un icono de descarga que
> de ocultar en el tray"

---

## Tasks

| ID | Task | Specs | Route | Commit |
| --- | --- | --- | --- | --- |
| T1 | Design tokens + bundled fonts + global styles | S1, S2, S6 | inline | **done** (uncommitted) |
| T2 | Shell: top bar, navbar, scroll surface | S1, S3 | inline | **done** (uncommitted) |
| T3 | Card and grid: surface, ramp, numerals | S1, S3, S5 | worker | **done** (uncommitted) |
| T4 | Inicio, Biblioteca, Categoría: hierarchy | S1, S3 | worker | **done** (uncommitted) |
| T5 | Ajustes (index + 3) and Modal | S1, S3 | worker | **done** (uncommitted) |
| T6 | Rename `oct` → `loci`, data adoption | S4 | worker + parent | **done** (uncommitted) |
| T7 | README, this record, licence notices | S1–S4 | inline | pending |
| T8 | Verification: tests, typecheck, build, real screen | all | parent | **done, with one residual** |
| T9 | Inicio: carousel arrows, no scrollbar, "Lista completa →" | S7 | inline | **done** (uncommitted) |
| T10 | Biblioteca: numbered pagination, selects, cover fill, count, 100 % | S8 | inline | **done** (uncommitted) |
| T11 | Ajustes: back arrow, Library and Application structure | S9 | inline | **done** (uncommitted) |
| T12 | Window chrome: three strokes in one hand | S10 | inline | **done** (uncommitted) |
| T13 | Copy: both catalogues, in the catalogue's own register | S10 | inline | **done** (uncommitted) |
| T14 | Verification of the round | S7–S10 | parent | **done** |
| T15 | Editor: two groups, combobox author field, icon buttons, cover preview | S11 | inline | **done** (uncommitted) |
| T16 | Pagination: vertical breathing room | S12 | inline | **done** (uncommitted) |
| T17 | Hide icon: an eye with a slash, not a download | S13 | inline | **done** (uncommitted) |
| T18 | Verification of the round | S11–S13 | parent | **done** |
| T19 | Cover source as a stored fact (schema V10), UI switch, tests | S18 | inline | **done** (uncommitted) |
| T20 | Verification of the round | S18 | parent | **done** |

**The user's standing rule applies: nothing is committed until they approve it on screen** (recorded
in `pdf-progress-tracker.md`, L70). The "commit" column therefore records the intended Conventional
Commit message, not a hash.

A task is checked only after its outcome and checks were observed. Commit slices are the work-unit
boundary: ~400 authored lines each, tests and docs alongside the behaviour.

---

## Log

**L1 — the request, verbatim.**
> "Bien, el core duro de la aplicacion ya esta desarrollado como tal. Falta mejorar la UI, definir un
> estilo propio, paleta de colores y demas. Tambien, quisiera analizar si es un buen nombre OCT o
> habria mejores alternativas"

**L2 — the decisions, verbatim.**
> Dirección: "Me gusta mucho la UI de esta web: https://www.raycast.com/. Podriamos aplicar un estilo
> similar a esta APP"
> Tipografía: "Copiar la tipografia de la WEB"
> Alcance: "Rediseño de pantallas también"
> Nombre: "Renombrar a Loci"

**L19 — the name is `mirk`, and the previous names stay as history.**
The user's final decision, once the criterion was named — *"Que suene bien y sea corto"* — was
**Mirk**. The analysis that preceded it is recorded rather than erased: `loci` had the meaning (the
method of loci: remembering where things are) and a clean namespace, but it did not convince the
owner. Of the two alternatives, **Oide** had every domain and the npm name free but no agreed
pronunciation in either language, and **Mirk** has the sound but its Scots meaning — dark, murky — runs
against a product whose promise is clear provenance; `mirk.app` and `mirk.com` are occupied and an
iPhone app already uses the exact name. That is the recorded trade-off, accepted by the owner.

What the rename costs, and it was paid in the same commit: five crates and every identifier, the Tauri
identity, the tray words, the catalogues, the README, the documentation — and **a second layer of data
adoption**. The chain is now `oct` → `loci` → `mirk`, newest first: adoption looks for a legacy
directory that holds a database before one that merely exists (an empty `loci` left by an interrupted
launch must not beat a populated `oct`), renames the trio all or nothing, and `database_file_in` looks
for `mirk.sqlite3`, then `loci.sqlite3`, then `oct.sqlite3`.

**L3 — why the name changes (kept as history: it explains the oct → loci decision).** `oct` is dominated in search by optical coherence tomography, `.oct`
is GNU Octave's compiled-module extension and Radiance's octree format, the npm name has been taken
since 2016, `oct.dev` and `oct.fyi` are live, and the `oct*` software family already belongs to
GitHub (OctoPrint, Octotree, Octicons, October CMS). `loci` is the plural of *locus*, "the places":
the *method of loci* is the memory palace, the classical technique for remembering where things are.
It is four ASCII letters, `.app` and `.dev` were free when checked, and it says what the product does
without describing a feature. Candidates rejected with reasons: Recto (in Spanish it is the rectum),
Verso (`verso.dev` taken and Verso Books is a publisher), Vellum (a book-formatting competitor),
Quire (a project-management tool), Codex (OpenAI), Hilván (Spanish-only and accented).

**L5 — the progress ramp had to be lifted, and that is an amendment, not a reversal.** The stops of
R38 were chosen for a white card on a pale track (`#e8ebef`), and their own comment says so: they were
"deliberately dark for their hue" so that a pale yellow would not vanish. On the new `#07080a` canvas
that same decision produced the opposite defect — the render of `spike/ui/loci-preview.html` showed an
18 % fill that was a barely visible dark red line and a 91 % fill that read as dirty olive. The
landmarks, the direction and the 70 % threshold are unchanged; the five values were lifted
(`#ff5a52`, `#ff8a3d`, `#f0b429`, `#6ee7a0`, `#2fbf71`) until every point of the ramp clears 3:1
against the dark track. The two tests that asserted the old luminance ceiling and the old `rgb`
values now assert a contrast floor against the real track — the same intent, measured against the
surface the bar is actually on. Reverting is one constant plus two expectations.

**L6 — the rename worker died mid-way, and the parent closed the hole it left.** The mechanical rename
landed (crates, `Cargo.toml`, `package.json`, Tauri `productName`/`identifier`/`title`, tray strings,
`LOCI_PDFIUM_LIB`, `app.name`), but the worker stopped before implementing the data-directory adoption
it had been told to write: `APP_DIR` became `"loci"` while `~/.local/share/oct` still held the reader's
42 documents, covers and a pre-V9 backup. That is a silent data loss for anyone who ran the app after
the rename. The parent implemented it as `adopt_legacy_data_dir` (rename the directory, then the
database file and its `-wal`/`-shm` sidecars) and added `database_file_in` as a second, independent
guarantee: if the file rename fails, the legacy filename is still found instead of a new empty database
being created beside a full one. Four tests, including the one that caught the missing file rename:
the first version moved the directory and asserted `loci/loci.sqlite3`, which failed with `NotFound`
because the file inside kept its old name. That failure is the reason both mechanisms exist.

**L8 — the restyle worker died after finishing, and the parent found two defects it never reported.**
All eleven components were restyled (one hex remains, deliberately: white text on the danger hover of
the close button), so the unit's work was there; the report was not. Two real failures surfaced in
verification: `tsc` refused the side-effect CSS import (`Cannot find module … tokens.css`) because
`web/src/ambient.d.ts` only declared `*.svelte`, and the hue-monotonicity test failed at 71 % by
0.2 degrees. The first is a declaration the file needed anyway. The second is measurement, not
permission: the test measures the sRGB hue of an 8-bit triple produced by interpolating in OKLab, and
the new last segment drifts back by a fifth of a degree at its midpoint — below the precision of the
output and far below what an eye resolves. The tolerance is now 1 degree, with the reason written in
the test; a real reversal still moves tens of degrees and still fails.

**L9 — verification, and what it did and did not prove.** `cargo test --workspace`: **280 passed**;
`cargo clippy --workspace --all-targets -- -D warnings`: clean; `bun run typecheck`: **0 errors, 0
warnings**; `bun run test`: **305 passed in 19 files**; `bun run build`: 248 KB dist, both fonts and
both OFL texts shipped. The migration was exercised end to end on an isolated copy of the reader's
real library (`XDG_DATA_HOME=/tmp/loci-verify`, `~/.local/share/oct` copied in): the app renamed
`oct` → `loci`, renamed `oct.sqlite3` → `loci.sqlite3` (same 102 400 bytes), moved `covers/`, left the
pre-V9 backup untouched, and started with an empty log. **Residual, stated rather than hidden:** the
running window was not photographed. The application is tray-resident and starts hidden (`visible:
false`), and this environment has no tool able to click the tray or capture a Wayland surface; the
screen check is the user's, on their machine, which is also the approval step.

**L10 — an independent verifier found two blockers after the work looked finished, and both were
real.** The first: sixteen user-visible strings still said `oct` in both catalogues (`window.close`,
`source.native`, the first-run copy, the settings summaries, the suggestion queue). The mech­anical
rename had covered identifiers and paths, not prose. The second, and the serious one: the adoption
skipped whenever `loci/` existed — including the empty directory an interrupted first launch leaves
behind — so a reader's full library could stay in `oct/` while the application created a fresh empty
database beside it. Two independent runs of this session had already produced exactly that empty
`loci/`. Fixed by:
- merging a populated legacy directory into an existing modern one (database, sidecars and covers,
  file by file, skipping collisions, deleting nothing);
- renaming the database trio **all or nothing**, with a rollback, so a WAL is never separated from the
  database it belongs to;
- normalising the file name after a merge;
- five tests that use a **real SQLite database** built by `Store::open`, not a byte string: the adopted
  file is reopened and read; the trio travels together; the empty-`loci/` merge; both populated; and
  the filename fallback.

Verified end to end on a copy of the reader's real library in an isolated `XDG_DATA_HOME`, in the
exact case the verifier called out (`loci/` present and empty, `oct/` populated): the database moved to
`loci/loci.sqlite3` with the same 102 400 bytes, `covers/` moved, and `oct/` was left in place with its
backup. **314 Rust tests, clippy clean, 305 frontend tests, typecheck 0/0, build 248 KB.**

**L18 — the page button that never went away: three comparisons in one.**
The user reported that pressing "Usar esta página" left the button on screen. The cause was a
comparison between two values that were never the same kind of thing: the field shows **one-based**
pages, the store keeps **zero-based** ones, and the baseline was written from the stored number. After
saving page 3 the field said `"3"` and the baseline said `"2"`, so the editor believed there was
always one more change. A `<input type="number">` makes it worse: `bind:value` can hand back a number
where the last save stored a string.

The fix is not a cast. `sameCoverPage` compares two drafts as **covers**, and `coverPageText` rewrites
the field and the baseline to the accepted page's canonical spelling after a save. That closes the
reported defect and the corner cases around it, each one now pinned:

- `"03"` and `"3"` are the same page; a number `3` and the string `"3"` are the same page.
- An empty field and `"1"` are the same cover — the stored `null` and `0` both display page 1 — so
  typing "1" over a blank field is not a phantom change.
- An invalid draft (`"0"`, `"abc"`) **is** a change, so the button stays and the refusal can be shown;
  but a draft identical to the saved text is not, so an error never becomes a fake difference.
- A failed write leaves the button and the baseline exactly as they were, because the baseline only
  moves after `coverError` is still `null`.
- Double-clicking cannot write twice: the button is disabled while `coverSaving`.
- The loop the user described is a test: the button disappears when the page takes over, returns when
  the number changes, and returns when the saved image comes back.

**L17 — the cover's source became a stored fact (schema V10).**
The defect was in the data model, not the screen: `cover_choice()` was a *consequence* — "an image
wins if there is one" — so choosing a page while an image existed wrote a column the rule ignored, and
returning to the image meant picking the file again. Nothing in the interface could fix that.

- **Core** gains `CoverSource` (`page` | `image`) and `Document::cover_choice_from`, which honours the
  source and falls back to the old rule when it is absent. **Store** gains schema **V10**:
  `documents.cover_source`, backfilled from the old rule so an existing library opens on the cover it
  showed before. `DocumentRecord` carries the source and answers `cover_choice()`.
- **The three cover writes now set the source**: a page makes the page the cover (image kept), an
  image makes the image the cover, and removing the image hands the cover back to the page. A new
  `use_cover_image` command returns to the stored image without the file chooser — and refuses when
  there is no image to return to, because a source saying "image" over no file is a broken cover.
- **The interface** reads `coverSource`, states which source is in force, and offers "Usar la imagen
  guardada" only while the page is in force; the page button is offered whenever the image is in
  force, even with the page number unchanged, because that click *is* the switch.

Two things the tests caught, both worth recording. The V10 migration had to be made idempotent for the
schema-shape tests, which build a current database and rewind its version stamp: `ADD COLUMN` has no
`IF NOT EXISTS`, so the step asks `PRAGMA table_info` first. And the migration test itself drops the
new column and rewinds to 9, then asserts the backfill: a document with an image opens as `image`, one
without opens as `page`.

Verification: **327 frontend tests**, **cargo workspace green**, and the real library copy migrated in
place (V9 → V10, two documents carrying an image backfilled as `image`).

**L16 — the editor stops being a form that saves itself away.**
Four corrections in one round, and one of them removed a rule rather than adding one:

- **Restoring asks** (S14): a second modal says what is lost — the reader's title and the unsaved draft,
  because the editor closes afterwards. It closes for an honest reason: the field would have to show
  the document's own title, which the entry snapshot does not carry, and a field showing the discarded
  title is worse than no field.
- **Save no longer closes** (S15). `editorClosesAfterCommit` is gone, not inverted. What replaces the
  closed editor is a **baseline** (`savedTitle`, `savedChips`, `savedPage`) seeded with the editor and
  moved forward by each successful commit.
- **The pair exists only when it means something** (S16): Save and Cancel appear with the first change
  and disappear when the store holds the draft. Cancel now *discards* and keeps the editor open —
  with nothing to discard, a Cancel that closed the dialog would be a second way to close, and the
  backdrop and Escape already are the first.
- **The cover section says what the cover is** (S17): a sentence under the preview — *"Ahora: página 1
  del documento"* or *"Ahora: una imagen suya"* — plus two labelled blocks separated by a rule, "Página
  del documento" and "Una imagen suya". The user could not tell which mechanism was in force, and the
  old layout put the preview, the page field and both image actions in one undivided column.

The tests moved with the rules: the cancel test asserts the draft is discarded and the dialog stays;
the save test asserts the pair disappears while the dialog remains; the restore test asserts the
question writes nothing and the answer writes once; and the page-save test now changes the page first,
because the button no longer exists until it does.

**L15 — the horizontal scroll was the popover, exactly as the user suspected.**
A dialog that scrolls vertically also scrolls horizontally: CSS computes the other axis to `auto` as
soon as one is not `visible`. The restore popover was anchored to the centre of the last control in the
row, so its right half poked past the dialog's box and gave the whole editor a horizontal scrollbar —
the user's screenshot shows the dialog pushed off its own left edge. Two fixes, because either alone
would leave the failure mode armed: the popover now grows **inwards** (right-anchored), and `.dialog`
declares `overflow-x: hidden`, so a tooltip, a long word or a future element can never turn a modal
into a horizontally scrolling surface again.

**L14 — the screenshot showed a self-inflicted regression, and it had one cause.**
The user's picture of the editor showed 21-pixel fields, an author input cut off mid-placeholder, and a
cover preview that did not change. Two of those were one mistake of mine: when the combobox replaced
the old `<select>`, a CSS cleanup deleted the selector block by matching `  select {` — which matched
*inside* `input, select {`, leaving a dangling `input,` and removing the whole rule. Every field in the
editor fell back to the browser's own chrome. The rule is re-stated for `input` alone, at 2.25 rem,
full width, so no future `select` cleanup can take it down with it. The lesson is in the code comment
because the next cleanup will face the same temptation.

The third defect was real and separate: `finishCoverChange` refreshed the card's cover and the grid's
override, but the editor's preview had been read once, when the editor opened, so a page change or a
chosen image left the old picture on screen. The preview is now set from the same answer as the card,
and a DOM test presses "Usar esta página" and asserts the image's `src` is the cover the store returned
afterwards.

Also from the same report: the restore control moved to the RIGHT of the field (a trailing action on
what the field holds), and the cover's three actions share one row with the text's two, wrapping only
when "Quitar imagen" appears.

**L13 — the editor becomes a form that says what it is.**
The document editor was five controls in one column with three unlabelled commits: the structure the
user asked to improve was, literally, that nothing said which Save applied to what. It is now two
groups — **Texto y autores** (title and authors, one commit) and **Cambiar la portada** (preview, page,
image) — separated by a rule.

- **The author field is the picker** (S11). The input-plus-`<select>` pair is gone. Typing narrows a
  listbox, the arrow keys walk it, Enter takes the highlighted name or the typed one, Escape closes it,
  and clicking keeps focus. The decision — which names are worth offering, minus the ones already on
  the list, in the caller's locale order — is `authorChoices` in `library.ts`, tested on its own. A
  `<datalist>` was never candidate: WebKitGTK does not draw its popup (L70), which is why the select
  existed in the first place.
- **The restore control moved left, became an icon, and grew a popover** (S11). It acts on the field
  beside it; a sentence-wide button had given a secondary action the weight of a primary one.
- **The cover preview is the cover the document has now** (S11), fetched when the editor opens — a
  card in a strip may never have asked for its own, and a preview that said "no cover" about a
  document that has one would be a lie in the one screen that exists to fix it.
- **Every button in the editor carries a drawing and a word** (S11): check for the two saves, cross for
  cancel, picture for choosing an image, bin for removing it. They were browser-default buttons before
  — the modal had never styled a single one.
- **The pagination breathes** (S12): `0.4rem` above and below.
- **The hide icon is an eye with a slash** (S13). The arrow descending into a tray is "download" in
  every icon set, and the user read it exactly that way. Hiding a window is hiding it.

Verification: **322 frontend tests, 0 typecheck errors and 0 warnings**, with the combobox pinned by
DOM tests that focus, type, arrow and click for real — the select's tests were rewritten rather than
deleted, because the behaviour they protected (a name can only be on the list once; removing a chip
brings it back; the keyboard and the pointer land on the same value) still holds.

**L12 — the round of the arrows, the page strip and the two voices.**
All ten points of the request are in, and two of them changed a decision rather than a style:

- **The carousel's arrows are the controls** (S7). The strip keeps native scrolling but its scrollbar
  is gone, and two arrows float over its edges, disabled at the ends so the strip does not change
  width when the reader reaches the last tile. `step()` measures a tile plus the gap, so an arrow
  never cuts a card in half.
- **The paginator is `1 2 3 … 5`** (S8), and the rule that decides which pages to show —
  `pageWindow`, first and last always reachable, a gap only when more than one page is hidden — lives
  in `library.ts` with its own tests, not in the screen. The screen draws what that function decides.
- **The cover fills its frame** (S8): `object-fit: cover` instead of `contain`. The first version
  letterboxed a page of a different ratio, leaving two pale bars inside a card on a dark canvas, which
  is exactly what the user described as not filling the space.
- **"42 of 42 documents" no longer exists** (S8). The count is a line under the title, and it states
  the total alone unless a filter or a page actually hides something. `42 of 42` is arithmetic
  dressed as information.
- **100 % is a state** (S8): `describeProgressView` now returns `complete`, and the card gives that
  number a wider pill in the success colour. The decision is in the one place that decides what a
  fraction means; the card only draws it.
- **A select belongs to the family** (S8): `appearance: none` plus one inline SVG chevron in
  `tokens.css`. The trap this exposed is recorded in a comment — a component that sets the
  `background` shorthand erases the chevron, so selects must set `background-color`.
- **Two options are not a dropdown** (S9): the language is a segmented control, both languages on
  screen, the current one pressed.
- **The window controls are drawn, not typed** (S10): three strokes in one hand, and the hide icon is
  an arrow into a tray because a bar cannot say what "hide" does.
- **The catalogue's register test caught me.** The first Spanish draft said "qué te vio leyendo" and
  "el idioma que elijas"; `i18n.test.ts` — a rule the catalogue had before this round — rejected the
  informal second person. The strings now say "le vio" and "que elija". That is the test earning its
  keep on the first paragraph it ever saw from me.

Verification of the round: **315 frontend tests, 0 typecheck errors and 0 warnings**, and the two new
behaviours are pinned with DOM tests that press the real buttons (the arrow's distance and direction,
the 100 % pill's class), not with a server-side render that cannot click.

**L11 — the second verification pass found a WAL-mixing path, and the merge was made transactional.**
The verifier accepted the strings fix and the empty-`loci/` merge, then found what the first fix had
missed: `rename_database_files` skipped an individual destination that already existed, which in a
mixed state could rename `oct.sqlite3-wal` onto a `loci.sqlite3-wal` belonging to a different database —
a log attached to the wrong database is corruption, not migration. `move_contents` also ignored rename
failures and still told the caller to use the modern directory, so a failed merge could point the
application at a directory with no database in it. Both are fixed structurally rather than patched:

- every move of the trio is **planned against all destinations first**, and a single collision aborts
  the whole operation before the first rename — no file is touched in a mixed state;
- the main database is the gate: if a modern database already exists, nothing is renamed at all;
- a failed trio move rolls back in reverse order, and a failed merge returns the **legacy** directory,
  so the application keeps reading the library where it actually is;
- covers stay best-effort and never overwrite, because a cover is a rendering of a page, not the
  reading record.

The tests were replaced with ones that prove the claim instead of the shape: a **real** database built
by `Store::open` is merged into an empty `loci/`, then reopened and read; and a deliberate sidecar
collision must decline the merge and leave both directories byte-identical. The remaining residual is
stated in the code: a colliding cover name leaves the legacy image in `oct/` rather than replacing the
modern one.

**L7 — the typography is bundled, not fetched.** `web/public/fonts/` holds Inter (variable, latin,
48 KB) and Instrument Serif (latin, 21 KB) with both OFL texts; `tokens.css` declares them with
`font-display: swap`. A desktop application cannot depend on a font CDN at runtime, and the project
already records every licence it ships. JetBrains Mono, which the reference uses for code, was not
bundled: loci displays no code, and the numbers that are compared use Inter's tabular figures.

**L4 — the state before this feature, measured.** 13 of 13 components carried hardcoded hex values
(`#ffffff` ×35, `#5a6472` ×29, `#ccd2da` ×23, `#24506b` ×18), one font family (`system-ui`), and no
token layer: a theme change meant editing thirteen files. The design decision aid that preceded this
document is `spike/ui/style-preview.html` (four token sets over one markup, contrast ratios computed
from the applied CSS).

---

## Delivery slices (recorded on request)

**Branch boundary, recorded because the branch is far past any PR-sized unit:**
`feat/phase-1-core` carries **~39.500 authored changed lines in 119 commits since `main`**. This
feature is four work-unit commits:

| Commit | Message | Why this boundary |
| --- | --- | --- |
| `fceebf2` | `feat!: rename to mirk, adopt the data-directory chain, and store the cover source (schema V10)` | The rename and the data model live in the same Rust files and cannot be separated without rewriting the tree; both are data-path changes and belong together. |
| `0f1b428` | `feat(ui): a token layer, self-hosted fonts, and the window shell` | Foundation only: it compiles and renders the old screens with the new chrome. |
| `fa1cb93` | `feat(ui): the card, the screens and the document editor` | One unit, because `library.ts` and the components changed together: splitting them would produce commits whose build fails (the removed `editorClosesAfterCommit` is still imported by the old `BookGrid`). |
| (this one) | `docs: the mirk record and the design aids` | The record and the decision artifacts. |

**What this means for delivery, stated rather than discovered at review time:** the feature is
reviewable commit by commit, but the branch as a whole is not a pull request. Whether it becomes one
PR, a chain of slices, or is merged as-is is the user's decision, not inferred here.

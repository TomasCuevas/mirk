# Feature: the window layout is chosen — 2, 3, 4 per row, or full screen

Status: **in progress** — implementation and runtime proof complete; the commits were undone at the
user's request (L6) and wait for approval. Delivery (push/PR/merge) is the user's decision.
Created: 2026-10-21
Engram mirror: `odd/window-layout/tasks`

---

## Specs

**S1 — the request, quoted verbatim.**
> "Actualmente en esta APP, la ventana es fija y no se puede cambiar ni ajustar el tamaño. Lo que
> quiero es que el usuario pueda elegir si ajustar el tamaño a 2 elementos por fila (el actual
> predefinido), a 3 elementos por fila, a 4 elementos o simplemente seleccionar pantalla completa y
> que se ajusten los elementos por fila que entren allí"

**S2 — exactly four layouts, and the sizes that make them true.**

| Layout key | Documents per row | Window size |
| --- | --- | --- |
| `columns-2` (default, shipped window) | 2 | 430 × 844 |
| `columns-3` | 3 | 634 × 844 |
| `columns-4` | 4 | 838 × 844 |
| `auto` | as many as fit | the work area (maximised) |

**The cards keep the size they have with two per row (L7).** The window grows; the cards never shrink.
So the card's width is the one the shipped 430 px window already gives it —
`(430 − 32 padding − 9.6 gap) / 2 = 194.2` — and a row of N cards needs `N × 194.2 + (N − 1) × 9.6`
of content. Adding the padding gives 430 / 633.8 / 837.6, rounded up to 430 / 634 / 838. Height stays
844 for the three windowed layouts: the request changes the width the row needs, not the shape of the
window.

`auto` is the work area, not native full screen: this host's stack (Wayland/GNOME with WebKitGTK)
ignores `set_fullscreen(true)` — it returns `Ok` and `is_fullscreen()` reports `true` while the window
keeps its size, measured with `maximizable` both false and true — while `maximize()` does fill the work
area. The choice is therefore carried by the compositor's own maximise, and the desktop's top bar stays
visible, which is the honest consequence of the stack and not a missing feature.

**S3 — the choice is stored and applied at launch.**

The layout lives in the same `settings` table as the language, under the key `window_layout`, because
two halves need it: the desktop process sizes the window before it is shown, and the interface draws
the grid. A value this build does not know is reported as *nothing stored* (the default stands),
exactly as an unknown language is. Writing an unknown value is refused as an invalid argument and
never reaches the store.

**S4 — the grid follows the layout.**

A preset states its column count explicitly (`repeat(3, …)`), so "3 per row" is guaranteed rather than
left to a `minmax` computation that could disagree with the window width. `auto` uses
`repeat(auto-fill, …)` with a floor of the width two columns have in the shipped window (194.2 px, L10),
so full screen shows cards the size the fixed layouts show and as many as fit. The card itself is not
redesigned.

**Scope of the count (L9):** it applies to Biblioteca and to a category's own list — the screens that
draw a grid. Inicio's carousels are strips, not grids: they keep their horizontal scroll and their own
tile width, and the choice does not touch them.

**S5 — the choice lives in Ajustes, as a fourth section.**

Ajustes is an index (R43); the new section `window` is appended last, with its own sub-screen, its own
one-line purpose, and four options where the current one is marked `aria-pressed`. The two catalogues
carry every new string in both languages.

**S6 — what the user did not ask to reopen stays.**

The window keeps no native decorations and no maximise control; dragging its edge still does not
resize it — the presets are the only way its size changes. The top bar's three controls, the tray and
the language setting behave exactly as before. Nothing about a stored library changes.

**S8 — Biblioteca follows the layout (L11).**

The window's layout decides four things on the listing, because a page that ends halfway through a row
reads as missing documents:

| Layout | Documents per page | Pager | Filters |
| --- | --- | --- | --- |
| `columns-2` | 10 (unchanged) | centred | three stacked rows |
| `columns-3` | 12 | right-aligned | three stacked rows |
| `columns-4` | 12 | right-aligned | three stacked rows |
| `auto` | 14 | right-aligned, larger | search, author and order on one row |

From three per row the grid fills the window, so the strip follows the content and ends at the right
edge; full screen's page controls are drawn larger because they are read from farther away, and its three
filters share a row, which is height given back to the documents.

**S7 — documentation matches the new truth.**

The README's "fixed 430×844" claims, the capture script's fixed viewport, and the module comments that
describe the window as immovable are updated; the capture script accepts a layout so the interface can
be judged at 3 and 4 per row without running the desktop shell.

---

## Tasks

| ID | Task | Specs | Route | Commit |
| --- | --- | --- | --- | --- |
| T1 | The layout model in the interface: keys, grid columns, window sizes | S2, S4 | inline | `7abf0d3` |
| T2 | Reading and writing the stored layout from the interface | S3 | inline | `7abf0d3` |
| T3 | The layout model in Rust: parse, size table, refusal | S2, S3 | inline | `dbcbbe5` |
| T4 | The desktop commands, the stored setting and the launch-time application | S3, S6 | inline | `dbcbbe5` |
| T5 | The grid variable and the shell wiring | S4 | inline | `e39f9a1` |
| T6 | Ajustes → Ventana, with both catalogues | S5 | inline | `e39f9a1` |
| T7 | Documentation and the capture script | S7 | inline | `docs` slice |
| T8 | Verification of the round | S1–S7 | parent | see L3–L5 |
| T9 | The page size, pager alignment and size, and the filters' row | S8 | inline | uncommitted (L11) |

---

## Log

**L1 — the request, verbatim.**
> "Actualmente en esta APP, la ventana es fija y no se puede cambiar ni ajustar el tamaño. Lo que
> quiero es que el usuario pueda elegir si ajustar el tamaño a 2 elementos por fila (el actual
> predefinido), a 3 elementos por fila, a 4 elementos o simplemente seleccionar pantalla completa y
> que se ajusten los elementos por fila que entren allí"

**L2 — decisions taken before writing, and why.**

- The preset changes the window's width *and* the grid's column count together: a window wide enough
  for three cards that still auto-filled to two would answer half the request.
- The control goes in Ajustes, because that is where every other stored choice already lives and the
  index gives it a home without adding a fourth chrome control to the top bar.
- Manual edge-dragging stays off. The user asked for four choices, not for an arbitrary window; S2's
  sizes are what make each choice mean what it says.

**L3 — the suites, and what the captures show.**

`bun run typecheck` → 0 errors, 0 warnings. `bun run test` → 20 files, 351 tests passed.
`cargo test -p mirk-desktop` → 107 passed. `cargo check -p mirk-desktop` → no warnings.

`node scripts/capture-ui.mjs --layout <layout>` ran at all four layouts and exited 0 with "no
exceptions or console errors" each time; the Library captures show 2, 3 and 4 cards per row at
430/460/605 px, and `auto` at 1440 px shows the cards at their 8.5 rem minimum with the empty tracks
`auto-fill` creates. The Ventana sub-screen renders as the four-row choice with the layout in force
raised, and the Ajustes index shows four rows.

**L4 — the live run, and fast refresh.**

`bun run tauri dev` is the running loop: Vite on `:1420`, the desktop binary rebuilt in 7.38 s and
launched, both watching. Fast refresh was proven in the running window, not inferred from the config:
saving a style rule produced `[vite] (client) hmr update /src/App.svelte?svelte&type=style&lang.css`,
and saving a line in the component's script produced `[vite] (client) hmr update /src/App.svelte` —
a component update, with no `page reload`. The probes were reverted; the tree carries only the
feature.

**L5 — the one link no tool here can measure.**

The window's real GTK geometry after `apply_window_layout` is not readable on this host: the app runs
on Wayland, `xwininfo` does not see it, and neither `xdotool` nor `wmctrl` is installed. What the
tests pin is the size table and the store rules; what they cannot pin is that this window manager
accepts a programmatic size change on a window that is not resizable. The resize path lifts
`set_resizable` around its own `set_size` call for exactly that reason. The live application is the
check: Ajustes → Ventana → 3 per row should widen the window to 460 × 844 immediately.

## Delivery slices (planned, not committed)

The four commits made during the first pass were reset at the user's request (L6). When they approve the
result, the same boundaries are the ones to use, in this order:

| Slice | Message | Why this boundary |
| --- | --- | --- |
| 1 | `feat(desktop): size the window from a stored layout` | The Rust half stands alone: the table, the stored setting, the launch-time application and the configuration change are one behaviour, and the frontend does not call it until the next slice. |
| 2 | `feat(window): name the four layouts and the columns each one asks for` | The interface's model and its store bridge compile and pass without the screens. |
| 3 | `feat(ui): choose the window's layout in Ajustes` | The reader-facing half: the section, its screen, both catalogues, and the shell publishing the column count to the grid. Splitting the screen from the wiring would pass a prop before it exists. |
| 4 | `docs: record the window layout and capture it at every size` | Documentation, the capture script and this record. No behaviour changes. |

**L10 — full screen's cards were too small.**
> "Ahora que vi, los documentos en pantalla completa son muy pequeños, el tamaño minimo deberia ser
> mayor, similar al tamaño que poseen cuando es fijo el numero de elementos"

`auto-fill` still used the card's bare 8.5 rem minimum (136 px), which on a 1920-wide screen fit about
twelve tiny columns. Full screen's floor is now the same derivation the window's sizes use —
`(430 − 32 − 9.6) / 2 = 194.2 px` — published by the shell as `--grid-card-min`; the fixed layouts keep
136 px because their count is explicit and the window is already sized for it. `gridCardMinimum` is a
pure rule with a test, the shell publishes it, and the grid reads it. The `auto` capture now draws five
tracks instead of twelve, with the cards at the fixed layouts' size.

**L11 — the listing's page and controls follow the layout.**
> "- A partir de 3 elementos por fila, la paginacion debe estar alineada a la derecha y no en el centro.
> - En pantalla completa, los elementos pertenecientes a la paginacion deben ser mas grandes.
> - Se deben ajustar los elementos de la paginacion. Cuando son 2 por fila esta bien que sean 10
>   elementos, pero al ser 3 por fila debe pasarse a 12 para que no queden espacios. En 4 por fila esta
>   bien 12 y en pantalla completa pasar a 14 por pagina.
> - En pantalla completa, los filtros de busqueda podrian estar todos en la misma fila para ahorrar
>   altura, ya que como puedes ver en la captura, la busqueda ocupa todo el ancho y queda feo"

`libraryPageSize(layout)` is the page size rule (10 / 12 / 12 / 14) and `paginate` already took the size;
the screen now cuts the page with it. The pager gains `right` from three per row and `large` in full
screen, and the controls gain `row` in full screen — the query keeps twice the width of each selector.
Pinned by `window.test.ts` (the sizes) and `LibraryScreen.test.ts` (the four layouts × page size, pager
alignment, pager size and filter row), with the capture at 16 seeded documents showing the right-aligned
strip in 3 per row and the single filter row with larger pages in full screen.

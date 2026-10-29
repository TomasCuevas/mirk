# Feature: the interface reads at every window size

Status: **in progress** — implemented and verified; waiting for the user's approval before committing.

---

## Specs

**S1 — the request, verbatim.**
> "Bien, hay que trabajar en algunos otros detalles de responsive:
> - En 2 documentos por fila, el navbar inferior es muy alto. El icono y el texto deberian estar mas cerca.
> - En pantalla completa tienes que agrandar todos los textos de la aplicacion, ya que quedan muy chicos y en ocasiones, se dificulta la lectura.
> - Es muy probable que tengas que agrandar el ancho minimo de los documentos si el texto es mas grande.
> - Tambien debes hacer mas altos todos los inputs, selects y elementos de formulario.
> - Analiza todos los posible corner cases y atacalos"

**S2 — the bottom bar is tighter, not shorter than a target.**

The bottom bar loses the air the first round added: bar padding 0.5 → 0.3 rem, item padding 0.4 → 0.25 rem, the
icon-to-label gap 0.25 → 0.1 rem, and the drawing 22 → 20 px. The target keeps a 2.9 rem minimum (≈ 46 px at
16 px root), so the bar is roughly 57 px instead of 72 px and the icon and its word read as one item.

**S3 — full screen draws on a larger root.**

The token layer is written in `rem`, so one root value scales type, spacing, control heights and the sidebar
together: `auto` sets the root to **18 px** (from 16, +12.5 %), every windowed layout keeps 16 px. The value is a
pure rule (`interfaceFontSize(layout)`), the shell applies it to `document.documentElement` and restores it on
unmount, so nothing leaks between layouts.

**S4 — the card floor grows with the text.**

The full-screen grid floor is the fixed card width scaled by the root (`194.2 × 18/16 = 218.475 px`), because a
card whose text grew but whose floor did not would wrap more and read smaller than the words inside it.

**S5 — form controls are one taller height.**

Inputs, selects and action buttons are 2.5 rem (≈ 40 px at 16, 45 at 18); the language selector 2.25 rem and the
window-layout choices 2.4 rem. The rule is a single height per kind of control, so the toolbar, the editor and the
settings screens cannot drift apart.

**S6 — corner cases found and attacked.**

| Corner case | What was done |
| --- | --- |
| The scaled root must not leak into windowed layouts | The shell's effect restores the root on unmount and re-runs on every layout change; pinned in `App.dom.test.ts` (16 px windowed, 18 px full screen). |
| The windowed widths were derived at 16 px | Only `auto` scales. 430 / 634 / 838 stay what the desktop process pins. |
| The full-screen card floor has to scale too | `gridCardMinimum` multiplies by `interfaceFontSize / 16`, pinned in `window.test.ts`. |
| A wider root narrows the full-screen grid enough to break "14 per page" | The sidebar is 10.5 rem, which is ≈ 189 px at the 18 px root — its old 200 px — and gives the grid back the column. Verified at 1828 CSS px (a 1920 screen at 1.05 scale) with 16 documents: **7 columns, 14 per page, two full rows**, none left over. |
| Drawings would stay small beside bigger words | The nav, top-bar and cover-pill icons are sized in `rem` instead of px, so they scale with the root. |
| Taller fields inside the single-row filters | The row still fits: the query keeps twice a selector's width, and all three grow by the same 0.5 rem. |
| The right-aligned pager in a wider root | Its large variant is already in rem (2.4 rem → 43 px at 18); alignment is unchanged. |
| The editor dialog at the larger root | Its 36 rem cap becomes 648 px, still narrower than full screen and than the 3-per-row window (≈ 604 CSS px), where `min(36rem, 100%)` keeps it inside. |
| Anything reading `16px` as a constant | The only literals left are in the Rust size table, which describes the *window*, not the interface: the full-screen layout is maximised and does not use them. |

---

## Log

**L1 — the request, verbatim.**
> "Bien, hay que trabajar en algunos otros detalles de responsive:
> - En 2 documentos por fila, el navbar inferior es muy alto. El icono y el texto deberian estar mas cerca.
> - En pantalla completa tienes que agrandar todos los textos de la aplicacion, ya que quedan muy chicos y en ocasiones, se dificulta la lectura.
> - Es muy probable que tengas que agrandar el ancho minimo de los documentos si el texto es mas grande.
> - Tambien debes hacer mas altos todos los inputs, selects y elementos de formulario.
> - Analiza todos los posible corner cases y atacalos"

**L2 — decisions.**

- One root scale instead of per-component rules: the token layer already speaks `rem`, so the alternative would
  be a dozen overrides that could disagree.
- `18 px` is a visible step (12.5 %) without turning the interface into a different product; the reader can ask
  for 19 or 20 as a one-line change.
- The sidebar gives back the width the larger root took from it, instead of shrinking the page size the user
  chose, because "no holes in the last row" was the reason that page size exists.

**L3 — evidence.**

`bun run typecheck`: 0 errors. `bun run test`: 22 files, 371 tests (new: the root rule and the scaled floor, and
the shell publishing both). Captures: at `columns-2` the bar is shorter and the controls taller; at `auto` the type,
the controls, the sidebar and the icons are visibly larger; at 1828 CSS px with sixteen documents the full-screen
page shows seven columns and fourteen documents in two full rows, with the pager right-aligned and larger. All
captures exit 0 with no page errors.

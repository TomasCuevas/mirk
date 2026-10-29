# Feature: the dialog follows the window, and its width follows its content

Status: **in progress** — implemented and verified; waiting for the user's approval before committing.

---

## Specs

**S1 — the request, verbatim.**
> "Otra cosa que he notado, es que el dialog/modal o como se llame donde esta el formulario del
> documento, es muy pequeño y no se va adaptando a las diferentes dimensiones. Queda muy bien en el
> tamaño de 2 por fila, pero al agrandar la pestaña, queda muy mal. Deberias ajustar un poco hasta
> encontrar una forma estetica para ese formulario en todos los tamaños"

**S2 — the width belongs to the content, not to one number.**

`Modal` gains a `size` of `narrow` (default, 22 rem) or `wide` (up to 46 rem, never wider than the
window). A confirmation — one sentence and two buttons — stays narrow, because a wide box around one
sentence is the same complaint in reverse. The document editor asks for `wide`: it is the dialog with
content to justify the room.

**S3 — the editor is one vertical column at every width.**

The dialog is wider than a confirmation's so the form is not cramped, but its groups never sit side by
side: text and authors first, the cover below, which is the order the form was designed in. The extra
width goes to the fields, not to a second column.

**S4 — what the dialog already did is untouched.**

Focus on open, Escape and backdrop dismissal, the scroll behaviour (vertical only), the body of every
confirmation and the editor's own controls stay exactly as they were.

---

## Log

**L1 — the request, verbatim.**
> "Otra cosa que he notado, es que el dialog/modal o como se llame donde esta el formulario del
> documento, es muy pequeño y no se va adaptando a las diferentes dimensiones. Queda muy bien en el
> tamaño de 2 por fila, pero al agrandar la pestaña, queda muy mal. Deberias ajustar un poco hasta
> encontrar una forma estetica para ese formulario en todos los tamaños"

**L2 — decisions.**

- The width is the caller's choice rather than one global breakpoint, because the two dialogs have
  different content: the editor holds a form, a confirmation holds a sentence.
- `46 rem` is a cap, not a width: the dialog still fits a narrower window, which keeps the 3-per-row
  window (about 604 CSS px) comfortable without shrinking the form.
- The two columns arrive at 700 px of window, where the wide dialog has enough room for two ~330 px
  columns. Below that the form keeps the single column it was designed in.

**L3 — evidence.**

The captures show the two cases: at `columns-2` the editor fills the phone-width window in one column;
at `auto` it is a 46 rem dialog with the text group beside the cover group. `bun run typecheck`: 0
errors. `bun run test`: 22 files, 370 tests, including the new `Modal.test.ts` (narrow by default, wide
when asked) and the editor test that pins the wide dialog and the groups wrapper. Both captures exit 0
with no page errors.

**L4 — the two-column split was rejected.**
> "No me gusta que el formulario se parta en 2. Hazlo todo vertical"

The grid is gone and the wrapper with it; the editor is one column at every window size. The wide cap
drops from 46 rem to 36 rem, because a single column of fields stretched across the wider box would be
the opposite problem. The capture at `auto` shows a 36 rem dialog centred, with the text group above the
cover group and the fields at a readable width.

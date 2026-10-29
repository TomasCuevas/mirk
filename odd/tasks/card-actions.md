# Feature: the cover's actions appear on hover, each with a popover

Status: **in progress** — implemented and verified; waiting for the user's approval before committing.

---

## Specs

**S1 — the request, verbatim.**
> "Otra cosa que quisiera es que el icono para abrir la edicion del documento o abrir la carpeta
> contenedora solamente aparezcan cuando se haga hover sobre el documento. Por defecto, unicamente
> apareceria sobre la portada el numero de paginas. Tambien, estos iconos deberian tener un popover, de
> la misma forma que lo tiene el boton de "Restaurar el titulo del documentos"."

**S2 — the cover states one fact until the card is reached.**

The page-count badge is the cover's default statement. The pencil (document editor) and the folder
(containing folder) fade in when the card is hovered, and — the same rule for a reader without a
pointer — when anything inside the card takes keyboard focus. While hidden they are not click targets
(`pointer-events: none`); a device that cannot hover shows them always, because there is no gesture
that would reveal them.

**S3 — each icon explains itself the way the restore button does.**

A CSS popover, not a library: `data-tip` carries the words, the pseudo-element shows them on hover and
on keyboard focus, opening below the pill and growing away from its corner so it stays inside the card.
The folder reuses `card.openFolder` (already a short sentence); the pencil gets `card.editTip`, because
its accessible name interpolates the document's title and a tooltip is not the place for a title.

Nothing else about the card changes: the accessible names, the corners, the page badge and both actions'
behaviour stay exactly as they were.

---

## Tasks

| ID | Task | Specs | Route | Commit |
| --- | --- | --- | --- | --- |
| T1 | The hover/focus reveal, with the no-hover fallback | S2 | inline | — |
| T2 | The two popovers and the shorter pencil tooltip | S3 | inline | — |
| T3 | Verification: suites and the running window | S1–S3 | parent | — |

---

## Log

**L1 — the request, verbatim.**
> "Otra cosa que quisiera es que el icono para abrir la edicion del documento o abrir la carpeta
> contenedora solamente aparezcan cuando se haga hover sobre el documento. Por defecto, unicamente
> apareceria sobre la portada el numero de paginas. Tambien, estos iconos deberian tener un popover, de
> la misma forma que lo tiene el boton de "Restaurar el titulo del documentos"."

**L2 — decisions.**

- "Hover on the document" is the whole card, not only the cover rectangle: the reader is reaching for
  the card, and requiring the pointer to land on the cover's own pixels would make the actions appear to
  flicker as the pointer crosses the title.
- The tooltip opens BELOW the pill. The restore button's opens above because it sits at the foot of a
  dialog; a cover pill sits at the top of a card, where a tooltip above would leave the card.
- `card.editTip` is a new, short string: `card.editDocument` has the document's title spliced into it,
  which is right for an accessible name and wrong for a 160 px tooltip.

**L3 — evidence.**

`bun run test`: 22 files, 370 tests (the two new card tests pin the two `data-tip` values and the page
count as the cover's default statement). `bun run typecheck`: 0 errors. The reveal itself is CSS, so it was judged in a hover capture: with `hover: hover` emulated and the
pointer on the first card, only that card shows the pencil and the folder, with the pencil's popover
open; the rest of the grid shows the page badge alone. Removing the pointer and re-capturing shows every
card quiet.

**L4 — the no-hover fallback was dropped.**

An `@media (hover: none)` branch made the pills always visible so a device without a pointer could
still reach them. The capture showed it fighting the request on hosts that report no hover, and the
application is a desktop window with a pointer, so the branch is gone: hover reveals, and keyboard focus
reveals too, which is the accessible path that does not depend on a pointer.

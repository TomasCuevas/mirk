# Feature: the navigation earns its size, its icons, and a sidebar in full screen

Status: **done** — implemented, verified on the running application by the user, and committed in `90a801e`.
(L3), as they asked for the previous round.
Created: 2026-10-22
Engram mirror: `odd/navigation/tasks`

---

## Specs

**S1 — the request, verbatim.**
> "- El bottom-nav debe ser mas alto, los botones para navegar tambien. Actualmente son muy pequeños.
> - En pantalla completa, el bottom-nav debe desaparecer y en su lugar, se tiene que colocar un sidebar lateral a la izquierda.
> - Los elementos del bottom-nav deben tener un icono. Icono arriba y texto abajo. En modo siberal, debe estar el icono a la izquierda y luego el texto"

**S2 — the bottom bar is taller, and so are its targets.**

The bar grows from roughly 43 px to roughly 68–72 px: more padding on the bar itself and a target of at
least 3.4 rem per item, which is above the 44 px a pointer target is expected to have. The label keeps
its size; what grows is the room around it, because "very small" was about the targets, not the words.

**S3 — full screen moves the navigation to the left.**

The layout the window is in decides the navigation's place, and it is the same stored layout the window
already uses (`Pantalla completa` is the `auto` layout). In `auto`, the bottom bar disappears and a
left sidebar takes its place, running the full height under the top bar; the screen list keeps that
height. In the three windowed layouts the bottom bar is exactly what it was.

**S4 — every destination has an icon, in the orientation of its bar.**

Three destinations (Inicio, Biblioteca, Ajustes), three icons drawn in the one hand the top bar already
uses (24-unit box, 1.6 stroke, round caps and joins). In the bottom bar the icon sits above the label;
in the sidebar the icon sits to its left. The icons are decorative — the accessible name stays the
label — so nothing is announced twice.

---

## Tasks

| ID | Task | Specs | Route | Commit |
| --- | --- | --- | --- | --- |
| T1 | The rule that maps a window layout to the navigation's mode | S3 | inline | `window.ts` + test |
| T2 | The navbar: icons, both modes, the taller bar | S2, S4 | inline | `Navbar.svelte` + `Navbar.test.ts` |
| T3 | The shell: the sidebar row in full screen | S3 | inline | `App.svelte` + `App.dom.test.ts` |
| T4 | Verification: suites, captures at both modes | S1–S4 | parent | see L3 |

---

## Log

**L1 — the request, verbatim.**
> "- El bottom-nav debe ser mas alto, los botones para navegar tambien. Actualmente son muy pequeños.
> - En pantalla completa, el bottom-nav debe desaparecer y en su lugar, se tiene que colocar un sidebar lateral a la izquierda.
> - Los elementos del bottom-nav deben tener un icono. Icono arriba y texto abajo. En modo siberal, debe estar el icono a la izquierda y luego el texto"

**L2 — decisions taken before writing.**

- "Pantalla completa" is the stored `auto` window layout, so the mode follows the layout state the shell
  already holds. Nothing new is stored and nothing is inferred from the viewport width: a wide windowed
  layout keeps the bottom bar, because the reader chose a window, not a different navigation.
- The sidebar is the same component and the same three destinations; only its orientation changes, so
  the two modes cannot drift apart in words, order or active state.
- The icons live beside the labels they belong to, in the navbar, because they are presentation: the
  screen list stays what it is.

**L3 — the user's correction, and the proof.**

> "En sidebar, los elementos deben estar alineados a la izquierda, no al medio"

The shared button row centres its content, which is what the bottom bar's stacked icon-and-label needs;
the sidebar inherited it and centred a row that has to start at the left edge. `nav.side button` now
sets `justify-content: flex-start` (the `text-align: left` alone never reached a flex row). The capture
at `auto` shows the three items starting at the same left edge, icon then label.

Evidence: `bun run typecheck` 0 errors; `bun run test` 21 files, 359 tests; the captures at
`--layout columns-2` and `--layout auto` exit 0 with no page errors. The bottom bar measures roughly
72 px with a 3.4 rem target per item, the sidebar 12.5 rem wide, and the icons are decorative
(`aria-hidden`) with the label as the only accessible name.

The commits wait for the user's approval, as with the previous round.

**L4 — the icons' stroke was too heavy.**
> "Los iconos del sidebar/navbar son muy gruesos. Tienes que hacer mas pequeño su trazo"

The icons are drawn from a 24-unit box at 22 px, so the top bar's 1.6 stroke rendered heavier than it
does at 16 px. The nav's strokes are now 1.2 — the same box, the same round caps, and a rendered weight
that matches the chrome above. Evidence: the captures at `columns-2` and `auto` show the thinner
strokes in both orientations.

# Feature: the window's own gestures agree with the layout it is in

Status: **in progress** — implemented and green; the two gestures wait for the user's manual check.
Created: 2026-10-29
Engram mirror: `odd/window-drag/tasks`

---

## Specs

**S1 — the request, quoted verbatim.**
> "- Cuando esta en ventana completa y hago clic y arrastro desde la parte superior de la ventana para
> reacomodar la aplicacion, esta se cambia a un tamaño más chico pero sigue manteniendo el estado de
> pantalla completa. Por lo que sigue apareciendo el sidebar lateral y el boton de maximizar sigue
> presionado. Lo que tienes que hacer es que debe cambiarse al tamaño que este seleccionado (por
> defecto 3 por fila si no hay).
> - Tambien, al hacer doble clic en la parte superior, se debe cambiar el estado de la ventana, entre la
> venta completa y las filas que este seleccionada (por defecto 3 por fila)"

**S2 — the two states are one fact, not two.**

A windowed preset and full screen are the same stored choice, and three things must never disagree
about it: the window's real geometry, the selected option in Ajustes → Ventana, and what the interface
draws (the sidebar, the maximise control's pressed state, the grid's column count). Today the drag
gesture of the window manager changes the first one behind everyone's back: the compositor unmaximises
the window to a smaller size while the stored layout still says `auto`, so the interface keeps drawing
full screen. The rule is that the window's shape wins: when the window is no longer maximised and the
stored layout says `auto`, the stored layout becomes the reader's preset — the last one they chose, or
three per row when they never chose one.

**S3 — the last windowed preset is remembered.**

"The last one they chose" must survive the application, so it cannot live in the interface's memory
alone: after a restart, dragging out of full screen has to come back to four per row if that is what
the reader chose yesterday. It is a second stored setting, beside the layout itself, written whenever a
windowed layout is chosen and never by full screen (full screen is not a preset).

**S4 — a double click on the top bar toggles the two states.**

The top bar is the window's handle, so the gesture that means "change the window's shape" belongs
there. A double click on it toggles between full screen and the reader's preset, exactly as the
maximise control does — the control and the gesture must never disagree, so the gesture switches the
stored layout rather than a geometry of its own. The first press of the double click still starts a
drag, as it always has; the second one must not, or the press would be swallowed by the window manager
before the toggle could happen.

**S5 — what does not change.**

Single-press dragging the bar still moves the window. Presses on the four controls still never start a
drag. Closing, hiding and minimising are untouched. Nothing about a stored library changes.

---

## Tasks

| ID | Task | Specs | Route | Commit |
| --- | --- | --- | --- | --- |
| T1 | The stored preset, and reading it | S3 | worker | uncommitted (done) |
| T2 | The rule that reconciles the window's real shape with the stored layout, and the tick that applies it | S2 | worker | uncommitted (done) |
| T3 | The event that tells the interface the layout changed behind it | S2 | worker | uncommitted (done) |
| T4 | The double click on the bar, and its press that must not drag | S4 | worker | uncommitted (done) |
| T5 | Verification: suites, and the two gestures on the running window | S1–S5 | parent + user | manual check pending |

---

## Log

**L1 — the request, verbatim.**
> "- Cuando esta en ventana completa y hago clic y arrastro desde la parte superior de la ventana para
> reacomodar la aplicacion, esta se cambia a un tamaño más chico pero sigue manteniendo el estado de
> pantalla completa. Por lo que sigue apareciendo el sidebar lateral y el boton de maximizar sigue
> presionado. Lo que tienes que hacer es que debe cambiarse al tamaño que este seleccionado (por
> defecto 3 por fila si no hay).
> - Tambien, al hacer doble clic en la parte superior, se debe cambiar el estado de la ventana, entre la
> venta completa y las filas que este seleccionada (por defecto 3 por fila)"

**L2 — decisions.**

- The gesture is answered by polling the window's real maximise state rather than by trusting a window
  event. This host is Wayland with WebKitGTK, where several window events have already proved absent or
  late (`set_fullscreen` is accepted and ignored, `is_minimized` never becomes true). A one-second
  comparison between intent and reality is cheap, needs no assumption about which event fires, and
  self-heals any other way the two could drift.
- The preset is stored, not remembered in the interface: S3 is about "the rows that were selected", and
  a restart must not silently reset it to three.
- The double click is detected on `mousedown`'s `detail`, not on `dblclick`, because the first press
  hands the window to the compositor's drag loop and the second press is exactly what that loop is most
  likely to eat.

**L3 — the interface half, and the gap it exposed.**

The bar now sends a press with `detail >= 2` to a new optional `onToggle` prop instead of the drag, and
the shell answers it with the same function the maximise control uses (`toggleWindowLayout`), so the
control and the gesture cannot disagree. The `window-layout-changed` listener re-reads the stored
layout, remembers a windowed payload as the preset, and ignores an unknown key rather than guessing.
Frontend evidence: `bun run typecheck` 0/0, `bun run test` 24 files / 390 tests (6 new), the new cases
first observed red (the double press called `start_drag` and stored nothing; the event left the grid at
`auto-fill`).

What this half exposed, and T1 must close: `window_preset` is stored by the desktop process but nothing
reads it back to the interface at startup. After a restart, a reader whose stored layout is `auto` and
whose preset is four per row would get four by dragging (the desktop process reads its own setting) and
three by double-clicking (the interface still starts at `RESTORE_WINDOW_LAYOUT`). S3 says the two halves
answer the same question with the same value, so the preset needs a read path to the interface before
this feature is done.

**L4 — the desktop half, the read path, and the one thing nobody could verify.**

The desktop process now owns the preset: `layout::preset` resolves a stored value (`auto`, an unknown
spelling and nothing stored all read as three per row), `main.rs`'s `remember_layout` writes the preset
whenever a windowed layout is chosen and never for full screen, and `start_reconciling` compares the
stored layout with the window's real maximise state once a second. Two consecutive disagreeing ticks
before acting is deliberate: a single one can be the maximise `setup` itself asks for, a layout
mid-apply, or the middle of the drag while the compositor still holds the window under the pointer.
The order inside the tick is store → apply → emit, so the interface reads the layout the window is
about to take even when applying it fails, and every failure path reports and continues rather than
ending the loop.

The interface reads the preset back through `get_window_preset`, which reuses `stored_preset()` and
`layout::preset` instead of re-deriving the fallback, and seeds `lastPreset` at startup. That is what
makes the two gestures answer the same question: after a restart with layout `auto` and preset
`columns-4`, dragging and double-clicking both come back to four per row.

Evidence: `cargo test -p mirk-desktop` 125 passed (114 before this feature; the new ones are the four
combinations of the rule, the preset parser, the two `remember_layout` rules and the exposed read
path), `cargo check` zero warnings, `bun run test` 24 files / 396 tests (6 new for the interface half),
`bun run typecheck` 0/0.

The one thing none of these can answer: what this compositor actually reports through `is_maximized()`
after it unmaximises a dragged window, and whether the first press's `start_drag` on Wayland leaves the
second `mousedown` reaching the DOM at all. Both are the user's manual check (T5). If either signal
misbehaves, the premise to revisit is stated here rather than rediscovered: the loop trusts
`is_maximized()`, and the gesture trusts `detail >= 2` arriving after a drag was already handed to the
compositor.

# Feature: the window chrome works, and the default is three per row

Status: **done** — implemented, verified on the running application by the user, and committed in `8a06af9`.

---

## Specs

**S1 — the request, verbatim.**
> "- Por defecto, el tamaño debe ser 3 elementos por fila.
> - Debes añadir iconos a cada apartado de ajustes.
> - Los botones "minimizar", "tray" y "cerrar" no funcionan actualmente.
> - A los botones anterior nombrados, debes agregar el de maximizar, este boton tambien deberia cambiar el estado seleccionado en opciones.
> - Cuando se maximiza, debe aparecer el boton tambien para volver al estado anterior, osea, establecer las filas que habia."

**S2 — three per row is the window's declared size (superseded by S7 for the layout itself).**

The desktop process creates the 634 × 844 window, because that is the size of the layout "restore" returns to and the frame before `setup` applies the stored choice should be a size a layout really names. Two per row remains available.

**S3 — every Ajustes row has an icon.**

The four sections (Biblioteca, Lectura, Aplicación, Ventana) carry a drawing in the same hand as the navigation's: 24-unit box, 1.2 stroke, round caps, decorative for assistive technology and the row's text as the name.

**S4 — the four window controls act on this desktop.**

`windowControls` gains `maximize` between minimise and hide: **minimise | maximise/restore | hide | close**. Two of the three reported failures are platform facts measured here, and both are handled rather than hidden:

- **Minimise does nothing.** `Window::minimize()` returns `Ok`, `is_minimized()` stays `false` and the window stays on screen, pinned or with its size hints released. The command now checks a moment after the request and, when the window is still not minimised, hides it — the same gesture ("leave the screen", tray brings it back) instead of a button that silently fails. Where the platform does minimise, nothing changes.
- **Hide and close work at the process level**: `hide()` sets the window invisible and `exit(0)` ends the process (both measured). What can look broken is the *way back*: without a tray icon the hidden window has no restore path, and the browser preview disables all three controls because there is no desktop behind it.
- **Maximise** uses the same stored layout as Ajustes: it sets `auto` (Pantalla completa), so the settings row shows that choice selected, and a second press returns to the preset that was in force.

**S5 — "the rows that were there".**

The shell remembers the last windowed layout in `lastPreset` while it runs: choosing 4 per row, then maximising, then pressing restore returns to 4 per row.

**S6 — minimise comes before the tray control (R6).**

The four controls read, left to right: **minimise | hide | maximise/restore | close**. The two window states sit together and open first, the tray control follows, and maximise plus close close the row.

**S7 — full screen is the default, and restore returns to three per row (R7).**

`DEFAULT_WINDOW_LAYOUT` and the desktop process's own default are both `auto`, so a store that has never chosen opens full screen — the maximised work area, as S2 of `window-layout.md` defines it. Because the default is no longer a preset, "restore" cannot fall back to it: `RESTORE_WINDOW_LAYOUT` is `columns-3`, and that is where the window comes back to when the reader never chose a preset. A reader who did choose one still gets their own choice back, exactly as S5 says; the two rules compose instead of competing. The declared launch size stays the restore layout's size (634 × 844), so the frame the window has before it fills the work area is a size the reader can get back to.

---

## Log

**L1 — the request, verbatim.**
> "- Por defecto, el tamaño debe ser 3 elementos por fila.
> - Debes añadir iconos a cada apartado de ajustes.
> - Los botones "minimizar", "tray" y "cerrar" no funcionan actualmente.
> - A los botones anterior nombrados, debes agregar el de maximizar, este boton tambien deberia cambiar el estado seleccionado en opciones.
> - Cuando se maximiza, debe aparecer el boton tambien para volver al estado anterior, osea, establecer las filas que habia."

**L2 — evidence for the controls.**
Measured with the real window shown, through the same window calls the commands make: `minimize()` → `is_minimized=false`, still visible, with equal min/max and with them released; `hide()` → visible false; `show()` → visible true; `maximize()` with hints released → 1920 × 1046, `unmaximize()` → 430 × 844; `app.exit(0)` → process ends. The interface's own handler was pinned separately: clicking the three buttons through a fake desktop invokes `minimize_window`, `hide_window` and `close_app`.

**L3 — evidence.**

`bun run typecheck`: 0 errors. `bun run test`: 23 files, 379 tests. `cargo test -p mirk-desktop`: 107
passed. Captures at `columns-3` and `auto` exit 0 with no page errors: the bar draws four controls (a
square while windowed, stacked panes in full screen), every Ajustes row has its icon, the sidebar is
left-aligned in full screen and the bottom bar is gone there. The interface's click path is pinned in
`App.dom.test.ts` (the three invocations) and the maximise/restore round trip there too; the maximise
control's two states are pinned in `WindowBar.test.ts`.

**R5 — the controls press on the first click, minimise really minimises, and the tray control moves left.**
> "- Los botones de control de ventana siguen sin funcionar del todo bien. En ocasiones funcionan pero
> hay que darle varios clic hasta que hagan algo. Tambien, el boton de minimizar funciona como tray y no
> como minimizar de toda la vida.
> - El boton minimizar en tray deberia estar a la izquierda y el de maximizar a la derecha, osea,
> deberian invertir sus posiciones"

Two defects, one symptom each.

*The presses that sometimes did nothing.* The bar's drag guard asked `event.target instanceof HTMLElement`
before deciding a press was on a control. A press on a control lands on its `<svg>` or a `<path>`, and an
SVG element is an `Element` but not an `HTMLElement`, so the press fell through to `start_dragging`: the
drag took the pointer and the click never completed. Only presses that happened to land on the button's
padding answered. The guard now asks for `Element`, which covers both, and `WindowBar.dom.test.ts` presses
the drawing itself.

*Minimise that behaved like a tray button.* The command minimised, then waited 250 ms and hid the window if
`is_minimized()` still said false. Measured on this host with a temporary probe: `minimize()` returns
`Ok(())`, and 120/300/800 ms later `is_minimized()` is `Ok(false)` while `is_visible()` is `Ok(false)` — the
compositor *did* iconify the window, and tao's Wayland "minimised" flag simply never becomes true. The
fallback therefore hid an already-minimised window, which is why the control left the taskbar. The
fallback is gone; `minimize_window` is the iconify request and nothing else.

*Order.* The controls are now hide → minimise → maximise → close: the tray control on the left, maximise
to the right of the two window states, close last, as asked. The server-rendered order test and the DOM
test that presses maximise by index were updated with it.

**R6 — minimise and the tray control trade places.**
> "Debes invertir el orden del boton \"Ocultar en la bandeja\" con el de \"minimizar\"."

The list is data (`windowControls`), so the reversal is one swap and the bar renders it with no
component change. What moved with it: the server-rendered order expectation, the DOM test that presses
the tray control by index, and the comment in the maximise test that counted the buttons.

**R7 — full screen is the default, and restore lands on three per row.**
> "Debes establecer que el valor de la pestaña por defecto es pantalla completa y al restaurar la
> ventana se coloque con valor de tres columnas."

Two halves, one per process. The desktop process's `DEFAULT_LAYOUT` became `Auto`, so a store with
nothing stored is maximised before the window is first shown; the interface's `DEFAULT_WINDOW_LAYOUT`
became `auto` for the same reason, and both halves now agree about a fresh install.

The restore control read its fallback from the default, which would have restored **to full screen** —
the button would have done nothing. It now reads `RESTORE_WINDOW_LAYOUT` (`columns-3`) in the interface
and `layout::RESTORE_LAYOUT` on the Rust side, where the declared launch size is pinned to it by a test.
A preset the reader chose while windowed still wins over that fallback (S5), pinned separately by a DOM
test that stores `columns-4`, restores and gets four back.

Evidence: `bun run typecheck` 0/0, `bun run test` 24 files / 384 tests, `cargo test -p mirk-desktop` 111
passed (was 107; the two new layout rules plus the watcher's three). The single "three per row is the
default" test became two: the default is `auto`, and the restore layout is `columns-3` and deliberately
not the default.

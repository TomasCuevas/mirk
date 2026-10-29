# Feature: the palette follows the reader's wallpaper

Status: **done** — implemented, verified on the running application by the user, and committed in `36aed0d`.

---

## Specs

**S1 — the request, verbatim.**
> "Por ultimo, quiero que adaptes la paleta de colores de la aplicacion y te guies con los colores de
> esta imagen /home/tomas-cuevas/Pictures/Wallpapers/milad-fakurian.jpg"

**S2 — the sampled colours, and what each one became.**

The wallpaper is a near-black indigo field crossed by violet bands with a hot magenta crest. Sampled
from the file itself: `#160033` / `#170038` (canvas), `#2d1954` / `#362462` (violet bands),
`#523c8e` (lighter violet), `#ff008c` / `#fb0490` (magenta crest).

| Token | Was | Now | From |
| --- | --- | --- | --- |
| `--canvas` | `#07080a` | `#0d0321` | the field's darkest indigo |
| `--surface` … `--surface-3` | neutral greys | `#160a30` → `#2d1b55` | the violet bands, four steps |
| `--line` / `--line-strong` | white 8 / 14 % | lavender-white 9 / 17 % | the bands' own tint |
| `--fg` | `#f4f4f6` | `#f6f2ff` | a violet-tinted white |
| `--fg-muted` / `--fg-subtle` | neutral greys | `#b7a9d8` / `#8b7dad` | the light mauve |
| `--brand` | `#ff6363` | `#ff2f92` | the magenta crest |
| `--info` / `--line-focus` | `#56c2ff` | `#b9a6ff` | the lighter violet |
| `--danger` | `#ff6363` | `#ff5c7a` | a rose that stays distinct from the brand |
| `--warning` | `#ff9217` | `#ffa63d` | warmed to sit on violet |
| `--success` | `#59d499` | unchanged | mint reads on violet and keeps its meaning |

**S3 — what does not change.**

The progress ramp still means reading progress (red → orange → yellow → green) and its colours are
data, not theme; the one loud control is still the near-white fill with dark text; the loud hover and
the modal shadow take the canvas's violet instead of pure black.

---

## Log

**L1 — the request, verbatim.**
> "Por ultimo, quiero que adaptes la paleta de colores de la aplicacion y te guies con los colores de
> esta imagen /home/tomas-cuevas/Pictures/Wallpapers/milad-fakurian.jpg"

**L2 — decisions.**

- The palette is sampled, not eyeballed: the hex values above come from the file (`PIL`, 17 px grid).
- The brand is the crest's magenta at a slightly gentler value (`#ff2f92`): the pure `#ff008c` is a
  two-pixel highlight and vibrating on large areas.
- `--danger` moved to a rose so "remove this" and "the product's mark" cannot be confused now that the
  mark is pink.
- The progress ramp is untouched: it is an encoding of reading state, and re-hueing it would change what
  the screen says rather than how it looks.

**L3 — evidence.**

Every colour lives in `styles/tokens.css` except a handful of surface overlays that sit ON a cover or a
dialog; those were retinted to the canvas violet (the page-count badge, **the** carousel arrow ground, the
primary hover, the danger tint and the close button's glow). Captures at `columns-3` and `auto` show the
new field, bands, mark and borders with the same hierarchy as before.

**L4 — the first palette was too loud; the second is glass.**
> "No me gusta el color que quedo. Quiero que la paleta sea mas sutil, tenga efectos de destellos y
> liquid glass"

The hues are the wallpaper's, but the saturation moved out of the paint and into the light. Surfaces are
now translucent whites over the near-black indigo (`rgba(255,255,255,.035 → .11)`), so what is behind them
shows through; floating chrome (top bar, bottom bar, sidebar, dialog) is a glass pane — a tinted
`--glass` / `--glass-strong`, `backdrop-filter: blur(20–26px) saturate(1.08)`, a 10 % border and a light
top edge. The strong magenta became a soft rose-lilac used only for the mark and its glint; danger, info,
warning and success were muted to sit on the field instead of shouting over it.

The glints: an aurora of two violet blooms and one pink one behind the whole shell (the thing the glass
refracts), a slow 7 s pulse on the mark's halo (off under `prefers-reduced-motion`), a light edge on every
pane, and the current navigation item and hovered settings row catching a breath of pink. Fields are
`rgba(255,255,255,.06)` with a `--line-strong` border and a soft focus halo, so "subtle" never costs the
affordance.

Supersedes S2's literal values, which were the saturated first pass. The ramp, the near-white loud
control and the geometry are untouched.

**L5 — the glints need ground, and the mid-pink needs a job.**
> "- El fondo del porcentaje actual no es tan notable por lo que se mezcla el numero con la barra de
> progreso. Unicamente esta bien cuando esta al 100%.
> - Lo mismo que lo anterior pasa en los iconos para editar y abrir carpeta contenedora. No tienen un
> fondo con suficiente blur que haga que el icono no se mezcle con la portada.
> - Debes utilizar un poco mas el color medio rosa que tiene la imagen que te pase. Este color podrias
> utilizarlo para la opcion activa del sidebar/navbar y otros pequeños detalles"

Three rules followed from it:

- **The percentage marker owns its ground.** It sits on the coloured bar, so the translucent surface it
  had let the bar's hue run through the digits. It is now a near-opaque dark pane (`rgba(10,7,20,.86)`)
  with 6 px of blur, a 22 % white hairline and a shadow; the 100 % marker keeps its green fill.
- **The cover pills are glass, not tint.** Their 3.5 % white surface vanished on a light cover. They now
  use a dark pane (`rgba(10,7,20,.55)`) with 10 px of blur, a 16 % hairline and an inner top light — the
  same material as the page-count badge, which was already doing this correctly.
- **The mid-pink has a role.** `--brand-mid` (`#c47aa8`, sampled from the wallpaper between field and
  crest) fills the current destination in the bottom bar and the sidebar, the current window-layout and
  language choices, and the current page in the pager; the active nav icon takes the brighter
  `--brand`. Everything else stays achromatic violet.

Captures at `columns-3` (with the pointer on a card) and `auto` confirm the marker is legible at 18/72/93 %,
the two pills read on the art, and the current destination and choice are pink without competing with the
progress ramp.

**L6 — anything that floats needs a ground that ends the layer.**
> "Los popover no se ven correctamente, no tienen un background que los proteja de no mezclar su contenido
> con el resto. Tambien, el selector de autor en el editor del documento no tiene un background que lo
> proteja."

Both were the same mistake: a floating surface painted with a translucent *theme* surface, which assumes
something calm behind it. Behind a popover there is cover art; behind the author list there is the form's
own labels.

- Card popovers (`BookCard`, and the restore-title popover in `BookGrid`) — `rgba(10,7,20,.92)`, 8 px blur,
  18 % white hairline, shadow and inset highlight.
- The author combobox list — `rgba(12,8,24,.96)`, 14 px blur, 18 % hairline, the existing 36 px shadow plus
  an inset highlight; its hovered/highlighted row went from 11 % white surface to `rgba(255,255,255,.12)` so
  the row still reads on the darker pane.

Verified by capture at `columns-3`: the folder popover reads over the cover, and typing "car" opens a list
that ends the dialog's text behind it.

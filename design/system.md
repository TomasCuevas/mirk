# mirk — design system

Captured from the project's own sources on 2026-11-06. Every value below exists in the source named
beside it; nothing here is invented.

Source of truth: `web/src/styles/tokens.css` (one file, one source of truth — its own words). The two
`pdf_viewer.css` files the canvas reported are vendored pdf.js viewer styles, not mirk's system, and are
excluded deliberately.

Window scale: the window opens at **430 × 844** and the reader can widen it (`Ajustes → Ventana`: two,
three or four per row, or full screen). The scale is sized for the narrowest window, not for a browser.

## Color

All from `web/src/styles/tokens.css`.

| Token | Value | Usage |
| --- | --- | --- |
| `--canvas` | `#0a0714` | The field. Near-black indigo, the wallpaper's own base. |
| `--surface` | `rgba(255,255,255,0.035)` | Unfilled panel over the canvas. |
| `--surface-1` | `rgba(255,255,255,0.05)` | One step up: rows, inputs at rest. |
| `--surface-2` | `rgba(255,255,255,0.075)` | Hover, raised row. |
| `--surface-3` | `rgba(255,255,255,0.11)` | Pressed, selected row. |
| `--glass` | `rgba(18,13,32,0.5)` | Tint for floating panes (top bar, navigation, dialog). |
| `--glass-strong` | `rgba(24,18,40,0.68)` | Floating pane over busy content. |
| `--glass-border` | `rgba(255,255,255,0.1)` | The edge of a glass pane. |
| `--glass-highlight` | `rgba(255,255,255,0.12)` | The light a glass pane catches on its top edge. |
| `--glass-blur` | `20px` | The blur itself. |
| `--line` | `rgba(255,255,255,0.07)` | Hairline between things at rest. |
| `--line-strong` | `rgba(255,255,255,0.13)` | A border that means something. |
| `--line-focus` | `#a9b8f0` | Focus ring. |
| `--fg` | `#f2eef9` | Body text and the only text on a loud control. |
| `--fg-muted` | `#a79fbb` | Secondary text. |
| `--fg-subtle` | `#7b7391` | Non-essential marks only (4.0:1 — not body text). |
| `--fg-on-loud` | `#0b0714` | Text on a white fill. |
| `--brand` | `#e6a3c9` | The mark and nothing else. Not an action colour. |
| `--brand-glow` | `rgba(230,163,201,0.35)` | Halo behind the mark. |
| `--brand-mid` | `#c47aa8` | Fills active choices and the current page: "where am I" in the wallpaper's own colour. |
| `--brand-mid-soft` | `rgba(196,122,168,0.22)` | The same, as a wash. |
| `--danger` | `#e58a95` | Failure, destructive. |
| `--success` | `#8fd6b4` | Done, saved. |
| `--warning` | `#e0b78d` | Attention, not failure. |
| `--info` | `#a9b8f0` | Neutral information. |

**The rule that governs the palette:** the chrome is achromatic. Colour states a fact about a document,
so the progress ramp is the only saturated thing on a screen, and `--brand` never competes with it.

## Typography

| Token | Value | Usage |
| --- | --- | --- |
| `--font-sans` | `"Inter", system-ui, -apple-system, "Segoe UI", sans-serif` | The interface. |
| `--font-display` | `"Instrument Serif", Georgia, serif` | The screen's own voice — once per screen, never twice. |
| `--text-xs` | `11px` | Provenance, badges. |
| `--text-sm` | `13px` | Card text, controls. |
| `--text-base` | `14px` | Body. |
| `--text-lg` | `16px` | Section lead. |
| `--text-xl` | `18px` | Panel title. |
| `--display` | `30px` | The screen title, in Instrument Serif. |
| `--tracking-tight` | `-0.012em` | Display text. |
| `--tracking-wide` | `0.14em` | Small caps labels. |
| `--weight-regular` / `medium` / `semibold` | `400` / `500` / `600` | Three weights, no more. |

Both faces ship as local `woff2` under `/fonts/`. **Artboards approximate them** with `Georgia` for the
display face and `system-ui` for the interface, because an artboard may not reference a remote or
project-hosted font.

## Geometry and motion

| Token | Value |
| --- | --- |
| `--r-xs` … `--r-xl` | `4px`, `6px`, `8px`, `12px`, `16px` |
| `--r-full` | `999px` |
| `--space-1` … `--space-6` | `4px`, `8px`, `12px`, `16px`, `20px`, `24px` |
| `--ease` | `cubic-bezier(0.2, 0.8, 0.2, 1)` |
| `--fast` | `120ms` |
| `--shadow-modal` | `0 30px 80px rgba(4,2,10,0.55), 0 0 0 1px var(--glass-border)` |

## Component rules

- **Glass is for things that float** — the top bar, the navigation, a dialog. Panels use the translucent
  surfaces and a sheen instead: a blur behind every row is a cost with no visible gain.
- **One loud control per screen**: a white fill with `--fg-on-loud` text. Everything else is a surface.
- **The display face appears once per screen.** Used twice it stops being a voice and becomes a costume.
- **Progress is the only saturated thing.** A page counted by the reader, one typed by hand and one read
  from another viewer are three different facts; the interface must never render them identically.
- **Contrast**: body text is `--fg` or `--fg-muted`; `--fg-subtle` never carries information a reader
  needs.

/**
 * The window-chrome controls the custom top bar draws (A2), and the four layouts the window can take
 * (S2 of `odd/tasks/window-layout.md`).
 *
 * The application is undecorated on purpose — native decorations would bring a maximise button the
 * user explicitly does not want — so the three controls the user does want live here as data.
 * Keeping them as data is what makes "there is no maximise control" testable instead of a promise:
 * the top bar renders exactly this list, and its test asserts the list has no such thing.
 *
 * The window's SIZE is a choice of its own, and it is not a fourth chrome control: it lives in Ajustes
 * → Ventana, because it is a stored preference like the language rather than an action on the window.
 * The three presets state how many documents a row holds, and the desktop process sizes the window to
 * fit exactly that row; full screen is the one layout with no count.
 *
 * The commands are the Tauri commands in `crates/mirk-desktop`; each one performs the same
 * [`ShellAction`] the tray menu performs.
 */

/** The three things the top bar can ask the shell to do. */
export type WindowAction = "minimize" | "hide" | "close";

/**
 * The drawing a control uses.
 *
 * The first version used font glyphs (`–`, `⌄`, `✕`), and the user's verdict was that they read as
 * characters borrowed from a font rather than as this application's own controls — the three of them
 * came from three different typefaces. These are three strokes in one hand: the same 1.6 weight, the
 * same rounded caps, the same 16 px box. Minimise is a bar, hiding is an arrow into a tray (which is
 * what the action does, and what a bar cannot say), and closing is a cross.
 */
export type WindowIcon = "minimize" | "hide" | "close";

/** One control in the top bar. */
export interface WindowControl {
  /** The shell action this control performs. */
  action: WindowAction;
  /** The Tauri command that performs it. */
  command: string;
  /** The catalogue key for its accessible label and tooltip. */
  labelKey: string;
  /** The drawing inside the button. Decorative; the label is the accessible name. */
  icon: WindowIcon;
}

/**
 * Exactly minimise, hide and close — in that order, and nothing else.
 *
 * There is deliberately no maximise entry. The window is also configured `maximizable: false` and
 * `resizable: false`, so neither the button nor a dragged edge can change its size; the four layouts
 * below are the only way it changes, and `fullscreen: false` only means the application does not
 * START full screen.
 */
export const windowControls: readonly WindowControl[] = [
  { action: "minimize", command: "minimize_window", labelKey: "window.minimize", icon: "minimize" },
  { action: "hide", command: "hide_window", labelKey: "window.hide", icon: "hide" },
  { action: "close", command: "close_app", labelKey: "window.close", icon: "close" },
];

/**
 * One of the four window layouts the reader can choose (S2).
 *
 * `columns-2`, `columns-3` and `columns-4` name the documents a row holds and the window is sized to fit
 * exactly that row; `auto` is full screen, where the grid fills the width with as many documents as the
 * card's own minimum allows.
 */
export type WindowLayout = "columns-2" | "columns-3" | "columns-4" | "auto";

/**
 * Every layout, in the order Ajustes → Ventana offers them: the current window first, the widest preset
 * next, and full screen last because it is the one that leaves the window's own size behind.
 */
export const windowLayouts: readonly WindowLayout[] = [
  "columns-2",
  "columns-3",
  "columns-4",
  "auto",
];

/** The layout in force when nothing is stored: the window the application already shipped with. */
export const DEFAULT_WINDOW_LAYOUT: WindowLayout = "columns-2";

/** Whether a stored or received value is a layout this build knows. Anything else is not one. */
export function isWindowLayout(value: unknown): value is WindowLayout {
  return typeof value === "string" && (windowLayouts as readonly string[]).includes(value);
}

/**
 * The `grid-template-columns` count a layout asks the document grid for (S4).
 *
 * A preset states its count, so "three per row" is guaranteed instead of being an outcome of a `minmax`
 * computation that could disagree with the window the desktop process sized for it. `auto` returns
 * `auto-fill`, which is the rule the grid already used: as many cards as fit at the card's minimum.
 * The value is meant for the `--grid-columns` custom property, so `repeat()` takes it verbatim.
 */
export function gridColumns(layout: WindowLayout): string {
  if (layout === "auto") {
    return "auto-fill";
  }
  return layout.slice("columns-".length);
}

/**
 * The catalogue key that names a layout in Ajustes → Ventana (S5).
 *
 * The key is derived from the layout rather than stored in a table, so a layout cannot be offered with
 * another layout's words. Both catalogues are checked against it by `window.test.ts`.
 */
export function layoutLabelKey(layout: WindowLayout): string {
  return layout === "auto"
    ? "settings.window.auto"
    : `settings.window.columns${layout.slice("columns-".length)}`;
}

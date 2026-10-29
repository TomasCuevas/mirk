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

/** The four things the top bar can ask the shell to do (S4). */
export type WindowAction = "minimize" | "maximize" | "hide" | "close";

/**
 * The drawing a control uses.
 *
 * The first version used font glyphs (`–`, `⌄`, `✕`), and the user's verdict was that they read as
 * characters borrowed from a font rather than as this application's own controls — the three of them
 * came from three different typefaces. These are three strokes in one hand: the same 1.6 weight, the
 * same rounded caps, the same 16 px box. Minimise is a bar, hiding is an arrow into a tray (which is
 * what the action does, and what a bar cannot say), and closing is a cross.
 */
export type WindowIcon = "minimize" | "maximize" | "restore" | "hide" | "close";

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
 * Exactly minimise, hide, maximise/restore and close — in that order (S4, R6).
 *
 * The maximise entry is the one whose label and drawing depend on the window's state: it names
 * "maximise" while a windowed layout is in force and "restore" while the full-screen layout is. It does
 * not carry a window command of its own: it chooses the same stored layout Ajustes → Ventana does, which
 * is what keeps the button and the selected option in agreement.
 */
export const windowControls: readonly WindowControl[] = [
  // R6: the two window states read left to right — minimise first, then the tray control — because
  // the user asked for that order and, with full screen the stored default, maximise sits beside
  // close as the outer pair.
  { action: "minimize", command: "minimize_window", labelKey: "window.minimize", icon: "minimize" },
  { action: "hide", command: "hide_window", labelKey: "window.hide", icon: "hide" },
  {
    action: "maximize",
    command: "set_window_layout",
    labelKey: "window.maximize",
    icon: "maximize",
  },
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

/** The layout in force when nothing is stored: full screen (R7 of `odd/tasks/desktop-chrome.md`). */
export const DEFAULT_WINDOW_LAYOUT: WindowLayout = "auto";

/**
 * The windowed layout the maximise control returns to (R7).
 *
 * It is a constant of its own rather than the default: the default is full screen, so "restore"
 * cannot fall back to it. A reader who never chose a preset restores to three per row, which is what
 * the user asked for — the window comes back to a size that shows the grid the application opens
 * with once, rather than to the narrowest one.
 */
export const RESTORE_WINDOW_LAYOUT: WindowLayout = "columns-3";

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
/**
 * The documents a Biblioteca page holds, for a layout (L11 of `odd/tasks/window-layout.md`).
 *
 * A page that ends mid-row leaves holes that read as missing documents, so the count follows the row:
 * two per row keeps the ten the library always had, three and four hold twelve, and full screen — the
 * layout with the most room — holds fourteen. The numbers are the reader's, not a derivation.
 */
export function libraryPageSize(layout: WindowLayout): number {
  switch (layout) {
    case "columns-2":
      return 10;
    case "columns-3":
    case "columns-4":
      return 12;
    case "auto":
      return 14;
  }
}

/**
 * The root font size a layout draws on, in CSS pixels (L15 of `odd/tasks/responsive.md`).
 *
 * Full screen is read from farther away, so the whole interface grows: every size in the token layer is
 * expressed in `rem`, which means one root value scales the type, the spacing, the control heights and
 * the sidebar together. The three windowed layouts keep the shipped 16 px — their widths were derived
 * from it, and moving it would resize windows the reader did not resize.
 */
export function interfaceFontSize(layout: WindowLayout): number {
  return layout === "auto" ? 18 : 16;
}

/**
 * The grid's minimum card width, in CSS pixels, for a layout (L10 of `odd/tasks/window-layout.md`).
 *
 * The three windowed layouts size the window for an exact count, so their floor is the card's own
 * declared minimum — 8.5 rem, 136 px — and never binds. Full screen has no count: `auto-fill` decides,
 * and at the bare 8.5 rem floor a large screen fit a dozen tiny cards, which the reader rejected. Its
 * floor is therefore the width two columns already have in the shipped window —
 * `(430 − 32 padding − 9.6 gap) / 2 = 194.2` px — so full screen shows cards the size the fixed layouts
 * show, and as many of them as fit at that size.
 */
export function gridCardMinimum(layout: WindowLayout): number {
  if (layout !== "auto") {
    return 8.5 * 16;
  }
  const base = (430 - 32 - 9.6) / 2;
  return base * (interfaceFontSize(layout) / 16);
}

/**
 * Where the navigation sits for a layout: the bottom bar, or the full screen's left sidebar (S3 of
 * `odd/tasks/navigation.md`).
 *
 * It is derived from the stored layout rather than from the viewport's width, because the reader chose
 * a layout and not a breakpoint: a three- or four-per-row window is still a window, and keeps the bar it
 * had. Only full screen, which fills the work area, has the room a sidebar needs.
 */
export type NavMode = "bottom" | "side";

/** The navigation's mode for `layout`. */
export function navMode(layout: WindowLayout): NavMode {
  return layout === "auto" ? "side" : "bottom";
}

export function layoutLabelKey(layout: WindowLayout): string {
  return layout === "auto"
    ? "settings.window.auto"
    : `settings.window.columns${layout.slice("columns-".length)}`;
}

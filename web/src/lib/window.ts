/**
 * The window-chrome controls the custom top bar draws (A2).
 *
 * The application is undecorated on purpose — native decorations would bring a maximise button the
 * user explicitly does not want — so the three controls the user does want live here as data.
 * Keeping them as data is what makes "there is no maximise control" testable instead of a promise:
 * the top bar renders exactly this list, and its test asserts the list has no such thing.
 *
 * The commands are the Tauri commands in `crates/oct-desktop`; each one performs the same
 * [`ShellAction`] the tray menu performs.
 */

/** The three things the top bar can ask the shell to do. */
export type WindowAction = "minimize" | "hide" | "close";

/** One control in the top bar. */
export interface WindowControl {
  /** The shell action this control performs. */
  action: WindowAction;
  /** The Tauri command that performs it. */
  command: string;
  /** The catalogue key for its accessible label and tooltip. */
  labelKey: string;
  /** The visual mark shown inside the button. Decorative; the label is the accessible name. */
  glyph: string;
}

/**
 * Exactly minimise, hide and close — in that order, and nothing else.
 *
 * There is deliberately no maximise entry. The window is also configured `maximizable: false`,
 * `resizable: false` and `fullscreen: false`, so the absence of the button is enforced by the
 * window itself and not only by this list.
 */
export const windowControls: readonly WindowControl[] = [
  { action: "minimize", command: "minimize_window", labelKey: "window.minimize", glyph: "–" },
  { action: "hide", command: "hide_window", labelKey: "window.hide", glyph: "⌄" },
  { action: "close", command: "close_app", labelKey: "window.close", glyph: "✕" },
];

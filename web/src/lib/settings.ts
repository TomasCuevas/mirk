/**
 * The settings the interface and the desktop process share: the language and the window layout.
 *
 * The tray's words are built by the desktop process (R1), and the window's size is applied by it before
 * the window is shown, so neither choice can be the interface's private state: both are written where
 * that process reads them and read back when the interface starts. The store is that place, and
 * `invoke` is injected rather than imported so the rules below can be checked without a Tauri
 * application — the same injection the Rust commands use.
 */

import { isLocale, type Locale } from "./i18n";
import { isWindowLayout, type WindowLayout } from "./window";

/** Calls a command exposed by `mirk-desktop`, returning its deserialised value. */
export type Invoke = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;

/**
 * The stored language, or `null` when nothing is stored or when what is stored is not a language
 * this build knows.
 *
 * Those two are the same answer on purpose — both mean "the interface keeps its default" — and a
 * failure to read is deliberately *not* one of them: the caller is handed the error rather than a
 * quiet `null` that would hide a database that cannot be opened.
 */
export async function loadLanguage(invoke: Invoke): Promise<Locale | null> {
  const stored = await invoke<unknown>("get_language");
  return isLocale(stored) ? stored : null;
}

/** Stores `locale`, which is what lets the tray follow the choice and the next launch remember it. */
export async function saveLanguage(invoke: Invoke, locale: Locale): Promise<void> {
  await invoke("set_language", { locale });
}

/**
 * The stored window layout, or `null` when nothing is stored or when what is stored is not a layout this
 * build knows.
 *
 * Both are the same answer on purpose — the caller keeps the default — while a failure to read is handed
 * to the caller as an error, exactly as [`loadLanguage`] does: "nothing was chosen" and "the database
 * could not be read" must not look alike.
 */
export async function loadWindowLayout(invoke: Invoke): Promise<WindowLayout | null> {
  const stored = await invoke<unknown>("get_window_layout");
  return isWindowLayout(stored) ? stored : null;
}

/**
 * Stores `layout` and asks the desktop process to apply it.
 *
 * It is one command rather than two because the two halves must not disagree: the process writes the
 * choice and sizes its own window from it, so a failure leaves both the store and the window as they
 * were instead of a stored choice the window never took.
 */
export async function saveWindowLayout(invoke: Invoke, layout: WindowLayout): Promise<void> {
  await invoke("set_window_layout", { layout });
}

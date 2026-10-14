/**
 * The one setting the interface and the desktop process share: the language.
 *
 * The tray's words are built by the desktop process (R1), so the language cannot be the interface's
 * private state: it is written where that process reads it and read back when the interface starts.
 * The store is that place, and `invoke` is injected rather than imported so the rules below can be
 * checked without a Tauri application — the same injection the Rust commands use.
 */

import { isLocale, type Locale } from "./i18n";

/** Calls a command exposed by `oct-desktop`, returning its deserialised value. */
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

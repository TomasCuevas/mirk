/**
 * Which screen is shown, and what the two screens that are not the library say.
 *
 * The same discipline as `library.ts`: the decisions live here as plain values and functions, so
 * they can be tested without mounting a component and so no screen can invent its own wording.
 *
 * # Why this is separate from `library.ts`
 *
 * `library.ts` owns the honesty rule about reading progress. This module owns the shell: the three
 * destinations, the hash that names one, and the text of the screens that do not list documents.
 * Keeping them apart means the fairness rule has one home and the navigation has another.
 */

import type { Translate } from "./i18n";

/** The three destinations of the bottom navbar (S1). */
export type Screen = "home" | "library" | "settings";

/** Every screen, in navbar order. */
export const screens: readonly Screen[] = ["home", "library", "settings"];

/** Whether a string names a screen. Anything else is not a destination. */
export function isScreen(value: string): value is Screen {
  return (screens as readonly string[]).includes(value);
}

/**
 * The screen a URL hash names, defaulting to Inicio.
 *
 * Navigation writes the screen into the hash and reads it back, so a reload keeps the reader where
 * they were and an unknown or hand-typed hash lands somewhere honest instead of a blank page.
 */
export function screenFromHash(hash: string): Screen {
  const name = hash.startsWith("#/") ? hash.slice(2) : "";
  return isScreen(name) ? name : "home";
}

/** The hash that names a screen. The inverse of [`screenFromHash`]. */
export function hashForScreen(screen: Screen): string {
  return `#/${screen}`;
}

/** The navbar label for a screen. */
export function screenLabel(screen: Screen, t: Translate): string {
  return t(`nav.${screen}`);
}

/** One summary Inicio promises to show once its queries exist. */
export interface HomeSection {
  title: string;
  detail: string;
}

/**
 * What Inicio will show, as three sections with no numbers in them.
 *
 * The numbers — in progress, finished, not read in a long time — need queries that do not exist
 * yet, and one of them depends on `documents.last_opened_at`, which nothing writes. **A screen that
 * says "not yet" is better than one that shows invented statistics** (L19), so this placeholder
 * names the three summaries and stops there.
 */
export function homeSections(t: Translate): HomeSection[] {
  return [
    { title: t("home.inProgress.title"), detail: t("home.inProgress.detail") },
    { title: t("home.finished.title"), detail: t("home.finished.detail") },
    { title: t("home.stale.title"), detail: t("home.stale.detail") },
  ];
}

/** A watched folder exactly as the store records it. */
export interface WatchedFolder {
  id: number;
  path: string;
  recursive: boolean;
  enabled: boolean;
}

/**
 * What Configuración says about the watched folders, or `null` once there is a list to render.
 *
 * `null` (still reading the store) and `[]` (nothing recorded) are **different facts**, the same
 * distinction the provider registry makes: a screen that says "no folders" before the answer
 * arrives is asserting something it does not know.
 */
export function describeWatchedFolders(
  folders: readonly WatchedFolder[] | null,
  t: Translate,
): string | null {
  if (folders === null) {
    return t("settings.folders.loading");
  }
  return folders.length === 0 ? t("settings.folders.empty") : null;
}

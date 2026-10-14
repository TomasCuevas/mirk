/**
 * Which screen is shown, and the wording the shell shows around it.
 *
 * The same discipline as `library.ts`: the decisions live here as plain values and functions, so
 * they can be tested without mounting a component and so no screen can invent its own wording.
 *
 * # Why this is separate from `library.ts`
 *
 * `library.ts` owns the honesty rule about reading progress — including which documents belong to
 * each home category. This module owns the shell: the destinations and the hash that names one, the
 * headings and definitions of the home categories, and the text of the screens that do not list
 * documents. Keeping them apart means the fairness rule has one home and the navigation has
 * another.
 */

import type { Translate } from "./i18n";
import { STALE_AFTER_DAYS, isHomeCategory, type HomeCategory } from "./library";

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
  const name = hashName(hash);
  return isScreen(name) ? name : "home";
}

/** The screen name inside a `#/...` hash, or an empty string when there is none. */
function hashName(hash: string): string {
  return hash.startsWith("#/") ? hash.slice(2) : "";
}

/** The hash that names a screen. The inverse of [`screenFromHash`]. */
export function hashForScreen(screen: Screen): string {
  return `#/${screen}`;
}

/**
 * A hash-named view: one of the navbar screens, or one home category's list on a screen of its own
 * (B2).
 */
export type View =
  | { readonly kind: "screen"; readonly screen: Screen }
  | { readonly kind: "category"; readonly category: HomeCategory };

/**
 * The view a hash names, defaulting to Inicio.
 *
 * A category list is a screen of its own rather than a flag on Inicio (B2), so a reload or a shared
 * link lands on the list the reader was looking at. An unknown category falls back to Inicio, the
 * same way [`screenFromHash`] falls back for an unknown screen.
 */
export function viewFromHash(hash: string): View {
  const name = hashName(hash);
  const prefix = "category/";
  if (name.startsWith(prefix)) {
    const category = name.slice(prefix.length);
    if (isHomeCategory(category)) {
      return { kind: "category", category };
    }
  }
  return { kind: "screen", screen: isScreen(name) ? name : "home" };
}

/** The hash that names a view. The inverse of [`viewFromHash`]. */
export function hashForView(view: View): string {
  return view.kind === "category" ? `#/category/${view.category}` : hashForScreen(view.screen);
}

/** The navbar label for a screen. */
export function screenLabel(screen: Screen, t: Translate): string {
  return t(`nav.${screen}`);
}

/**
 * The heading Inicio's carousel for `category` shows (B4).
 *
 * A category's definition is stated on screen because "finished" and "in progress" are readings of
 * a stored position, not facts the document carries: a reader who disagrees with the list can see
 * the rule that produced it. The stale definition carries the threshold itself, so "a long time" is
 * a number the reader can read rather than one hidden in the code (B3/B4).
 */
export function categoryTitle(category: HomeCategory, t: Translate): string {
  return t(`home.${category}.title`);
}

/** The definition a category states about itself, threshold included for the stale category (B4). */
export function categoryDefinition(category: HomeCategory, t: Translate): string {
  return t(`home.${category}.definition`, { days: STALE_AFTER_DAYS });
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

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
 * A hash-named view: one of the navbar screens, one home category's list on a screen of its own (B2),
 * one of Ajustes' sub-screens (R43/S2), or the suggestions screen (S1 of
 * `odd/tasks/suggestions-screen.md`).
 */
export type View =
  | { readonly kind: "screen"; readonly screen: Screen }
  | { readonly kind: "category"; readonly category: HomeCategory }
  | { readonly kind: "settings"; readonly section: SettingsSection }
  | { readonly kind: "suggestions" };

/**
 * The sections Ajustes is an index of (R43/S1, plus S5 of `odd/tasks/window-layout.md`).
 *
 * The user's report was that Ajustes was *"nada intuitiva"*, and the shape they chose when asked was an
 * index of three sub-screens rather than five unrelated blocks on one page. The order is theirs —
 * Biblioteca, Lectura, Aplicación — and it is the order the index renders in. Ventana was appended last
 * when the window's size became a stored choice, so the three rows the reader chose keep their order.
 */
export type SettingsSection = "library" | "reading" | "app" | "window";

/** Every settings section, in the index's order. */
export const settingsSections: readonly SettingsSection[] = [
  "library",
  "reading",
  "app",
  "window",
];

/** Whether a string names a settings section. Anything else is not a sub-screen. */
export function isSettingsSection(value: string): value is SettingsSection {
  return (settingsSections as readonly string[]).includes(value);
}

/**
 * The hash the suggestions screen answers to (S1).
 *
 * It is its own address and not a flag on Biblioteca, so a reload — or a link the reader keeps — lands on
 * the queue rather than on the library, exactly as a home category and an Ajustes sub-screen already do.
 */
const SUGGESTIONS_HASH = "suggestions";

/**
 * The view a hash names, defaulting to Inicio.
 *
 * A category list is a screen of its own rather than a flag on Inicio (B2), so a reload or a shared
 * link lands on the list the reader was looking at. An unknown category falls back to Inicio, the
 * same way [`screenFromHash`] falls back for an unknown screen.
 *
 * A settings sub-screen is a view of its own for the same reason (R43/S2), and its fallback is
 * different on purpose: an unknown category belongs to a screen the reader did not ask for, while an
 * unknown sub-screen belongs to Ajustes — so it lands on the index, with the three choices in front of
 * them, rather than on Inicio.
 *
 * The suggestions screen is neither a navbar screen nor a child of one in the type's own terms: it is
 * reached from Biblioteca (S8) but it is not a view OF Biblioteca's listing, so it gets its own kind
 * rather than a category-like seat inside `screens` — which would have made it a fourth navbar entry,
 * the thing S8 refuses.
 */
export function viewFromHash(hash: string): View {
  const name = hashName(hash);

  if (name === SUGGESTIONS_HASH) {
    return { kind: "suggestions" };
  }

  const categoryPrefix = "category/";
  if (name.startsWith(categoryPrefix)) {
    const category = name.slice(categoryPrefix.length);
    if (isHomeCategory(category)) {
      return { kind: "category", category };
    }
  }

  const settingsPrefix = "settings/";
  if (name.startsWith(settingsPrefix)) {
    const section = name.slice(settingsPrefix.length);
    if (isSettingsSection(section)) {
      return { kind: "settings", section };
    }
    return { kind: "screen", screen: "settings" };
  }

  return { kind: "screen", screen: isScreen(name) ? name : "home" };
}

/** The hash that names a view. The inverse of [`viewFromHash`]. */
export function hashForView(view: View): string {
  if (view.kind === "category") {
    return `#/category/${view.category}`;
  }
  if (view.kind === "settings") {
    return `#/settings/${view.section}`;
  }
  if (view.kind === "suggestions") {
    return `#/${SUGGESTIONS_HASH}`;
  }
  return hashForScreen(view.screen);
}

/**
 * The navbar destination a view belongs to.
 *
 * A category list belongs to Inicio, a settings sub-screen belongs to Ajustes, and the suggestions queue
 * belongs to Biblioteca — which is the screen it is reached from (S8) — so the navbar keeps that
 * destination marked while the reader is inside one rather than marking nothing, which would leave them
 * with no answer to "where am I". The rule lives here, once, because it is the same rule for every kind
 * of view and a screen must not carry its own version of it. Because Biblioteca is what stays marked,
 * the suggestions screen is not a fourth navbar entry: the bar still has three.
 */
export function navbarScreen(view: View): Screen {
  switch (view.kind) {
    case "category":
      return "home";
    case "settings":
      return "settings";
    case "suggestions":
      return "library";
    case "screen":
      return view.screen;
  }
}

/** The name of one settings section, as the index and the sub-screen's own heading show it. */
export function settingsSectionTitle(section: SettingsSection, t: Translate): string {
  return t(`settings.${section}.title`);
}

/**
 * The one sentence the index says under a section's name, saying what lives inside it (R43/S1).
 *
 * It is a separate string from the sub-screen's purpose line on purpose: the index answers "what is in
 * there" for a reader choosing, and the sub-screen answers "what is this for" for a reader who has
 * already chosen. One sentence doing both jobs would be the wall this round removed.
 */
export function settingsSectionSummary(section: SettingsSection, t: Translate): string {
  return t(`settings.${section}.summary`);
}

/** The one line a sub-screen states about itself, saying what it is for (R43/S4). */
export function settingsSectionPurpose(section: SettingsSection, t: Translate): string {
  return t(`settings.${section}.purpose`);
}

/** The navbar label for a screen. */
export function screenLabel(screen: Screen, t: Translate): string {
  return t(`nav.${screen}`);
}

/**
 * The heading Inicio's carousel for `category` shows (B4).
 *
 * The CAROUSELS used to state each category's definition under this heading, and the reader asked for
 * those three sentences to go (L70): a heading that already says "En curso" or "Terminados" does not
 * need a line explaining what the words mean, and three of those lines made Inicio read like a manual.
 * The category's own screen still states its definition — one line on a screen that holds nothing
 * else is not noise, and it is where a reader who disagrees with the list can see the rule that
 * produced it — so the helper below stays and only its use on Inicio was removed.
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

/**
 * The path components of `path`, ignoring empty ones.
 *
 * Two paths are compared component by component and never by the characters they spell: `/library/pdfs-old`
 * shares the whole string prefix `/library/pdfs` and is a different folder. Every "is this inside that"
 * question in the interface goes through this one function so there is one rule rather than three.
 */
export function pathSegments(path: string): string[] {
  return path.split("/").filter((part) => part !== "");
}

/**
 * The watched folder that already covers `candidate`, or `null` when none does (R22).
 *
 * Adding a folder inside a watched one would watch the same files twice and split one library across two
 * records, so the interface refuses it — and it has to name the offender, because "it was blocked" with
 * no "by which one" is a half-answer. The interface does this check as well as the Rust side so the
 * explanation can be in the reader's own language; the Rust side enforces it because it is the truth.
 *
 * The comparison is by path SEGMENT and never by string prefix: `/library/pdfs-old` is not inside
 * `/library/pdfs`, and a prefix comparison would say it is. A path equal to a watched one is covered, and
 * so is a deeper one; a shorter or unrelated path is not.
 */
export function coveringFolder(
  folders: readonly WatchedFolder[],
  candidate: string,
): WatchedFolder | null {
  const wanted = pathSegments(candidate);

  // When more than one watch covers the choice, the TIGHTEST one is named — the same rule the Rust
  // side applies, so the alert and the error speak about the same folder instead of two folders that
  // are both technically true.
  return folders.reduce<WatchedFolder | null>((tightest, folder) => {
    const watched = pathSegments(folder.path);
    const covers =
      wanted.length >= watched.length &&
      watched.every((part, index) => part === wanted[index]);
    if (!covers) {
      return tightest;
    }
    return tightest === null || watched.length > pathSegments(tightest.path).length
      ? folder
      : tightest;
  }, null);
}

/**
 * A folder the interface refused to add, and the path that was refused (R22).
 *
 * The candidate is carried alongside the covering folder because the *sentence* depends on both: when the
 * two name the same folder, telling the reader it is inside itself explains nothing.
 */
export interface FolderRefusal {
  covered: WatchedFolder;
  candidate: string;
}

/**
 * The sentence that explains why a chosen folder was not added (R22, R26).
 *
 * The covering folder can be the chosen path itself — `/lib` is inside `/lib` — and telling a person that
 * a folder is inside itself is true and useless: they learn nothing they did not already know. The two
 * cases get their own sentences, and the case is decided here rather than in the screen, because a screen
 * that had to work out which sentence applied would be a second implementation of the same rule.
 */
export function describeCoveredFolder(
  covered: WatchedFolder,
  candidate: string,
  t: Translate,
): string {
  return isSamePath(covered.path, candidate)
    ? t("settings.folders.coveredSelf")
    : t("settings.folders.covered", { folder: covered.path });
}

/** Whether two paths name the same folder, compared component by component. */
function isSamePath(one: string, other: string): boolean {
  const left = pathSegments(one);
  const right = pathSegments(other);
  return left.length === right.length && left.every((part, index) => part === right[index]);
}

/**
 * What removing one watch would take with it, as the store computes it (R27).
 *
 * This is the store's answer and not the interface's guess: the same computation the removal itself
 * carries out is read first, so the number the confirmation states and the number of documents that
 * actually leave cannot disagree.
 */
export interface FolderRemoval {
  folder: WatchedFolder;
  /** Paths of the documents that leave the library. */
  documents: string[];
  /** Paths of the documents that stay because another remaining watch still covers them. */
  kept: string[];
}

/**
 * What the confirmation says about how much the removal takes (R27).
 *
 * A destructive action states its size before it happens, and "0" is not a smaller number: when the
 * folder has no document in the library the sentence says so, because "0 documents leave your library"
 * reads as a failure rather than as the answer.
 *
 * Nothing leaving has TWO different reasons, and they must not share a sentence. A folder can be empty of
 * the library, or every document in it can be covered by another remaining watch — the guard that keeps a
 * nested folder from being emptied. Saying the first thing when the second is true would deny documents
 * that are in the library and in the very sentence that follows.
 */
export function describeRemoval(removal: FolderRemoval, t: Translate): string {
  const count = removal.documents.length;
  if (count === 0) {
    return removal.kept.length > 0
      ? t("settings.folders.removalAllKept")
      : t("settings.folders.removalNone");
  }
  return count === 1
    ? t("settings.folders.removalOne")
    : t("settings.folders.removalMany", { count });
}

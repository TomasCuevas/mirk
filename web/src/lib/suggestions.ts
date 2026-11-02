import type { Translate } from "./i18n";

/**
 * The suggestion queue, as the interface sees it (L56 in \`odd/tasks/pdf-progress-tracker.md\`).
 *
 * # The rule this module exists to enforce
 *
 * Nothing enters the library because the application saw a document open. The queue records what was
 * observed; only the reader's own decision turns a suggestion into a library document. That is why
 * nothing here adds anything, and why the screen offers exactly the two explicit decisions and no
 * third way in.
 *
 * # Three states, never two
 *
 * \`null\` is "the answer has not arrived", an empty list is "there is genuinely nothing waiting", and a
 * list with items is a queue to decide. A screen that says "nothing to decide" before the answer
 * arrives is claiming something it does not know, which is the failure this project refuses
 * everywhere — and which the producer's own doc comment names for the case where the platform has no
 * detector at all.
 */

/** A document the application observed open that the reader has not decided about. */
export interface Suggestion {
  id: number;
  /**
   * The path as the backend displays it, with the home directory abbreviated.
   *
   * It is shown rather than a title, and that is correct here: a suggestion is not a library document,
   * so it has no title, and inventing one from the file name would be the screen guessing at data the
   * application never read.
   */
  path: string;
  /** How many times it has been observed open since the reader last acted. */
  seenCount: number;
  /**
   * Unix seconds of the first observation, which is what the backend orders the queue by. The screen
   * does not decide its order from this field (S4 of `odd/tasks/suggestions-screen.md`), and it is kept
   * because the queue's own read still orders by it.
   */
  firstSeenAt: number;
  /**
   * Unix seconds of the date the DESKTOP's history recorded for this document — when the reader opened
   * it, not when the application noticed (S4, S10).
   *
   * It is deliberately a fact of its own and never `firstSeenAt`: the two answer different questions, and
   * sharing one column between them has already produced a wrong answer once (L46).
   *
   * `null` and absent are one fact and not two — "no date was recorded" — because the column did not
   * always exist. Neither is "long ago": the order puts an undated suggestion **after** every dated one
   * rather than pretending it happened in 1970.
   */
  historyAt?: number | null;
}

/** What a finished preview request means for a card (L60). */
export type SuggestionCover =
  | { kind: "shown"; src: string }
  | { kind: "none" };

/** What the queue believes about this machine's ability to report open documents (L61, L63). */
export type DetectorBelief = "unknown" | "installed" | "unsupported" | "unavailable";

/** Reads the command's answer; anything unrecognised is unknown, never a claim (L63). */
export function detectorBelief(answer: string | null | undefined): DetectorBelief {
  return answer === "installed" ||
    answer === "unsupported" ||
    answer === "unavailable"
    ? answer
    : "unknown";
}

/** Only a definitive answer ends the asking: unknown and unavailable are worth another try (L63). */
export function detectorSettled(belief: DetectorBelief): boolean {
  return belief === "installed" || belief === "unsupported";
}

/** A failed ask never downgrades a definitive answer, and never upgrades a failure into one (L63). */
export const detectorAfterFailure = (previous: DetectorBelief): DetectorBelief =>
  detectorSettled(previous) ? previous : "unknown";

/**
 * Reads the answer to the only question the cover asks: what is this document's first page?
 *
 * The rule is the one the library's covers already follow: a preview that did not arrive keeps the
 * placeholder. It never becomes a broken image and never becomes an empty src — an image tag with
 * nothing behind it is a lie about the document, while a placeholder is the truth about the request.
 * A rejected call and a fulfilled null are one outcome for one reason.
 */
export function suggestionCover(
  settled: PromiseSettledResult<string | null>,
): SuggestionCover {
  if (
    settled.status === "fulfilled" &&
    typeof settled.value === "string" &&
    settled.value !== ""
  ) {
    return { kind: "shown", src: settled.value };
  }
  return { kind: "none" };
}

/** What a decision did, or why it did not. */
export type SuggestionOutcome =
  | { kind: "decided"; id: number }
  | { kind: "failed"; id: number; message: string };

/**
 * The document's own file name, for a card's heading.
 *
 * It is deliberately NOT presented as the document's title: a suggestion is a file mirk saw open, not a
 * document it has read, so nothing here claims an embedded title exists. This is the same fallback the
 * library already uses for a document nobody has named — the file's own name — and the full path stays
 * visible on the card beside it, so nothing about the document is hidden from the reader.
 */
export function suggestionTitle(path: string): string {
  const name = path.slice(path.lastIndexOf("/") + 1);
  const withoutExtension = name.replace(/\.pdf$/i, "");
  return withoutExtension === "" ? path : withoutExtension;
}

/**
 * The section's own state: the sentence to show, or \`null\` when there is a list to render instead.
 *
 * The three inputs are three different facts, and conflating "still checking" with "there are none" is
 * the defect this function exists to prevent.
 */
export function describeSuggestions(
  suggestions: readonly Suggestion[] | null,
  t: Translate,
  detector: DetectorBelief = "installed",
): string | null {
  if (suggestions === null) {
    return t("suggestion.loading");
  }
  // L63: only "unsupported" licenses a claim about the machine, and items always win over context.
  if (suggestions.length === 0 && detector === "unsupported") {
    return t("suggestion.noDetector");
  }
  return suggestions.length === 0 ? t("suggestion.none") : null;
}

/**
 * What the command answered, as a value rather than as control flow.
 *
 * A rejection is never dressed up as a recorded decision: believing a document joined the library when
 * it did not is worse than an error message, because the reader would have no reason to look again.
 */
export function suggestionOutcome(
  settled: PromiseSettledResult<unknown>,
  id: number,
): SuggestionOutcome {
  if (settled.status === "fulfilled") {
    return { kind: "decided", id };
  }
  return { kind: "failed", id, message: String(settled.reason) };
}

/**
 * The list without the decided item.
 *
 * The backend refuses a second decision on the same row, so leaving the item visible would offer a
 * promise the application cannot keep. The remaining items keep the order they arrived in; the order
 * the screen draws them in is a separate, pure decision ([`sortSuggestions`]), so a decision made in the
 * middle of a page does not shuffle what is left under the reader's cursor.
 */
export function withoutSuggestion(suggestions: readonly Suggestion[], id: number): Suggestion[] {
  return suggestions.filter((suggestion) => suggestion.id !== id);
}

/**
 * The orders the suggestions screen offers (S3 of `odd/tasks/suggestions-screen.md`).
 *
 * `recent` is the default and it is the reader's own decision (L2): the date the DESKTOP recorded for the
 * document, descending. `oldest` is its inverse, and `title` is the third choice Biblioteca's own selector
 * offers, kept because a queue of forty files is easier to scan by name.
 *
 * There is no author order and no author filter: a suggestion is a file the application saw open, not a
 * document it read, so it has no authors — and a control that can only empty the grid is exactly what
 * this project refuses to offer.
 */
export type SuggestionSort = "recent" | "oldest" | "title";

/** Every order the selector offers, the default first. */
export const suggestionSorts: readonly SuggestionSort[] = ["recent", "oldest", "title"];

/**
 * The form two strings are compared in: case folded, then stripped of its accents.
 *
 * The rule is the one Biblioteca's search follows (S3), which is `toLowerCase` and nothing more — this
 * module goes one step further on purpose, because a suggestion's title comes from a FILE NAME, and a
 * collection with `Anónimo.pdf` and `Analisis.pdf` is searched with the same fingers. NFD splits an
 * accented letter into the letter and its combining mark, and the mark is what gets dropped, so `on` finds
 * `ón` and `Ó` alike; `toLowerCase` then folds the case. It never invents letters: a `ñ` folds to `n` for
 * SEARCH only, and nothing is ever displayed folded.
 */
export function foldForSearch(value: string): string {
  return value
    .normalize("NFD")
    .replace(/\p{M}+/gu, "")
    .toLowerCase();
}

/**
 * Whether a suggestion answers `query` (S3): its preset title first, and the path when the title does not.
 *
 * Both are compared folded, so case and accents do not have to be remembered by the reader. An empty or
 * whitespace-only query matches everything, which is what makes clearing the box restore the whole queue.
 */
export function suggestionMatches(suggestion: Suggestion, query: string): boolean {
  const needle = foldForSearch(query.trim());
  if (needle === "") {
    return true;
  }
  return (
    foldForSearch(suggestionTitle(suggestion.path)).includes(needle) ||
    foldForSearch(suggestion.path).includes(needle)
  );
}

/** The suggestions matching `query`, in the order they arrived. */
export function filterSuggestions(
  suggestions: readonly Suggestion[],
  query: string,
): Suggestion[] {
  return suggestions.filter((suggestion) => suggestionMatches(suggestion, query));
}

/**
 * The queue in `sort` order, as a new array (S3, S4).
 *
 * **The default is the history's own date, newest first**, and it is the date the desktop recorded for the
 * document rather than the one the application wrote when it noticed (`firstSeenAt`) — the reader's choice
 * in L2, and the reason the bookmark date is a stored fact of its own.
 *
 * **An undated suggestion goes after every dated one, in both directions.** `oldest` reverses the dates
 * and deliberately does not reverse that rule: an absent date is not 1970 and not "earlier than
 * everything", so it stays where it is — last. Sorting it first would assert that the reader opened it
 * before anything else, which is precisely what the absence of a date denies.
 *
 * **Every order breaks ties on the path**, compared with the reader's locale and then by row id, so two
 * runs over one queue cannot disagree about which of two equally-named or equally-dated files comes first.
 * The input array is never reordered in place.
 */
export function sortSuggestions(
  suggestions: readonly Suggestion[],
  sort: SuggestionSort = "recent",
  locale = "en",
): Suggestion[] {
  const dated = (suggestion: Suggestion): number | null => suggestion.historyAt ?? null;

  const byPath = (a: Suggestion, b: Suggestion): number =>
    a.path.localeCompare(b.path, locale) || a.id - b.id;

  const byHistory = (a: Suggestion, b: Suggestion): number => {
    const left = dated(a);
    const right = dated(b);
    if (left === null && right === null) {
      return 0;
    }
    // The absence of a date is the one fact that does not flip with the direction.
    if (left === null) {
      return 1;
    }
    if (right === null) {
      return -1;
    }
    return sort === "oldest" ? left - right : right - left;
  };

  const byKey = (a: Suggestion, b: Suggestion): number =>
    sort === "title"
      ? suggestionTitle(a.path).localeCompare(suggestionTitle(b.path), locale)
      : byHistory(a, b);

  return [...suggestions].sort((a, b) => byKey(a, b) || byPath(a, b));
}

/**
 * The sentence for a search that emptied the grid, or `null` when no search was made (S3).
 *
 * It names the query, because a reader who typed something and sees nothing deserves to read back what
 * they typed rather than a bare "no results" — the same rule Biblioteca's no-matches sentence follows. A
 * blank query is `null` and never a sentence: an empty grid with nothing typed into the box is not a
 * failed search, and saying it was would blame the reader for something they did not do.
 */
export function describeNoSuggestionMatches(query: string, t: Translate): string | null {
  const needle = query.trim();
  return needle === "" ? null : t("suggestions.noMatches", { query: needle });
}

/**
 * The sentence for a desktop history that could not be read, or `null` when there is nothing to add.
 *
 * The read happens once per run (S10), so its failure is a fact about this run and not about the queue:
 * the list may still hold what earlier runs found, and the screen keeps showing it. Two cases say nothing:
 *
 * - **No failure.** Nothing happened, so nothing is said.
 * - **The platform has no detector at all.** "This system cannot tell which documents are open" already
 *   explains why there is no history to read, and a second sentence about the same absence would be noise
 *   dressed as detail. `NotSupported` therefore stays one sentence, and stays distinguishable from an
 *   empty queue — the failure is `null` there, and the detector belief is what speaks.
 *
 * Everything else reaches the reader: a history that exists and cannot be parsed is an error (S10), and a
 * queue that silently looks shorter than it is would be the screen claiming there is nothing to decide.
 */
export function describeHistoryFailure(
  historyError: string | null,
  detector: DetectorBelief,
  t: Translate,
): string | null {
  if (historyError === null || detector === "unsupported") {
    return null;
  }
  return t("suggestions.historyError", { message: historyError });
}

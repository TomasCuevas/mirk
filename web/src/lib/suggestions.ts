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
   * inherits that order rather than deciding one of its own.
   */
  firstSeenAt: number;
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
 * promise the application cannot keep. The remaining items keep the order they arrived in: the queue
 * is oldest first because the backend says so, and the screen does not re-sort it.
 */
export function withoutSuggestion(suggestions: readonly Suggestion[], id: number): Suggestion[] {
  return suggestions.filter((suggestion) => suggestion.id !== id);
}

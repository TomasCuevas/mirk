import { describe, expect, it } from "vitest";
import { catalogues, translate } from "./i18n";
import { paginate } from "./library";
import { libraryPageSize } from "./window";
import {
  describeHistoryFailure,
  describeNoSuggestionMatches,
  describeSuggestions,
  detectorAfterFailure,
  detectorBelief,
  detectorSettled,
  filterSuggestions,
  foldForSearch,
  sortSuggestions,
  suggestionCover,
  suggestionMatches,
  suggestionOutcome,
  suggestionSorts,
  suggestionTitle,
  withoutSuggestion,
  type Suggestion,
} from "./suggestions";

const en = translate("en");
const es = translate("es");

const suggestion = (overrides: Partial<Suggestion> = {}): Suggestion => ({
  id: 1,
  path: "~/Documentos/Cosmos - Carl Sagan.pdf",
  seenCount: 3,
  firstSeenAt: 1776000000,
  historyAt: null,
  ...overrides,
});

/**
 * What the queue's own section says (L56).
 *
 * Three facts, deliberately distinct: the answer has not arrived yet, the queue is genuinely empty,
 * and there are items to render. This project refuses to conflate the first two — an empty queue that
 * merely looks like good news is the failure the backend's own doc comment names — and the screen
 * says which one it is.
 */
describe("the suggestion queue's own state", () => {
  it("says it is still checking before the answer arrives", () => {
    expect(describeSuggestions(null, en)).toBe(en("suggestion.loading"));
  });

  it("says the queue is empty only once the answer says so", () => {
    expect(describeSuggestions([], en)).toBe(en("suggestion.none"));
    expect(describeSuggestions([], es)).not.toBe(en("suggestion.none"));
  });

  it("renders the list rather than a message when there is something to decide", () => {
    expect(describeSuggestions([suggestion()], en)).toBeNull();
    expect(describeSuggestions([suggestion()], es)).toBeNull();
  });

  it("names the machine only when the platform does not implement the detector", () => {
    expect(describeSuggestions([], en, "unsupported")).toBe(en("suggestion.noDetector"));
    expect(describeSuggestions([], es, "unsupported")).not.toBe(en("suggestion.noDetector"));
  });

  it("still shows the items it has, whatever the capability says", () => {
    expect(describeSuggestions([suggestion()], en, "unsupported")).toBeNull();
  });

  it("a temporary failure is not the permanent claim", () => {
    expect(describeSuggestions([], en, "unavailable")).toBe(en("suggestion.none"));
    expect(describeSuggestions([], en, "unknown")).toBe(en("suggestion.none"));
  });

  it("recovers: a failed probe is retried and never sticks", () => {
    let belief = detectorAfterFailure(detectorBelief(null));
    expect(belief).toBe("unknown");
    expect(detectorSettled(belief)).toBe(false);
    expect(describeSuggestions([], en, belief)).toBe(en("suggestion.none"));

    belief = detectorBelief("installed");
    expect(detectorSettled(belief)).toBe(true);
    expect(describeSuggestions([], en, belief)).toBe(en("suggestion.none"));

    expect(detectorSettled("unavailable")).toBe(false);
    expect(detectorSettled("unsupported")).toBe(true);
    expect(detectorBelief("something-new")).toBe("unknown");
    // A definitive answer is never downgraded by a later failure.
    expect(detectorAfterFailure("unsupported")).toBe("unsupported");
  });
});

/**
 * A decision removes the item, and nothing else (L56).
 *
 * The backend refuses a second decision on the same row, so offering one would be a promise the
 * application cannot keep. The item leaves the list the reader is looking at, and the items that
 * remain keep the order they arrived in — the order the screen DRAWS is `sortSuggestions`, a separate
 * decision made after this one, so deciding in the middle of a page does not shuffle what is left.
 */
describe("a decision and the list it applies to", () => {
  it("takes the decided item out and leaves the rest in the backend's order", () => {
    const list = [suggestion({ id: 1 }), suggestion({ id: 2 }), suggestion({ id: 3 })];

    expect(withoutSuggestion(list, 2).map((item) => item.id)).toEqual([1, 3]);
  });

  it("leaves a list that does not hold the id untouched", () => {
    const list = [suggestion({ id: 1 })];

    expect(withoutSuggestion(list, 99)).toEqual(list);
  });

  it("reads a recorded decision as the decision, and never as anything else", () => {
    expect(suggestionOutcome({ status: "fulfilled", value: undefined }, 4)).toEqual({
      kind: "decided",
      id: 4,
    });
  });

  /**
   * The failure path matters more than the success path: a rejection that rendered as success would
   * leave the reader believing a document joined their library when it did not.
   */
  it("carries the failure's own message instead of pretending the decision was recorded", () => {
    expect(suggestionOutcome({ status: "rejected", reason: "the file is gone" }, 4)).toEqual({
      kind: "failed",
      id: 4,
      message: "the file is gone",
    });
  });
});




/**
 * Every word of the section exists in both languages (S6).
 *
 * The register guard in \`i18n.test.ts\` reads the Spanish values, so a string written in the informal
 * voice fails there. This is the other half: a key that exists in one catalogue and not the other
 * would render as its own name in the missing language, which is a screen with no words behind it.
 */
describe("the section's words in both catalogues", () => {
  const keys = [
    "suggestion.heading",
    "suggestion.loading",
    "suggestion.openFolder",
    "suggestion.none",
    "suggestion.noDetector",
    "suggestion.acceptLabel",
    "suggestion.discardLabel",
    "suggestion.error",
  ] as const;

  it("has every key in English and in Spanish", () => {
    for (const key of keys) {
      expect(catalogues.en[key], key).toBeTruthy();
      expect(catalogues.es[key], key).toBeTruthy();
    }
  });
});

/**
 * The card's heading (L57).
 *
 * The user's verdict on the first attempt was that a suggestion has to read like the other documents,
 * which means a heading rather than a path. What this returns is the file's OWN NAME — the same
 * fallback the library already uses for a document nobody has named — and not a title read out of the
 * document, which mirk has never opened for reading. The full path stays on the card beside it.
 */
describe("the answer about the document's first page", () => {
  it("shows the page that came back", () => {
    const src = "data:image/png;base64,AAA";
    expect(suggestionCover({ status: "fulfilled", value: src })).toEqual({
      kind: "shown",
      src,
    });
  });

  it("keeps the placeholder when there was no cover", () => {
    expect(suggestionCover({ status: "fulfilled", value: null })).toEqual({
      kind: "none",
    });
  });

  it("keeps the placeholder when the call failed", () => {
    expect(
      suggestionCover({ status: "rejected", reason: new Error("no pdfium") }),
    ).toEqual({ kind: "none" });
  });

  it("does not read an empty string as a cover", () => {
    expect(suggestionCover({ status: "fulfilled", value: "" })).toEqual({
      kind: "none",
    });
  });
});

describe("the heading a suggestion's card shows", () => {
  it("uses the file's own name, without the extension", () => {
    expect(suggestionTitle("/elsewhere/Cosmos - Carl Sagan.pdf")).toBe("Cosmos - Carl Sagan");
    expect(suggestionTitle("~/Documentos/Otro.PDF")).toBe("Otro");
  });

  it("keeps the whole path when there is no file name in it", () => {
    expect(suggestionTitle("/solo/carpeta/")).toBe("/solo/carpeta/");
  });

  it("never invents a title out of a path that has none to give", () => {
    // A file whose name is only its extension has nothing left once the extension goes, so the name
    // stays as it is rather than becoming an empty heading.
    expect(suggestionTitle("/x/.pdf")).toBe("/x/.pdf");
  });
});

/**
 * S3: the search reads the preset title first and the path after it, and it is case- and accent-blind.
 *
 * Everything below is stated as what a reader does with the box: type something and see which cards
 * remain. The accent rule is the one that cannot be checked anywhere else — a queue full of Spanish file
 * names is searched by a reader whose hands do not type accents, and `Anónimo.pdf` has to answer to
 * `anonimo` or the box is a trap.
 */
describe("the search over a suggestion's title and path", () => {
  const cosmos = suggestion({ id: 1, path: "~/Documentos/Cosmos - Carl Sagan.pdf" });
  const anonimo = suggestion({ id: 2, path: "/library/Física/Anónimo.pdf" });
  const contact = suggestion({ id: 3, path: "~/Descargas/contact.pdf" });

  it("folds case and accents into one form", () => {
    expect(foldForSearch("Anónimo")).toBe("anonimo");
    expect(foldForSearch("FÍSICA")).toBe("fisica");
    expect(foldForSearch("Cosmos")).toBe(foldForSearch("COSMOS"));
  });

  it("finds a title whatever the case and the accents", () => {
    expect(suggestionMatches(anonimo, "anonimo")).toBe(true);
    expect(suggestionMatches(anonimo, "ANÓNIMO")).toBe(true);
    expect(suggestionMatches(cosmos, "cosmos")).toBe(true);
  });

  it("finds a document by its path when the title does not hold the query", () => {
    // "Descargas" is only in the path: the title of that suggestion is "contact".
    expect(suggestionMatches(contact, "descargas")).toBe(true);
    expect(suggestionMatches(contact, "fisica")).toBe(false);
    // The accented folder is reachable without typing the accent.
    expect(suggestionMatches(anonimo, "fisica")).toBe(true);
  });

  it("matches everything when the box is emptied", () => {
    // Clearing the box has to restore the whole queue, including after a search that found nothing.
    expect(suggestionMatches(contact, "")).toBe(true);
    expect(suggestionMatches(contact, "   ")).toBe(true);
  });

  it("keeps the queue's own order for the cards it keeps", () => {
    const queue = [cosmos, anonimo, contact];

    expect(filterSuggestions(queue, "o").map((item) => item.id)).toEqual([1, 2, 3]);
    expect(filterSuggestions(queue, "anonimo").map((item) => item.id)).toEqual([2]);
    expect(filterSuggestions(queue, "nothing-here")).toEqual([]);
  });
});

/**
 * S4: "the most recent" is when the READER opened it (L2), and the default order is that date descending.
 *
 * Two things are pinned beyond the happy path, because both are lies the order could tell: an undated
 * suggestion sorted as 1970 would claim the reader opened it before everything else, and a tie without a
 * tie-break would let two runs over one queue disagree.
 */
describe("the order the screen offers", () => {
  const older = suggestion({ id: 1, path: "/b.pdf", historyAt: 1_700_000_000 });
  const newer = suggestion({ id: 2, path: "/a.pdf", historyAt: 1_760_000_000 });
  const undated = suggestion({ id: 3, path: "/c.pdf", historyAt: null });

  it("offers the default first, then its inverse, then by title", () => {
    // The default is the reader's own choice (L2), so it is the first the selector shows.
    expect(suggestionSorts).toEqual(["recent", "oldest", "title"]);
  });

  it("puts the most recently opened first by default", () => {
    expect(sortSuggestions([older, undated, newer]).map((item) => item.id)).toEqual([2, 1, 3]);
  });

  it("reverses the dates for the inverse order, and leaves the undated ones last", () => {
    // The absence of a date does not flip with the direction: it is not "older than everything".
    expect(sortSuggestions([older, undated, newer], "oldest").map((item) => item.id)).toEqual([
      1, 2, 3,
    ]);
  });

  it("treats an absent date as no date at all", () => {
    // The column did not always exist, so a queue read shaped before it carries no field.
    const missing = { id: 4, path: "/d.pdf", seenCount: 1, firstSeenAt: 1 };

    expect(sortSuggestions([missing, newer]).map((item) => item.id)).toEqual([2, 4]);
  });

  it("orders by the preset title in the reader's language", () => {
    const acero = suggestion({ id: 1, path: "/z.pdf", historyAt: null });
    const anonimo = suggestion({ id: 2, path: "/Anónimo.pdf", historyAt: null });
    const borges = suggestion({ id: 3, path: "/Borges.pdf", historyAt: null });

    // "Anónimo" is a word a Spanish reader files under A, not after every ASCII name.
    expect(sortSuggestions([borges, acero, anonimo], "title", "es").map((item) => item.id)).toEqual(
      [2, 3, 1],
    );
  });

  it("breaks every tie on the path, so two runs over one queue agree", () => {
    const first = suggestion({ id: 7, path: "/same-date-b.pdf", historyAt: 1_700_000_000 });
    const second = suggestion({ id: 8, path: "/same-date-a.pdf", historyAt: 1_700_000_000 });

    const one = sortSuggestions([first, second]).map((item) => item.id);
    const two = sortSuggestions([second, first]).map((item) => item.id);

    expect(one).toEqual([8, 7]);
    expect(two).toEqual(one);
  });

  it("never reorders the queue it was given", () => {
    const queue = [older, undated, newer];

    sortSuggestions(queue, "title");

    expect(queue.map((item) => item.id)).toEqual([1, 3, 2]);
  });
});

/**
 * S2: the page is cut from the filtered, ordered queue with the SAME `paginate` Biblioteca uses, at the
 * same page size — so "a page" cannot mean one thing here and another there.
 */
describe("paging the queue", () => {
  it("cuts the ordered queue into the pages Biblioteca would cut", () => {
    const queue = Array.from({ length: 11 }, (_, index) =>
      suggestion({ id: index + 1, path: `/doc-${index + 1}.pdf`, historyAt: index }),
    );
    const size = libraryPageSize("columns-2");

    const ordered = sortSuggestions(queue);
    const second = paginate(ordered, 2, size);

    expect(size).toBe(10);
    expect(second.pageCount).toBe(2);
    expect(second.items.map((item) => item.id)).toEqual([1]);
  });
});

/** S3: a search that emptied the grid names what was typed, and a blank box is not a failed search. */
describe("the sentence for a search that found nothing", () => {
  it("names the query", () => {
    expect(describeNoSuggestionMatches("borges", en)).toBe(
      en("suggestions.noMatches", { query: "borges" }),
    );
    expect(describeNoSuggestionMatches("borges", es)).not.toBe(
      describeNoSuggestionMatches("borges", en),
    );
  });

  it("trims it, and says nothing when there was nothing to search for", () => {
    expect(describeNoSuggestionMatches("  borges  ", en)).toBe(
      en("suggestions.noMatches", { query: "borges" }),
    );
    expect(describeNoSuggestionMatches("", en)).toBeNull();
    expect(describeNoSuggestionMatches("   ", en)).toBeNull();
  });
});

/**
 * S10 and L61: a history that could not be read is said out loud, and `NotSupported` stays ONE sentence.
 *
 * The distinction the whole rule exists for: "this machine cannot tell what you opened" and "nothing is
 * waiting for a decision" must never be the same sentence, and an error must never be dressed as an empty
 * queue.
 */
describe("the sentence for a history that could not be read", () => {
  it("says nothing when the read worked", () => {
    expect(describeHistoryFailure(null, "installed", en)).toBeNull();
  });

  it("carries the failure's own message otherwise", () => {
    expect(describeHistoryFailure("permission denied", "installed", en)).toBe(
      en("suggestions.historyError", { message: "permission denied" }),
    );
    expect(describeHistoryFailure("permission denied", "installed", es)).not.toBe(
      describeHistoryFailure("permission denied", "installed", en),
    );
  });

  it("leaves the platform's own sentence alone when there is no detector", () => {
    // Two sentences for one absence would be noise: the detector already says why there is no history.
    expect(describeHistoryFailure("not supported", "unsupported", en)).toBeNull();
    // …and the queue's own sentence is then the permanent claim, never "nothing is waiting".
    expect(describeSuggestions([], en, "unsupported")).toBe(en("suggestion.noDetector"));
    expect(describeSuggestions([], en, "unsupported")).not.toBe(en("suggestion.none"));
  });

  it("still speaks when the queue is empty but the machine has a detector", () => {
    expect(describeHistoryFailure("permission denied", "installed", en)).toBeTruthy();
    expect(describeHistoryFailure("permission denied", "unknown", en)).toBeTruthy();
  });
});

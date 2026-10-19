import { describe, expect, it } from "vitest";
import { catalogues, translate } from "./i18n";
import {
  describeSuggestions,
  suggestionCover,
  suggestionOutcome,
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

  it("says the machine cannot tell, instead of saying nothing is waiting", () => {
    expect(describeSuggestions([], en, false)).toBe(en("suggestion.noDetector"));
    expect(describeSuggestions([], es, false)).not.toBe(en("suggestion.noDetector"));
  });

  it("still shows the items it has, whatever the capability says", () => {
    // A capability report is context; an item in the queue is a fact, and facts win.
    expect(describeSuggestions([suggestion()], en, false)).toBeNull();
  });

  it("keeps the queue's own words while the capability is unknown or present", () => {
    // unknown is not missing: only an answer that says the detector does not work here changes the
    // sentence. Anything else would invent a defect out of a request that has not come back.
    expect(describeSuggestions([], en, null)).toBe(en("suggestion.none"));
    expect(describeSuggestions([], en, true)).toBe(en("suggestion.none"));
  });
});

/**
 * A decision removes the item, and nothing else (L56).
 *
 * The backend refuses a second decision on the same row, so offering one would be a promise the
 * application cannot keep. The item leaves the list the reader is looking at, and the items that
 * remain keep the order the backend gave them — oldest first, never re-sorted by the screen.
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
    "suggestion.definition",
    "suggestion.loading",
    "suggestion.openFolder",
    "suggestion.none",
    "suggestion.noDetector",
    "suggestion.acceptLabel",
    "suggestion.discardLabel",
    "suggestion.error",
    "suggestion.deciding",
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
 * document, which oct has never opened for reading. The full path stays on the card beside it.
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

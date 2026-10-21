import { describe, expect, it, vi } from "vitest";
import { translate } from "./i18n";
import {
  addAuthorChip,
  ALL_AUTHORS,
  authorChoices,
  authorEditor,
  authorFilter,
  CATEGORY_REFRESH_MS,
  categoryEntries,
  coverEditor,
  coverPageFromInput,
  coverPageInput,
  coverPageText,
  coverSource,
  currentUnixSeconds,
  describeAuthors,
  describeLibraryMessage,
  describeNoMatches,
  describeProgressView,
  describeProvenance,
  describeProvider,
  describeProviderRegistry,
  describeScanSummary,
  displayTitle,
  documentActionOutcome,
  editorEntry,
  editorRefreshesLibrary,
  fetchCover,
  filterAuthors,
  filterByAuthor,
  filterEntries,
  folderActionOutcome,
  LIBRARY_REFRESH_MS,
  libraryAuthors,
  libraryView,
  pageWindow,
  paginate,
  progressColor,
  progressFraction,
  removeAuthorChip,
  sameCoverPage,
  sortEntries,
  STALE_AFTER_SECONDS,
  startCategoryRefresh,
  startLibraryRefresh,
  titleEditor,
  type CoverInvoke,
  type LibraryEntry,
  type Progress,
  type SortKey,
} from "./library";

const english = translate("en");
const spanish = translate("es");

function progress(overrides: Partial<Progress> = {}): Progress {
  return {
    page: 0,
    total: null,
    source: "native",
    confidence: "exact",
    recordedAt: null,
    readAt: null,
    ...overrides,
  };
}

function entry(overrides: Partial<LibraryEntry> = {}): LibraryEntry {
  return {
    id: 1,
    path: "/library/document.pdf",
    title: "document",
    userTitle: null,
    authors: [],
    readerAuthors: [],
    pages: null,
    progress: null,
    ...overrides,
  };
}

describe("the card's progress view", () => {
  // R7 (L22, L23). The bar stands for the fraction read and the number sits over it, at the position
  // that fraction corresponds to. A document nobody has read shows 0 % — the user's decision.
  it("draws the fraction it read, and puts the number at that position", () => {
    const view = describeProgressView(
      entry({
        progress: progress({ page: 11, total: 80, source: "evince-gvfs", confidence: "exact" }),
      }),
      english,
    );

    expect(view.fraction).toBeCloseTo(12 / 80);
    expect(view.markerAt).toBeCloseTo(12 / 80);
    expect(view.marker).toBe("15%");
  });

  it("shows 0 % for a document nobody has read, and states nothing in words", () => {
    const view = describeProgressView(entry({ progress: null }), english);

    expect(view.fraction).toBe(0);
    expect(view.markerAt).toBe(0);
    expect(view.marker).toBe("0%");
    expect(view.provenance).toBeNull();
  });

  it("never turns a known page into a percentage the store cannot support", () => {
    // A viewer reported a page but no total. The reader is somewhere in the document, so 0 % would be
    // as wrong as a made-up fraction: the bar stays empty and the marker states the page.
    const view = describeProgressView(
      entry({ progress: progress({ page: 11, total: null, source: "evince-gvfs" }) }),
      english,
    );

    expect(view.marker).toBe("p. 12");
    expect(view.marker).not.toContain("%");
    expect(view.fraction).toBe(0);
    expect(view.provenance).toBe("recorded by Evince");
  });

  it("uses one implementation of the fraction, so the card and the sort cannot disagree", () => {
    const malformed = entry({
      progress: progress({ page: 90, total: 80, source: "evince-gvfs", confidence: "exact" }),
    });

    expect(describeProgressView(malformed, english).fraction).toBe(progressFraction(malformed));
    expect(describeProgressView(malformed, english).marker).toBe("100%");
  });
});

/**
 * R39: the fill's colour travels one continuous ramp as the reading advances.
 *
 * The user first asked for red, orange, yellow and green, and then corrected the shape of the answer:
 * *"no quiero que sean colores fijos, quiero que se vayan transformando poco a poco"*. So the four are
 * landmarks on one ramp, and what this file pins is exactly that: the stops are still the colours they
 * named, and between them the colour moves in small steps rather than jumping.
 */
describe("the progress fill's colour", () => {
  /** The hue of an `rgb()` triple, in degrees — the one measure of direction that survives a ramp
   *  whose last stretch darkens instead of brightening. */
  function hue([r, g, b]: [number, number, number]): number {
    const red = r / 255;
    const green = g / 255;
    const blue = b / 255;
    const max = Math.max(red, green, blue);
    const min = Math.min(red, green, blue);
    const delta = max - min;
    if (delta === 0) {
      return 0;
    }
    const sector =
      max === red
        ? ((green - blue) / delta) % 6
        : max === green
          ? (blue - red) / delta + 2
          : (red - green) / delta + 4;
    return (sector * 60 + 360) % 360;
  }

  /** `rgb(r, g, b)` to its three channels, so a test can compare numbers instead of strings. */
  function channels(colour: string): [number, number, number] {
    const match = colour.match(/^rgb\((\d+), (\d+), (\d+)\)$/);
    expect(match, `an rgb() triple, got ${colour}`).not.toBeNull();
    const [, r, g, b] = match as RegExpMatchArray;
    return [Number(r), Number(g), Number(b)];
  }

  it("keeps the four colours the user named as the ramp's landmarks", () => {
    // The landmarks are the same red, orange, yellow and green; the values were lifted for the dark
    // canvas (L5 in odd/tasks/mirk-ui.md), because the stops chosen for a white card disappeared on
    // #07080a. Changing them back is one constant, not a redesign.
    expect(progressColor(0)).toBe("rgb(255, 90, 82)");
    expect(progressColor(0.28)).toBe("rgb(255, 138, 61)");
    expect(progressColor(0.5)).toBe("rgb(240, 180, 41)");
    expect(progressColor(0.7)).toBe("rgb(110, 231, 160)");
    // The fifth stop is the deepened green the last stretch travels towards, so the ramp does not
    // flatten exactly where the end of a long book lives.
    expect(progressColor(1)).toBe("rgb(47, 191, 113)");
  });

  it("reaches green at 70 %, which is the threshold the user set", () => {
    // The correction, with the screenshot that motivated it: at 76 % the fill was olive and read as
    // "still yellow". Green is now a landmark at 70 %, and everything above it stays green.
    const atSeventy = channels(progressColor(0.7));
    expect(atSeventy).toEqual([110, 231, 160]);

    const above = channels(progressColor(0.76));
    expect(above[1]).toBeGreaterThan(above[0] + 40);
    expect(above[1]).toBeGreaterThan(above[2] + 40);
  });

  it("stays green from 70 % to the end instead of flattening", () => {
    // Two rules in one: the hue stays green (the green channel dominates both others), and the fill
    // keeps moving — each step is darker than the one before, so 100 % is not a decorative plateau.
    let previous = channels(progressColor(0.7))[1];
    for (let step = 71; step <= 100; step += 1) {
      const [r, g, b] = channels(progressColor(step / 100));
      expect(g, `green dominates at ${step}%`).toBeGreaterThan(r);
      expect(g, `green dominates at ${step}%`).toBeGreaterThan(b);
      expect(g, `the fill keeps darkening at ${step}%`).toBeLessThan(previous);
      previous = g;
    }
  });

  it("moves the colour in small steps instead of switching between states", () => {
    // The measurement the user's correction asks for: walking the fraction in hundredths, the biggest
    // change between two neighbouring documents has to be small. With the four bands this same walk
    // produced a jump of 115 units per channel at one boundary; the measured maximum here is 11, which
    // is a shade between neighbours rather than a state change. The bound is 15 and not 11 on purpose:
    // it is the line between "a ramp" and "a set of states", not a number fitted to today's palette.
    let previous = channels(progressColor(0));
    let largest = 0;
    for (let step = 1; step <= 100; step += 1) {
      const current = channels(progressColor(step / 100));
      for (let channel = 0; channel < 3; channel += 1) {
        largest = Math.max(largest, Math.abs(current[channel] - previous[channel]));
      }
      previous = current;
    }

    expect(largest).toBeLessThan(15);
  });

  it("never travels backwards: the hue rotates one way, from red to green", () => {
    // Direction is measured in hue, not in one channel. The green CHANNEL has to fall after the 70 %
    // landmark, because the last stretch deepens the green rather than lightening it — so "the green
    // goes up" would be the wrong invariant and would forbid the deepening. What must never happen is
    // the colour turning back on itself, and hue is the measure of that: red is ~3°, green ~123°, and
    // every step in between moves it the same way.
    const hues: number[] = [];
    for (let step = 0; step <= 100; step += 1) {
      hues.push(hue(channels(progressColor(step / 100))));
    }

    // Never backwards, not "always forwards": once the ramp lands on green the hue holds steady while
    // the fill keeps darkening, and that plateau is the design, not a defect. What the rule forbids is
    // the colour turning back towards amber or red, which is what a dip would look like on a card.
    // The bound is 1 degree, not 0, and the reason is measurement, not permission. Hue here is the
    // sRGB hue of an 8-bit triple produced by interpolating in OKLab, so a segment between two greens
    // can drift back by a fraction of a degree at its midpoint — measured 0.2 deg with the dark-canvas
    // stops. That is below the precision of the output and far below what an eye resolves; a real
    // reversal (amber or red coming back) moves tens of degrees and still fails.
    for (let index = 1; index < hues.length; index += 1) {
      expect(hues[index], `hue at ${index}%`).toBeGreaterThanOrEqual(hues[index - 1] - 1);
    }
    // Ends where the user's colours say it ends: red at nothing read, green at finished.
    expect(hues[0]).toBeLessThan(10);
    expect(hues[hues.length - 1]).toBeGreaterThan(110);
  });

  it("reads against the dark track at every point", () => {
    // The track is 9 % white composited over #0c0d0f — the `--track` token — so that is the colour
    // the fill is actually seen against. On the old white card the rule was a luminance CEILING (a
    // pale yellow would vanish); on the dark canvas the same intent points the other way, so what is
    // pinned is a contrast FLOOR against the real track: 3:1 at every point of the ramp.
    const relative = (r: number, g: number, b: number): number => {
      const lin = (channel: number): number => {
        const value = channel / 255;
        return value <= 0.04045 ? value / 12.92 : Math.pow((value + 0.055) / 1.055, 2.4);
      };
      return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
    };
    const track = relative(34, 35, 37);

    for (let step = 0; step <= 100; step += 1) {
      const [r, g, b] = channels(progressColor(step / 100));
      const fill = relative(r, g, b);
      const contrast =
        (Math.max(fill, track) + 0.05) / (Math.min(fill, track) + 0.05);
      expect(contrast, `contrast at ${step}%`).toBeGreaterThanOrEqual(3);
    }
  });

  it("clamps a fraction it cannot trust, and takes the floor for one it cannot read", () => {
    expect(progressColor(1.4)).toBe(progressColor(1));
    expect(progressColor(-0.2)).toBe(progressColor(0));
    expect(progressColor(Number.NaN)).toBe(progressColor(0));
  });

  it("colours the fill from the same fraction the width comes from", () => {
    // Colour and length must not be able to disagree: one number, one rule, two channels.
    const half = entry({
      // Page 50 of 100 is the fiftieth page, and the card counts pages from one — the fraction is
      // (page + 1) / total, which is `progressFraction`'s own rule and the reason this reads 0.5 here.
      progress: progress({ page: 49, total: 100, source: "native", confidence: "exact" }),
    });
    const view = describeProgressView(half, english);

    expect(view.fraction).toBe(0.5);
    expect(view.color).toBe(progressColor(view.fraction));
  });

  it("gives a document with no record the floor colour, which draws nothing", () => {
    const view = describeProgressView(entry({ progress: null }), english);

    expect(view.color).toBe(progressColor(0));
    expect(view.fraction).toBe(0);
  });
});

/**
 * The provenance half of the sentence, and the rule that keeps it from flattening two facts into one.
 */
describe("provenance in words", () => {

  it("renders every provenance combination differently", () => {
    const rendered = [
      describeProvenance(progress({ source: "native", confidence: "exact" }), english),
      describeProvenance(progress({ source: "manual", confidence: "manual" }), english),
      describeProvenance(progress({ source: "evince-gvfs", confidence: "exact" }), english),
      describeProvenance(progress({ source: "evince-gvfs", confidence: "close-time" }), english),
      describeProvenance(progress({ source: "evince-gvfs", confidence: "unknown" }), english),
    ];
    expect(new Set(rendered).size).toBe(rendered.length);
  });

  it("names the known sources and falls back to the raw id for unknown ones", () => {
    expect(describeProvenance(progress({ source: "native" }), english)).toContain("Mirk");
    expect(
      describeProvenance(progress({ source: "manual", confidence: "manual" }), english),
    ).toContain("you");
    expect(describeProvenance(progress({ source: "evince-gvfs" }), english)).toContain("Evince");
    expect(describeProvenance(progress({ source: "zathura-history" }), english)).toContain(
      "zathura-history",
    );
  });
});

describe("library states", () => {
  it("says the library is empty rather than rendering nothing", () => {
    expect(describeLibraryMessage([], english)).toBe(
      "The library is empty. Scan a folder to add documents.",
    );
  });

  it("has no message once the library holds something", () => {
    expect(describeLibraryMessage([entry()], english)).toBeNull();
  });
});

describe("what a listing screen shows", () => {
  // L47. The user removed a watched folder and watched it again; for the seconds the rescan took, the
  // listing was empty and the screen rendered its EMPTY state — the first-run note that says no folder
  // is watched. Nothing was missing; the work was in progress. The rule: an empty listing may only be
  // presented AS empty once nothing is in flight.
  it("shows the loader, not the empty state, while a reload is in flight", () => {
    expect(libraryView([], true)).toBe("loading");
  });

  it("shows the empty state only once nothing is in flight", () => {
    expect(libraryView([], false)).toBe("empty");
  });

  it("never covers content with a loader, even while something is in flight", () => {
    // The fifteen-second re-read and a scan of a second folder both run while documents are already on
    // screen. Content wins over the loader, so the listing does not flicker or flash empty.
    expect(libraryView([entry()], true)).toBe("content");
    expect(libraryView([entry()], false)).toBe("content");
  });
});

describe("provider states", () => {
  it("names the missing capability and the reason", () => {
    const sentence = describeProvider(
      { id: "evince-gvfs", capability: "live", availability: "missing" },
      english,
    );
    expect(sentence).toContain("Evince");
    expect(sentence).toContain("missing");
    expect(sentence).toContain("gio");
  });

  it("describes an installed provider with its capability", () => {
    const sentence = describeProvider(
      { id: "evince-gvfs", capability: "live", availability: "installed" },
      english,
    );
    expect(sentence).toContain("Evince");
    expect(sentence).toContain("live");
  });

  it("says clearly when the platform has no providers at all", () => {
    // A registry that has not answered yet is not an empty registry: claiming there are no providers
    // before the answer arrives asserts something the screen does not know.
    expect(describeProviderRegistry(null, english)).toBe(
      "Checking which progress providers are available…",
    );
    expect(describeProviderRegistry([], english)).toBe(
      "No progress providers are available on this platform.",
    );
  });
});

describe("scan summary", () => {
  it("reports what happened, including partial failures", () => {
    const sentence = describeScanSummary(
      {
        root: "/library",
        discovered: ["/library/a.pdf", "/library/b.pdf"],
        counted: [{ path: "/library/a.pdf", pages: 12, title: null, author: null }],
        skipped: [],
        failures: [{ path: "/library/b.pdf", reason: "cannot parse the PDF" }],
      },
      english,
    );
    expect(sentence).toContain("Counted 1 of 2");
    expect(sentence).toContain("/library");
  });

  it("says when a folder held no PDFs", () => {
    const sentence = describeScanSummary(
      { root: "/library", discovered: [], counted: [], skipped: [], failures: [] },
      english,
    );
    expect(sentence).toContain("No PDFs");
  });
});

describe("i18n", () => {
  it("provides Spanish as a real alternative to English", () => {
    expect(spanish("library.heading")).not.toBe(english("library.heading"));
    expect(
      describeProgressView(
        entry({ progress: progress({ page: 11, total: 80, source: "evince-gvfs" }) }),
        spanish,
      ).provenance,
    ).not.toBe(
      describeProgressView(
        entry({ progress: progress({ page: 11, total: 80, source: "evince-gvfs" }) }),
        english,
      ).provenance,
    );
  });
});

describe("the title editor's rule (R31)", () => {
  it("prefills with the title on screen, whoever it belongs to", () => {
    expect(titleEditor({ ...entry(), title: "The document's own", userTitle: null })).toEqual({
      value: "The document's own",
      yours: false,
    });
    expect(titleEditor({ ...entry(), title: "Mine", userTitle: "Mine" })).toEqual({
      value: "Mine",
      yours: true,
    });
  });

  it("offers to restore the document's own title exactly when the reader set a real one", () => {
    // The rule the unit hangs on: offering to restore something that was never set is an action without
    // a meaning. After the note left the card, this control and the field's label are the two places the
    // provenance is stated at all — so the rule is pinned here, where a test can reach it, instead of
    // living inside a modal nobody can render without clicking.
    expect(titleEditor({ ...entry(), title: "A", userTitle: null }).yours).toBe(false);
    expect(titleEditor({ ...entry(), title: "A", userTitle: "A" }).yours).toBe(true);
    // Blank is not a title, for the same reason it is not one in `Document::display_title`.
    expect(titleEditor({ ...entry(), title: "A", userTitle: "   " }).yours).toBe(false);
  });
});

/**
 * The author editor's rule.
 *
 * Its twin is the title editor, with one deliberate difference: an EMPTY chip list is legitimate —
 * "no author of my own" — and falls back to the document's own author, while the title refuses a
 * blank because a document must have a title.
 *
 * Since R42 the stored fact is an ORDERED LIST, so the editor is prefilled with a LIST of chips and
 * never with one name: the reader's own list when they have one (S3: their order is the order), and
 * otherwise the authors the card shows — which, for a document nobody has edited, is the embedded
 * metadata string as ONE entry, the honest starting point rather than a split that would invent two
 * people (S6). `yours` follows `readerAuthors`, so the label can state whose list is on screen.
 */
describe("the author editor's rule", () => {
  it("prefills with the whole author list, in the order it was given", () => {
    expect(
      authorEditor({
        ...entry(),
        authors: ["Carl Sagan", "Ann Druyan"],
        readerAuthors: ["Carl Sagan", "Ann Druyan"],
      }),
    ).toEqual({ chips: ["Carl Sagan", "Ann Druyan"], yours: true });

    // S3: the other order is a different list and equally legitimate. Nothing re-sorts it.
    expect(
      authorEditor({
        ...entry(),
        authors: ["Ann Druyan", "Carl Sagan"],
        readerAuthors: ["Ann Druyan", "Carl Sagan"],
      }).chips,
    ).toEqual(["Ann Druyan", "Carl Sagan"]);
  });

  it("falls back to the displayed authors when the reader has none of their own", () => {
    expect(
      authorEditor({ ...entry(), authors: ["The document's own"], readerAuthors: [] }),
    ).toEqual({ chips: ["The document's own"], yours: false });
  });

  it("takes an embedded author string as ONE entry, never split into people (S6)", () => {
    // "Sagan, Carl; Druyan, Ann" is one string one document's metadata happened to hold. Splitting it
    // would invent two authors nobody wrote, so it starts as one chip the reader can replace.
    expect(
      authorEditor({
        ...entry(),
        authors: ["Sagan, Carl; Druyan, Ann"],
        readerAuthors: [],
      }).chips,
    ).toEqual(["Sagan, Carl; Druyan, Ann"]);
  });

  it("prefills with no chip when neither the document nor the reader has an author", () => {
    // An empty list is the plain state for a document without an author; it is not an error, and
    // saving it unchanged keeps meaning "no author of my own".
    expect(authorEditor(entry({ authors: [], readerAuthors: [] }))).toEqual({
      chips: [],
      yours: false,
    });
  });

  it("treats a blank reader's author as no author, for the same reason a blank title is no title", () => {
    expect(authorEditor({ ...entry(), authors: ["A"], readerAuthors: ["   "] })).toEqual({
      chips: ["A"],
      yours: false,
    });
  });
});

/**
 * The chips' two rules, living in `library.ts` because a screen must not decide them (R42/S2–S3, U30).
 *
 * Adding is where the reader's order is protected: a name is trimmed before it becomes a chip, a
 * blank is refused rather than stored as "", a duplicate is refused so the existing chip keeps both
 * its spelling and its place, and a new name lands at the END. Removing takes out exactly the chip it
 * names and leaves every other chip in the order the reader chose.
 */
describe("the author chips' rules", () => {
  it("trims a name before it becomes a chip and appends it at the end", () => {
    const first = addAuthorChip([], "  Carl Sagan  ");
    expect(first).toEqual({ chips: ["Carl Sagan"], added: true });

    const second = addAuthorChip(first.chips, "Ann Druyan");
    expect(second).toEqual({ chips: ["Carl Sagan", "Ann Druyan"], added: true });
  });

  it("refuses a blank, and says so, so the field can keep what was typed", () => {
    expect(addAuthorChip(["Carl Sagan"], "   ")).toEqual({
      chips: ["Carl Sagan"],
      added: false,
    });
    expect(addAuthorChip([], "")).toEqual({ chips: [], added: false });
  });

  it("refuses a duplicate compared trimmed and case-insensitively, keeping the existing chip in place", () => {
    const chips = ["Carl Sagan", "Ann Druyan"];

    // Nothing is reordered and the first spelling wins: the reader's order is never reshuffled by the
    // act of typing a name the list already holds.
    expect(addAuthorChip(chips, "  carl sagan ")).toEqual({
      chips: ["Carl Sagan", "Ann Druyan"],
      added: false,
    });
    expect(addAuthorChip(chips, "ANN DRUYAN")).toEqual({
      chips: ["Carl Sagan", "Ann Druyan"],
      added: false,
    });
  });

  it("still refuses a duplicate when the existing chip carries surrounding spaces", () => {
    expect(addAuthorChip([" Carl Sagan "], "carl sagan")).toEqual({
      chips: [" Carl Sagan "],
      added: false,
    });
  });

  it("removes exactly the chip it names and keeps the rest in order", () => {
    const chips = ["Carl Sagan", "Ann Druyan", "Neil deGrasse Tyson"];

    expect(removeAuthorChip(chips, "Ann Druyan")).toEqual([
      "Carl Sagan",
      "Neil deGrasse Tyson",
    ]);
    expect(removeAuthorChip(chips, "Carl Sagan")).toEqual([
      "Ann Druyan",
      "Neil deGrasse Tyson",
    ]);
  });

  it("compares a removal the same way it compares an add", () => {
    // The trimmed, case-insensitive comparison is ONE rule, used by both halves: a chip that reads
    // "Carl Sagan" is removable by the name the reader typed in another case.
    expect(removeAuthorChip(["Carl Sagan", "Ann Druyan"], "  carl sagan ")).toEqual([
      "Ann Druyan",
    ]);
  });

  it("leaves the list untouched when the name is not on it", () => {
    expect(removeAuthorChip(["Carl Sagan"], "Nobody")).toEqual(["Carl Sagan"]);
    expect(removeAuthorChip([], "Nobody")).toEqual([]);
  });
});

/**
 * The authors the editor offers (R40).
 *
 * Derived from the listing the interface already holds — both the authors the documents carry and the
 * ones readers set — so somebody does not retype a name and end up with two spellings of one person.
 * Two entries whose authors differ only by surrounding spaces are one person, so the trimmed value is
 * what makes them unique. The list is sorted for the reader's locale (S3), because it is a list a
 * person reads.
 */
describe("the author suggestions", () => {
  it("offers every author the library already holds, without duplicates", () => {
    const entries = [
      entry({ id: 1, authors: ["Ursula K. Le Guin"], readerAuthors: [] }),
      entry({ id: 2, authors: ["Jorge Luis Borges"], readerAuthors: [] }),
      entry({ id: 3, authors: ["Ursula K. Le Guin"], readerAuthors: ["Ursula K. Le Guin"] }),
    ];

    expect(libraryAuthors(entries)).toEqual(["Jorge Luis Borges", "Ursula K. Le Guin"]);
  });

  // S4: one rule per list, and they are DIFFERENT rules. The editor's list is a spelling suggestion,
  // so it offers every author the reader has ever used on a document — including the reader's own,
  // which no card shows, because the card shows the resolved author. The library's filter offers only
  // what the cards show, because there a choice must have a document behind it.
  it("keeps the reader's own author that no card shows, which is where it differs from the filter", () => {
    const entries = [
      entry({ id: 1, authors: ["Ursula K. Le Guin"], readerAuthors: ["Le Guin"] }),
      entry({ id: 2, authors: [], readerAuthors: ["Only The Reader's"] }),
    ];

    expect(libraryAuthors(entries)).toEqual(["Le Guin", "Only The Reader's", "Ursula K. Le Guin"]);
    expect(filterAuthors(entries)).toEqual(["Ursula K. Le Guin"]);
  });

  // S3. The editor's list reads alphabetically to the PERSON, so it is sorted with their locale rather
  // than in first-seen order. The second assertion is the same two names in code-unit order — "Á" is
  // U+00C1, so a plain sort puts the accented name after every ASCII one, which is not where a Spanish
  // reader looks for it.
  it("orders the list the way the reader's locale reads it, not by code unit", () => {
    const entries = [entry({ id: 1, authors: ["Borges"] }), entry({ id: 2, authors: ["Ávila"] })];

    expect(libraryAuthors(entries, "es")).toEqual(["Ávila", "Borges"]);
    expect(["Borges", "Ávila"].sort()).toEqual(["Borges", "Ávila"]);
  });

  it("does not offer the same author twice for spellings that differ only by surrounding spaces", () => {
    const entries = [
      entry({ id: 1, authors: ["  Jorge Luis Borges  "], readerAuthors: [] }),
      entry({ id: 2, authors: ["Jorge Luis Borges"], readerAuthors: ["Jorge Luis Borges"] }),
      entry({ id: 3, authors: [], readerAuthors: [" Jorge Luis Borges "] }),
    ];

    expect(libraryAuthors(entries)).toEqual(["Jorge Luis Borges"]);
  });

  it("offers nothing when no document in the library has an author", () => {
    expect(libraryAuthors([entry({ authors: [], readerAuthors: [] }), entry({ id: 2 })])).toEqual(
      [],
    );
  });
});

describe("the authors the selector offers (R39)", () => {
  // S2. The list is the authors the CARDS show: `entry.authors`, which the core already resolved to
  // the reader's authors when they set any and the document's own otherwise. Offering `readerAuthors`
  // beside it — which is what the editor's suggestions do, for a different purpose — would list a
  // name no card on the screen stands for, and the reader would pick it and see an empty shelf.
  it("offers the author each card shows, never the reader's own field", () => {
    const entries = [
      entry({ id: 1, title: "A", authors: ["Ursula K. Le Guin"], readerAuthors: ["Le Guin"] }),
      entry({ id: 2, title: "B", authors: [], readerAuthors: ["Only The Reader's"] }),
    ];

    expect(filterAuthors(entries)).toEqual(["Ursula K. Le Guin"]);
  });

  // Uniqueness, trimming and blanks are one rule: an author is one person whatever the surrounding
  // spaces say, and a blank is not an author at all. The list the reader chooses from must not offer
  // the same person twice, or offer an unnamed entry they cannot recognise.
  it("offers each author once, trimmed, and never a blank", () => {
    const entries = [
      entry({ id: 1, authors: ["  Jorge Luis Borges  "] }),
      entry({ id: 2, authors: ["Jorge Luis Borges"] }),
      entry({ id: 3, authors: [] }),
      entry({ id: 4, authors: ["   "] }),
    ];

    expect(filterAuthors(entries)).toEqual(["Jorge Luis Borges"]);
    expect(filterAuthors([entry({ authors: [] }), entry({ id: 2, authors: [" "] })])).toEqual([]);
  });

  // S2's ordering clause: the list reads alphabetically to the PERSON, so it is sorted with their
  // locale rather than by code unit. The second assertion is the same two names in code-unit order —
  // "Á" is U+00C1, so a plain sort puts the accented name after every ASCII one, which is not where
  // a Spanish reader looks for it.
  it("orders the list the way the reader's locale reads it, not by code unit", () => {
    const entries = [entry({ id: 1, authors: ["Borges"] }), entry({ id: 2, authors: ["Ávila"] })];

    expect(filterAuthors(entries, "es")).toEqual(["Ávila", "Borges"]);
    expect(["Borges", "Ávila"].sort()).toEqual(["Borges", "Ávila"]);
  });
});

describe("filtering the listing by author (R39)", () => {
  const entries = [
    entry({ id: 1, title: "Cosmos", authors: ["Carl Sagan"] }),
    entry({ id: 2, title: "Contact", authors: ["Carl Sagan"] }),
    entry({ id: 3, title: "Ficciones", authors: ["Jorge Luis Borges"] }),
    entry({ id: 4, title: "Anónimo", authors: [] }),
  ];

  // S1: the selector starts on "all", so "all" has to mean the whole listing — the same thing a
  // blank query means to the text box. A document with no author at all belongs to it, because there
  // is no other list it could belong to.
  it("returns the whole listing for 'all', which is where the selector starts", () => {
    expect(filterByAuthor(entries, ALL_AUTHORS)).toEqual(entries);
    expect(filterByAuthor(entries, "")).toEqual(entries);
  });

  it("narrows to the documents whose card shows that author", () => {
    expect(filterByAuthor(entries, "Jorge Luis Borges").map((e) => e.title)).toEqual([
      "Ficciones",
    ]);
    expect(filterByAuthor(entries, "Carl Sagan").map((e) => e.title)).toEqual([
      "Cosmos",
      "Contact",
    ]);
  });

  // The list's values are trimmed, so the filter has to compare trimmed values too: otherwise a
  // document whose stored author carries spaces would be missing from the very list that offered it.
  it("matches the trimmed name, so a stored value with spaces still answers to it", () => {
    const spaced = [entry({ id: 9, authors: ["  Jorge Luis Borges  "] })];

    expect(filterByAuthor(spaced, "Jorge Luis Borges").map((e) => e.id)).toEqual([9]);
  });

  // S3: text first, then author, then order — ONE pipeline over the whole listing, so no two controls
  // can disagree about what is on screen. Sorting after filtering is what keeps a narrowed listing in
  // the order the reader chose instead of in the order the rows happened to arrive.
  it("composes with the text filter and the sort, in that order", () => {
    const visible = sortEntries(
      filterByAuthor(filterEntries(entries, "co"), "Carl Sagan"),
      "title",
    );

    expect(visible.map((e) => e.title)).toEqual(["Contact", "Cosmos"]);
    // The two filters intersect rather than overriding each other.
    expect(filterByAuthor(filterEntries(entries, "ficciones"), "Carl Sagan")).toEqual([]);
  });

  // S3's paging clause. The reader is three pages into the library, narrowing to an author with two
  // documents leaves one page, and the window has to come back with it — otherwise the screen shows an
  // empty page and claims the reader is somewhere the listing no longer reaches.
  it("clamps the page when the narrowing shrinks the listing under the reader", () => {
    const docs = [
      ...Array.from({ length: 22 }, (_, index) =>
        entry({ id: index + 1, title: `Sagan ${index}`, authors: ["Carl Sagan"] }),
      ),
      entry({ id: 90, title: "Ficciones", authors: ["Jorge Luis Borges"] }),
      entry({ id: 91, title: "El Aleph", authors: ["Jorge Luis Borges"] }),
    ];
    const whole = paginate(sortEntries(docs, "title"), 3);
    expect(whole.page).toBe(3);

    const narrowed = paginate(sortEntries(filterByAuthor(docs, "Jorge Luis Borges"), "title"), 3);

    expect(narrowed.page).toBe(1);
    expect(narrowed.items.map((e) => e.title)).toEqual(["El Aleph", "Ficciones"]);
  });
});

/**
 * R42/S4: the displayed authors are stated the way a person reads a list. The connector is the
 * catalogue's, so Spanish says "y" where English says "and" — a hardcoded one would be a second
 * language inside the logic.
 */
describe("stating a list of authors (R42/S4)", () => {
  it("says one name as itself, in both languages", () => {
    expect(describeAuthors(["Carl Sagan"], english)).toBe("Carl Sagan");
    expect(describeAuthors(["Carl Sagan"], spanish)).toBe("Carl Sagan");
  });

  it("joins two names with the catalogue's connector, never a hardcoded one", () => {
    expect(describeAuthors(["Carl Sagan", "Ann Druyan"], english)).toBe("Carl Sagan and Ann Druyan");
    expect(describeAuthors(["Carl Sagan", "Ann Druyan"], spanish)).toBe("Carl Sagan y Ann Druyan");
  });

  it("joins three or more with commas and one connector before the last", () => {
    expect(describeAuthors(["A", "B", "C"], english)).toBe("A, B and C");
    expect(describeAuthors(["A", "B", "C", "D"], english)).toBe("A, B, C and D");
    expect(describeAuthors(["A", "B", "C"], spanish)).toBe("A, B y C");
  });

  it("says nothing when nobody wrote it, and never invents a name from a blank", () => {
    expect(describeAuthors([], english)).toBeNull();
    expect(describeAuthors(["   "], spanish)).toBeNull();
  });

  it("states each name trimmed, because surrounding spaces are never part of a name", () => {
    expect(describeAuthors(["  Carl Sagan  ", "Ann Druyan"], english)).toBe(
      "Carl Sagan and Ann Druyan",
    );
  });
});

/**
 * R42/S7: a document can be written by several people, and the filter and the search read the
 * displayed list — the one the core resolved — so a co-author is as findable as the first name.
 */
describe("co-authors in the selector and the search (R42/S7)", () => {
  const coauthored = [
    entry({ id: 1, title: "Cosmos", authors: ["Carl Sagan", "Ann Druyan"] }),
    entry({ id: 2, title: "Contact", authors: ["Carl Sagan"] }),
    entry({ id: 3, title: "Ficciones", authors: ["Jorge Luis Borges"] }),
  ];

  it("offers every displayed author of a co-authored document", () => {
    expect(filterAuthors(coauthored)).toEqual(["Ann Druyan", "Carl Sagan", "Jorge Luis Borges"]);
  });

  it("narrows to the same document through either co-author", () => {
    expect(filterByAuthor(coauthored, "Ann Druyan").map((e) => e.title)).toEqual(["Cosmos"]);
    expect(filterByAuthor(coauthored, "Carl Sagan").map((e) => e.title)).toEqual([
      "Cosmos",
      "Contact",
    ]);
  });

  it("matches a co-author in the free-text search", () => {
    expect(filterEntries(coauthored, "druyan").map((e) => e.title)).toEqual(["Cosmos"]);
  });
});

describe("a chosen author who has left the library (R39)", () => {
  // S4. An edit or a rescan can remove the only document by the author the reader had chosen. The
  // choice then falls back to "all" and the screen says nothing about it: the shelf changed, which is
  // not an error. What must never happen is the alternative — a stale choice keeping the library
  // empty, which looks like a defect and is caused by something the reader cannot see.
  const authors = ["Carl Sagan", "Jorge Luis Borges"];

  it("keeps a choice the listing still names, and keeps 'all' as 'all'", () => {
    expect(authorFilter(authors, "Jorge Luis Borges")).toBe("Jorge Luis Borges");
    expect(authorFilter(authors, ALL_AUTHORS)).toBe(ALL_AUTHORS);
  });

  it("falls back to 'all' when the listing no longer names it", () => {
    expect(authorFilter(["Carl Sagan"], "Jorge Luis Borges")).toBe(ALL_AUTHORS);
    expect(authorFilter([], "Carl Sagan")).toBe(ALL_AUTHORS);
  });

  it("does it silently: the whole library comes back and no sentence mentions a filter", () => {
    const entries = [
      entry({ id: 1, title: "Cosmos", authors: ["Carl Sagan"] }),
      entry({ id: 2, title: "Ficciones", authors: ["Jorge Luis Borges"] }),
    ];
    const chosen = authorFilter(["Carl Sagan"], "Jorge Luis Borges");

    expect(filterByAuthor(entries, chosen)).toEqual(entries);
    expect(describeNoMatches("", chosen, english)).toBeNull();
  });
});

describe("the 'nothing matches' sentence (R39)", () => {
  // S5. With an author and no text it says no document by that author; with text it keeps the sentence
  // that names the query. The shape a careless wiring produces — `No document matches ""` — is a
  // sentence nobody wrote, and it tells the reader nothing about what they filtered by.
  it("names the author when an author is what was filtered", () => {
    const sentence = describeNoMatches("", "Jorge Luis Borges", english);

    expect(sentence).toBe(english("library.noMatchesAuthor", { author: "Jorge Luis Borges" }));
    expect(sentence).toContain("Jorge Luis Borges");
  });

  it("keeps naming the query when there is text, even with an author chosen", () => {
    expect(describeNoMatches("cosmos", "Jorge Luis Borges", english)).toBe(
      english("library.noMatches", { query: "cosmos" }),
    );
  });

  it("says nothing at all when nothing was filtered, instead of naming an empty query", () => {
    expect(describeNoMatches("", ALL_AUTHORS, english)).toBeNull();
    expect(describeNoMatches("   ", ALL_AUTHORS, english)).toBeNull();
  });

  it("says it in Spanish too", () => {
    expect(describeNoMatches("", "Jorge Luis Borges", spanish)).toBe(
      "Ningún documento de Jorge Luis Borges.",
    );
  });
});

describe("card titles", () => {
  // R31. The core resolves the displayed title before the interface sees it, so `displayTitle` reads
  // the value it was given rather than deciding anything itself — the fallback to the file's own name
  // is `mirk-core`'s `Document::display_title`, where it has its own tests.
  it("shows the title the core already resolved", () => {
    expect(displayTitle(entry({ title: "Cosmos" }))).toBe("Cosmos");
  });

  it("never falls back to the path", () => {
    const card = displayTitle(entry({ path: "/home/u/libros/Cosmos - Carl Sagan.pdf", title: "Cosmos" }));
    expect(card).toBe("Cosmos");
    expect(card).not.toContain("/");
  });

  it("carries the reader's title as a separate fact from the displayed one", () => {
    // The point of R31: the interface can tell the reader's title apart from the document's own.
    const named = entry({ title: "What the reader called it", userTitle: "What the reader called it" });
    expect(displayTitle(named)).toBe("What the reader called it");
    expect(named.userTitle).toBe("What the reader called it");

    const unnamed = entry({ title: "The document's own title", userTitle: null });
    expect(displayTitle(unnamed)).toBe("The document's own title");
    expect(unnamed.userTitle).toBeNull();
  });
});

describe("filtering the listing", () => {
  const entries = [
    entry({ id: 1, path: "/library/Cosmos.pdf", title: "Cosmos" }),
    entry({ id: 2, path: "/library/Deep Work.pdf", title: "Deep Work" }),
    entry({ id: 3, path: "/library/Programación/SICP.pdf", title: "Structures" }),
  ];

  it("returns the whole listing for an empty query", () => {
    expect(filterEntries(entries, "")).toEqual(entries);
    expect(filterEntries(entries, "   ")).toEqual(entries);
  });

  it("matches a title without caring about case", () => {
    expect(filterEntries(entries, "cosmos").map((e) => e.id)).toEqual([1]);
    expect(filterEntries(entries, "COSMOS").map((e) => e.id)).toEqual([1]);
  });

  it("matches the file's own name when the document has no embedded title", () => {
    expect(filterEntries(entries, "deep work").map((e) => e.id)).toEqual([2]);
  });

  it("matches anywhere in the path, including a directory", () => {
    expect(filterEntries(entries, "programación").map((e) => e.id)).toEqual([3]);
  });

  it("returns nothing when nothing matches", () => {
    expect(filterEntries(entries, "nothing like this")).toEqual([]);
  });
});

describe("sorting the listing", () => {
  function withProgress(page: number, total: number | null): Progress {
    return progress({ page, total, source: "evince-gvfs", confidence: "exact" });
  }

  const entries = [
    entry({ id: 3, title: "Charlie", pages: 10, progress: withProgress(0, 10) }),
    entry({ id: 1, title: "Alpha", pages: 300, progress: null }),
    entry({ id: 2, title: "Bravo", pages: null, progress: withProgress(4, 5) }),
  ];

  it("sorts by title", () => {
    expect(sortEntries(entries, "title").map((e) => e.title)).toEqual([
      "Alpha",
      "Bravo",
      "Charlie",
    ]);
  });

  it("sorts by page count and puts an unknown count last", () => {
    // An absent page count is not zero pages; it must not be sorted as if it were.
    expect(sortEntries(entries, "pages").map((e) => e.id)).toEqual([3, 1, 2]);
  });

  it("sorts by reading progress, most complete first, with unread last", () => {
    // Bravo is finished (5 of 5), Charlie has read page 1 of 10, and Alpha has no recorded
    // position at all — which is not the same fact as 0%.
    expect(sortEntries(entries, "progress").map((e) => e.id)).toEqual([2, 3, 1]);
  });

  it("treats a position without a total as unknown, and sorts it last", () => {
    const unknownTotal = entry({ id: 9, title: "Zulu", progress: withProgress(3, null) });
    const known = entry({ id: 8, title: "Yankee", progress: withProgress(1, 4) });
    expect(sortEntries([unknownTotal, known], "progress").map((e) => e.id)).toEqual([8, 9]);
  });

  it("breaks ties on the title and then on the id, so the order is stable", () => {
    const ties = [
      entry({ id: 5, title: "Same", pages: 100 }),
      entry({ id: 2, title: "Same", pages: 100 }),
      entry({ id: 9, title: "Aaa", pages: 100 }),
    ];
    expect(sortEntries(ties, "pages").map((e) => e.id)).toEqual([9, 2, 5]);
  });

  it("never mutates the listing it was given", () => {
    const original = [...entries];
    sortEntries(entries, "title");
    expect(entries).toEqual(original);
  });

  it("offers every sort the screen can choose from", () => {
    const keys: SortKey[] = ["title", "pages", "progress"];
    for (const key of keys) {
      expect(sortEntries(entries, key)).toHaveLength(entries.length);
    }
  });

  it("computes a fraction that agrees with the sentence the card shows", () => {
    // The card says "p. 12 of 80" for page 11, so 12 pages of 80 have been read.
    expect(progressFraction(entry({ progress: withProgress(11, 80) }))).toBeCloseTo(12 / 80);
    expect(progressFraction(entry({ progress: null }))).toBeNull();
    expect(progressFraction(entry({ progress: withProgress(3, null) }))).toBeNull();
  });
});

describe("progress fraction", () => {
  it("clamps a position beyond its total, agreeing with the domain", () => {
    // `StoredProgress::fraction()` clamps in Rust. A stale total reported by a viewer must mean
    // "finished" on both sides, not a fraction above one that outranks a completed document.
    const malformed = entry({ progress: progress({ page: 90, total: 80 }) });
    expect(progressFraction(malformed)).toBe(1);
  });

  it("is null without a position or without a total, never a guess", () => {
    expect(progressFraction(entry({ progress: null }))).toBeNull();
    expect(progressFraction(entry({ progress: progress({ page: 3, total: null }) }))).toBeNull();
    expect(progressFraction(entry({ progress: progress({ page: 0, total: 0 }) }))).toBeNull();
  });
});

describe("title fallback", () => {
  it("is the core's job, so the interface keeps a document's own title when it is the displayed one", () => {
    // The defect this replaced: the card rendered the absolute path where a title belongs, for the
    // 12 of the maintainer's 42 documents whose PDFs carry no embedded title. The core now returns
    // the file's own name as the displayed title (see `Document::display_title`); here we only pin
    // that the interface shows exactly what it was handed, path or not.
    const resolved = "gentleman-programming-book-es";
    expect(displayTitle(entry({ title: resolved, path: "/home/u/Programación/x.pdf" }))).toBe(
      resolved,
    );
    expect(displayTitle(entry({ title: resolved }))).not.toContain("/");
  });
});

describe("the home categories", () => {
  // R4 / B3 (L20, L22). The three carousels on Inicio are readings of the stored position, so the
  // selection is a pure function here and neither SQL nor markup decides it. "A long time" is measured
  // from when a source that OBSERVED the reading says it happened — `progress.read_at`, which the
  // interface receives as `Progress.readAt` (R30) — and never from when the row was written, which is when
  // the app looked. `documents.last_opened_at` is deliberately absent: it exists in the schema but nothing
  // writes it, so it is not a field the interface can even see.
  const now = 1_700_000_000;

  function recorded(
    page: number,
    total: number | null,
    readAt: number | null = now,
  ): Progress {
    // `recordedAt` is deliberately `now` — the row was written a moment ago — while `readAt` carries the
    // fact under test. Every category test therefore fails if a rule goes back to reading when the APP
    // looked instead of when the source says the person read: exactly the defect `R30` removed.
    return progress({
      page,
      total,
      source: "evince-gvfs",
      confidence: "exact",
      recordedAt: now,
      readAt,
    });
  }

  it("counts a recorded position that is not at the end as in progress", () => {
    const reading = entry({ id: 1, progress: recorded(11, 80) });
    const lastPage = entry({ id: 2, progress: recorded(79, 80) });

    expect(categoryEntries([reading, lastPage], "inProgress", now).map((e) => e.id)).toEqual([1]);
  });

  it("counts a recorded position at the end as finished", () => {
    const reading = entry({ id: 1, progress: recorded(11, 80) });
    const done = entry({ id: 2, progress: recorded(79, 80) });

    expect(categoryEntries([reading, done], "finished", now).map((e) => e.id)).toEqual([2]);
  });

  it("counts a stale record from when it was READ, and has nothing to measure without that", () => {
    const stale = entry({ id: 1, progress: recorded(11, 80, now - STALE_AFTER_SECONDS) });
    const fresh = entry({ id: 2, progress: recorded(11, 80, now - 60) });
    // A document with no reading record at all carries no timestamp, so it cannot be "stale" any
    // more than it can be "in progress": it is not measured here rather than guessed at.
    const never = entry({ id: 3, progress: null });

    expect(categoryEntries([stale, fresh, never], "stale", now).map((e) => e.id)).toEqual([1]);
  });

  it("pulls the two times apart, in both directions", () => {
    // R30. A row written a year ago but READ a minute ago is not stale: the person read it. A row written
    // a minute ago but read a year ago is. The launch sweep writes a whole library within a second of
    // starting, so the row's own time is when the application looked, which is not evidence about the
    // reader — and this test fails in one direction or the other if a rule ever goes back to reading it.
    const readRecently = entry({
      id: 1,
      progress: progress({
        page: 3,
        total: 80,
        source: "evince-gvfs",
        confidence: "exact",
        recordedAt: now - 365 * 86_400,
        readAt: now - 60,
      }),
    });
    const readLongAgo = entry({
      id: 2,
      progress: progress({
        page: 3,
        total: 80,
        source: "evince-gvfs",
        confidence: "exact",
        recordedAt: now - 60,
        readAt: now - STALE_AFTER_SECONDS,
      }),
    });

    expect(categoryEntries([readRecently], "stale", now)).toEqual([]);
    expect(categoryEntries([readLongAgo], "stale", now).map((e) => e.id)).toEqual([2]);
    // Both are in progress by position, which is a different question from age, and the carousel orders
    // them by when they were read.
    expect(categoryEntries([readRecently, readLongAgo], "inProgress", now).map((e) => e.id)).toEqual([
      1, 2,
    ]);
  });

  it("leaves a document whose reading time no source ever recorded out of the stale list", () => {
    // The honest half of R30: the application cannot say somebody forgot a document when no source ever
    // said when they read it. It is not stale, and the sentence on screen says so rather than letting the
    // carousel look empty for no reason.
    const neverRecorded = entry({
      id: 1,
      progress: progress({
        page: 3,
        total: 80,
        source: "evince-gvfs",
        confidence: "exact",
        recordedAt: now - 5 * 365 * 86_400,
        readAt: null,
      }),
    });

    expect(categoryEntries([neverRecorded], "stale", now)).toEqual([]);
  });

  it("re-evaluates as the clock advances, which is what an open screen's tick drives", () => {
    // The screens no longer capture the instant at mount; they re-read it. This is the rule that
    // re-read makes true: the same record and the same list, a different instant, a different
    // category. One second apart is enough because the comparison is `>=`, and without the re-read
    // the tick would change nothing.
    const justInside = entry({
      id: 1,
      progress: recorded(11, 80, now - STALE_AFTER_SECONDS + 1),
    });

    expect(categoryEntries([justInside], "stale", now)).toEqual([]);
    expect(categoryEntries([justInside], "stale", now + 1).map((e) => e.id)).toEqual([1]);
  });
});

describe("the clock an open screen re-reads", () => {
  it("reads Unix seconds, the unit recordedAt and the threshold share", () => {
    // If this helper ever returned milliseconds, every record would look ancient against a seconds
    // threshold, so the unit is the thing this pins.
    const before = Math.floor(Date.now() / 1000);
    const value = currentUnixSeconds();
    const after = Math.floor(Date.now() / 1000);

    expect(Number.isInteger(value)).toBe(true);
    expect(value).toBeGreaterThanOrEqual(before);
    expect(value).toBeLessThanOrEqual(after);
  });

  it("ticks on the refresh interval and stops when the screen goes away", () => {
    // The interval is the whole point: an open screen must re-evaluate without a remount, and it
    // must not keep a timer after it unmounts. Fake timers make both observable with no DOM.
    vi.useFakeTimers();
    try {
      let ticks = 0;
      const stop = startCategoryRefresh(() => {
        ticks += 1;
      });

      vi.advanceTimersByTime(CATEGORY_REFRESH_MS - 1);
      expect(ticks).toBe(0);
      vi.advanceTimersByTime(1);
      expect(ticks).toBe(1);

      stop();
      vi.advanceTimersByTime(CATEGORY_REFRESH_MS * 3);
      expect(ticks).toBe(1);
    } finally {
      vi.useRealTimers();
    }
  });

  it("keeps the tick cheap to run but short enough to notice", () => {
    // The threshold is measured in days: a sub-second tick would recompute for no visible gain, and
    // a multi-minute one would let a document sit in the wrong carousel long enough to be seen.
    expect(CATEGORY_REFRESH_MS).toBeGreaterThanOrEqual(1000);
    expect(CATEGORY_REFRESH_MS).toBeLessThanOrEqual(5 * 60 * 1000);
  });
});

describe("paginating the listing", () => {
  // R10 (L22). Ten per page is the user's number, and the controls behind the page have to say where
  // the reader is and how many documents exist.
  const many = Array.from({ length: 23 }, (_, index) =>
    entry({ id: index + 1, title: `Doc ${index + 1}` }),
  );

  it("cuts the listing into pages of ten, and reports where the reader is", () => {
    const first = paginate(many, 1);

    expect(first.items.map((e) => e.id)).toEqual([1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
    expect(first.page).toBe(1);
    expect(first.pageCount).toBe(3);
    expect(first.total).toBe(many.length);
  });

  it("cuts the filtered, sorted listing, so turning the page keeps the reader's filters", () => {
    // Filtering and sorting run over the whole listing and only then is the result cut into pages: a
    // page is a window onto what the reader filtered, not a slice of the raw rows. The clamp is what
    // keeps a filter that shrinks the listing from stranding the reader on an empty page.
    const docs = Array.from({ length: 25 }, (_, index) =>
      entry({ id: index + 1, title: index % 2 === 0 ? `Even ${index}` : `Odd ${index}` }),
    );
    const visible = sortEntries(filterEntries(docs, "Even"), "title");
    const first = paginate(visible, 1);
    const second = paginate(visible, 2);

    expect(first.items.every((item) => displayTitle(item).includes("Even"))).toBe(true);
    expect(second.items.every((item) => displayTitle(item).includes("Even"))).toBe(true);
    expect([...first.items, ...second.items]).toEqual(visible);
    expect(paginate(visible, 9).page).toBe(second.page);
  });

  it("reports one page for an empty listing, so the controls never say 'page 1 of 0'", () => {
    const empty = paginate([], 1);

    expect(empty.items).toEqual([]);
    expect(empty.page).toBe(1);
    expect(empty.pageCount).toBe(1);
    expect(empty.total).toBe(0);
  });
});

describe("re-reading the listing", () => {
  /**
   * #46. The listing changes without the interface being touched: the tracker writes a position every few
   * seconds, so a document gains its first record or reaches its last page while the reader is looking at
   * Inicio. Re-reading only at mount made the screen look dead for as long as the application worked.
   */
  it("re-reads on its own clock and stops when the application goes away", () => {
    vi.useFakeTimers();
    try {
      let reads = 0;
      const stop = startLibraryRefresh(() => {
        reads += 1;
      });

      vi.advanceTimersByTime(LIBRARY_REFRESH_MS - 1);
      expect(reads).toBe(0);
      vi.advanceTimersByTime(1);
      expect(reads).toBe(1);
      vi.advanceTimersByTime(LIBRARY_REFRESH_MS);
      expect(reads).toBe(2);

      // A timer that outlives the application would keep reading a store nobody is looking at.
      stop();
      vi.advanceTimersByTime(LIBRARY_REFRESH_MS * 3);
      expect(reads).toBe(2);
    } finally {
      vi.useRealTimers();
    }
  });

  it("re-reads fast enough to look alive and slowly enough to be free", () => {
    // The tracker writes at most once per few seconds, so a tick faster than that reads for facts that
    // cannot have changed; a tick measured in minutes would let a document that just finished sit in the
    // wrong carousel long enough to be seen. Five seconds and a minute are the bounds of that argument.
    expect(LIBRARY_REFRESH_MS).toBeGreaterThanOrEqual(5_000);
    expect(LIBRARY_REFRESH_MS).toBeLessThanOrEqual(60_000);
  });

  it("keeps its own number rather than reusing the category clock's", () => {
    // One number serving both would be wrong for one of them: the category rule is measured in days and
    // the listing changes in seconds. Pinning the two apart is what stops a later tidy-up from merging
    // them into the slower one and quietly bringing the defect back.
    expect(LIBRARY_REFRESH_MS).not.toBe(CATEGORY_REFRESH_MS);
  });
});

describe("the order of the carousels", () => {
  /**
   * R29 (L45). The reader asked for it in their own words: the last document they read belongs first, and
   * the tiles run left to right from the most recent to the oldest. The order lives in the selector rather
   * than in the screen, so the carousel, a category's own list and the ten-tile cap read one order.
   */
  const now = 1_700_000_000;
  const read = (id: number, readAt: number | null, title: string = "document"): LibraryEntry =>
    entry({
      id,
      title,
      progress: progress({
        page: 3,
        total: 80,
        source: "evince-gvfs",
        confidence: "exact",
        // The order is by when the document was READ. The write time is set to the opposite end of the
        // scale on purpose, so a rule that sorted by when the app wrote the row would order this the
        // other way round and fail.
        readAt,
        recordedAt: readAt === null ? null : 1_600_000_000,
      }),
    });

  it("puts the most recently read document first", () => {
    const oldest = read(1, now - 1000);
    const newest = read(2, now - 10);
    const middle = read(3, now - 100);

    expect(categoryEntries([oldest, newest, middle], "inProgress", now).map((e) => e.id)).toEqual([
      2, 3, 1,
    ]);
  });

  it("keeps every document whose reading time no source recorded after every document that has one", () => {
    // R30. "No source said" is not 1970 and it is not the newest thing on the shelf: it is the absence of
    // an answer, and its honest place is the tail. The comparator used to send it to `0`, which is the
    // START of the scale — so a real time below the epoch sorted *behind* a document nobody knows anything
    // about. Negative infinity cannot do that: every finite time sorts ahead of it, and the ones with no
    // time tie only with each other.
    const preEpoch = read(1, -86_400, "Aaa primera");
    const unknown = read(2, null, "Zzz ultima");
    const unknownToo = read(3, null, "Aaa tambien desconocida");
    const known = read(4, now - 10, "Mmm conocida");

    expect(
      categoryEntries([unknown, unknownToo, preEpoch, known], "inProgress", now).map((e) => e.id),
    ).toEqual([4, 1, 3, 2]);
  });

  it("orders every category by its own records, not only the one that was looked at", () => {
    // One implementation for three carousels is the point; a rule applied in only one branch would leave
    // the other two reading as if nobody had touched them.
    const finishedOlder = entry({
      id: 1,
      progress: progress({
        page: 79,
        total: 80,
        source: "evince-gvfs",
        confidence: "exact",
        // The order is by when each was READ, so that is what these carry; the row's own write time is
        // the decoy again, set in the opposite order.
        recordedAt: now - 10,
        readAt: now - 1000,
      }),
    });
    const finishedNewer = entry({
      id: 2,
      progress: progress({
        page: 79,
        total: 80,
        source: "evince-gvfs",
        confidence: "exact",
        recordedAt: now - 1000,
        readAt: now - 10,
      }),
    });
    const stale = entry({
      id: 3,
      progress: progress({
        page: 3,
        total: 80,
        source: "evince-gvfs",
        confidence: "exact",
        recordedAt: now - 60,
        readAt: now - STALE_AFTER_SECONDS - 500,
      }),
    });
    const staler = entry({
      id: 4,
      progress: progress({
        page: 3,
        total: 80,
        source: "evince-gvfs",
        confidence: "exact",
        recordedAt: now - 60,
        readAt: now - STALE_AFTER_SECONDS - 90_000,
      }),
    });

    expect(
      categoryEntries([finishedOlder, finishedNewer], "finished", now).map((e) => e.id),
    ).toEqual([2, 1]);
    expect(categoryEntries([staler, stale], "stale", now).map((e) => e.id)).toEqual([3, 4]);
  });

  it("keeps a stable order when a whole sweep lands in the same second", () => {
    // The launch sweep records every known document within a second or two of starting, so equal
    // timestamps are the common case rather than a corner. The fallback is the name the reader is shown,
    // and it has to be a function of the data alone: this listing is re-read every fifteen seconds, and an
    // order that could drift would move the tiles under the reader's hand while they are looking at them.
    const zebra = read(1, now - 5, "Zebra");
    const apple = read(2, now - 5, "apple");

    expect(categoryEntries([zebra, apple], "inProgress", now).map((e) => e.id)).toEqual([2, 1]);
    // The same set in the other order must not produce a different carousel.
    expect(categoryEntries([apple, zebra], "inProgress", now).map((e) => e.id)).toEqual([2, 1]);
  });

  it("sorts a record with no timestamp last, where it claims nothing", () => {
    // A position whose own time was never stored cannot claim to be the newest. Unknown recency belongs at
    // the far end of the list rather than at the front, where it would be asserting something false.
    const unknown = read(1, null);
    const known = read(2, now - 100_000);

    expect(categoryEntries([unknown, known], "inProgress", now).map((e) => e.id)).toEqual([2, 1]);
  });
});

/**
 * R32: a card's cover is asked for on demand, and "there is no cover" is a placeholder rather than
 * an error. The command already turns an unreadable document into `null`; the interface's job is to
 * keep that a placeholder and to make an empty `src` impossible.
 */
describe("a document's cover", () => {
  it("asks the application for the cover of the document's own row", async () => {
    const invoke = vi.fn<CoverInvoke>(async () => "data:image/png;base64,AAAA");

    await expect(fetchCover(entry({ id: 7 }), invoke)).resolves.toBe(
      "data:image/png;base64,AAAA",
    );
    // The row id, not the path: the cover belongs to the document the store knows.
    expect(invoke).toHaveBeenCalledWith("document_cover", { id: 7 });
  });

  it("keeps the placeholder when the application has no cover", async () => {
    const invoke = vi.fn<CoverInvoke>(async () => null);

    await expect(fetchCover(entry(), invoke)).resolves.toBeNull();
  });

  it("keeps the placeholder rather than reporting an error when the request fails", async () => {
    const invoke = vi.fn<CoverInvoke>(async () => {
      throw new Error("pdfium is missing");
    });

    await expect(fetchCover(entry(), invoke)).resolves.toBeNull();
  });

  it("treats anything that is not a non-empty string as no cover", () => {
    // The single rule that makes an empty `src` impossible: a browser renders `src=""` as a request
    // for the page itself, and `src="null"` as a broken image.
    expect(coverSource("data:image/png;base64,AAAA")).toBe("data:image/png;base64,AAAA");
    expect(coverSource(null)).toBeNull();
    expect(coverSource(undefined)).toBeNull();
    expect(coverSource("")).toBeNull();
    expect(coverSource(42)).toBeNull();
  });
});

/**
 * The containing-folder action's outcome, decided by a pure rule.
 *
 * The card no longer renders the result of "open containing folder": the user removed both the
 * success and the failure sentences (L27, L30), which left the outcome plumbing with no consumer. The
 * action itself remains, and it still either opened the folder or did not — a decision, and one whose
 * failure has to reach the console where the application reports failures that have no user state to
 * carry them. The rule lives here, with no webview and no DOM, so both branches are pinned even though
 * no screen draws them.
 */
describe("the containing-folder action's outcome", () => {
  it("names the folder and the viewer when the command opened the folder", () => {
    expect(
      folderActionOutcome({
        status: "fulfilled",
        value: { folder: "/library/documents", viewer: "Files" },
      }),
    ).toEqual({ kind: "opened", folder: "/library/documents", viewer: "Files" });
  });

  it("reports a failure rather than inventing an opened folder", () => {
    expect(
      folderActionOutcome({ status: "rejected", reason: new Error("no such folder") }),
    ).toEqual({ kind: "failed", message: "Error: no such folder" });
  });
});

/**
 * The open-document action's outcome, decided by a pure rule (R37).
 *
 * The same shape as the folder action's, and for the same reason: the card renders no outcome, so
 * the decision the command's answer forces is kept in a value both branches can be pinned on without
 * a webview. The page is part of it, because "opened" and "opened where I left it" are different
 * promises and only the second one is a promise about the page.
 */
describe("the open-document action's outcome", () => {
  it("names the document, the viewer and the page when the command opened it", () => {
    expect(
      documentActionOutcome({
        status: "fulfilled",
        value: { path: "/library/cosmos.pdf", viewer: "evince", page: 11 },
      }),
    ).toEqual({
      kind: "opened",
      path: "/library/cosmos.pdf",
      viewer: "evince",
      page: 11,
    });
  });

  it("keeps a document opened at its beginning distinct from one resumed at a page", () => {
    // `null` is "no source ever said where this document was", not "page zero": a document opened
    // at its beginning and a document nobody has read are the same answer, and both are different
    // from a document resumed at page 12.
    expect(
      documentActionOutcome({
        status: "fulfilled",
        value: { path: "/library/cosmos.pdf", viewer: "xdg-open", page: null },
      }),
    ).toEqual({
      kind: "opened",
      path: "/library/cosmos.pdf",
      viewer: "xdg-open",
      page: null,
    });
  });

  it("reports a failure rather than inventing an opened document", () => {
    expect(
      documentActionOutcome({ status: "rejected", reason: new Error("the file is gone") }),
    ).toEqual({ kind: "failed", message: "Error: the file is gone" });
  });
});

/**
 * R32: the reader can choose a page of their own or supply an image, and the editor has to speak the
 * store's own language while showing a person one-based numbers.
 *
 * The conversion and the "is there an image to remove" decision are pure functions rather than
 * conditions in the modal's markup, for the same reason `titleEditor` is: a rule nothing checks drifts,
 * and a test cannot click a modal.
 */
describe("the cover editor", () => {
  it("shows the first page when the reader has not chosen one", () => {
    // `null` means the default, and the default is page one — not page zero, which is not a number a
    // person reads.
    expect(coverPageInput(entry({ coverPage: null }))).toBe("1");
    expect(coverEditor(entry({ coverPage: null })).page).toBe("1");
  });

  it("shows the chosen page one-based, the way a person counts pages", () => {
    // The store keeps zero-based pages; a chosen page 12 is shown as 13.
    expect(coverPageInput(entry({ coverPage: 12 }))).toBe("13");
  });

  it("turns what the reader typed back into a zero-based page or the default", () => {
    expect(coverPageFromInput("13")).toEqual({ ok: true, page: 12 });
    expect(coverPageFromInput("1")).toEqual({ ok: true, page: 0 });
    // A number input can hand back a number, and the same field can hand back an emptied value as
    // `""`, `null` or `undefined`; all of them work and all of them mean the default.
    expect(coverPageFromInput(13)).toEqual({ ok: true, page: 12 });
    // Empty is the plain default: the first page, stored as `null`, the value the one rule treats as
    // the first page rather than as a choice the reader made.
    expect(coverPageFromInput("   ")).toEqual({ ok: true, page: null });
    expect(coverPageFromInput("")).toEqual({ ok: true, page: null });
    expect(coverPageFromInput(null)).toEqual({ ok: true, page: null });
    expect(coverPageFromInput(undefined)).toEqual({ ok: true, page: null });
  });

  it("refuses a page that is not a positive whole number rather than inventing one", () => {
    for (const refused of ["abc", "0", "-3", "1.5", "12px"]) {
      expect(coverPageFromInput(refused), refused).toEqual({ ok: false });
    }
  });

  it("offers to remove the image only when the reader actually supplied one", () => {
    expect(coverEditor(entry({ coverImage: null })).hasImage).toBe(false);
    expect(coverEditor(entry({ coverImage: "12-cover.png" })).hasImage).toBe(true);
    // A blank value is not an image, the same way a blank title is not a title: nothing to remove.
    expect(coverEditor(entry({ coverImage: "   " })).hasImage).toBe(false);
  });
});

/**
 * R18: the editor does not close itself.
 *
 * The user's request was explicit — *"cuando se haga clic en Guardar no tendría que sacar
 * automáticamente el formulario"* — so the rule that used to close it after a title save is gone
 * rather than inverted: `settleEditorAfter` only announces the change. What replaces the closed
 * editor is the baseline the buttons are compared against, which is pinned in `BookGrid.dom.test.ts`
 * by pressing Save and watching the pair disappear while the dialog stays.
 */

/**
 * R32, corrected: the editor's facts come from the listing the interface now holds, not from the
 * snapshot it was opened with, and a completed cover change refreshes that listing.
 *
 * The two defects the user hit are one rule stated twice: the listing was not re-read after a cover
 * action (so the chosen page came back empty), and the entry the editor showed was the snapshot taken
 * before the change (so the reader's own title appeared to be lost). The dialogue cannot be clicked, so
 * both halves are pinned here as the state the editor must derive.
 */
describe("the document editor and the listing it reads from", () => {
  const opened = entry({
    title: "Microsoft Word - Completo.rtf",
    userTitle: null,
    coverPage: null,
    coverImage: null,
  });

  const current = entry({
    title: "Padre Rico, Padre Pobre",
    userTitle: "Padre Rico, Padre Pobre",
    coverPage: 4,
    coverImage: "1-cover.png",
  });

  it("still carries the reader's title after a cover change", () => {
    // The document's own title is a separate fact a cover action never touched; a refresh that showed
    // it in place of the reader's is exactly the title "reverting" on screen after an image.
    const shown = editorEntry([current], opened);
    expect(displayTitle(shown)).toBe("Padre Rico, Padre Pobre");
    expect(titleEditor(shown)).toEqual({ value: "Padre Rico, Padre Pobre", yours: true });
  });

  it("carries the chosen page back, so reopening is not empty", () => {
    // The page field is one-based; the stored 4 is shown as 5.
    expect(coverEditor(editorEntry([current], opened)).page).toBe("5");
  });

  it("keeps the opened entry when the listing no longer names the document", () => {
    // A refresh cannot be allowed to close an editor over a document the reader is using.
    expect(editorEntry([entry({ id: 99 })], opened)).toBe(opened);
  });

  it("refreshes the listing after a cover change, exactly as after a title change", () => {
    // Without this the listing stayed stale, so the entry a reopened editor seeds from never learned
    // the chosen page — the defect the user reported as the page not being loaded.
    expect(editorRefreshesLibrary("cover")).toBe(true);
    expect(editorRefreshesLibrary("title")).toBe(true);
  });
});

/**
 * R15. The page strip the library draws: first, last, and a window around where the reader is.
 *
 * The reader's words were the familiar shape — *"<- 1 2 3 ... 5 ->"* — so what is pinned here is the
 * shape and the two rules that keep it honest: the ends are always reachable, and a gap is drawn only
 * when more than one page is missing (an ellipsis standing in for one number is longer than the
 * number).
 */
describe("the pages a paginator shows", () => {
  it("lists a small listing whole, without an ellipsis", () => {
    expect(pageWindow(1, 5)).toEqual([
      { kind: "page", page: 1 },
      { kind: "page", page: 2 },
      { kind: "page", page: 3 },
      { kind: "page", page: 4 },
      { kind: "page", page: 5 },
    ]);
    expect(pageWindow(1, 7)).toHaveLength(7);
  });

  it("always keeps the first and last page reachable", () => {
    expect(pageWindow(1, 20)).toEqual([
      { kind: "page", page: 1 },
      { kind: "page", page: 2 },
      { kind: "gap" },
      { kind: "page", page: 20 },
    ]);

    expect(pageWindow(20, 20)).toEqual([
      { kind: "page", page: 1 },
      { kind: "gap" },
      { kind: "page", page: 19 },
      { kind: "page", page: 20 },
    ]);
  });

  it("puts the current page in a window with its neighbours", () => {
    expect(pageWindow(10, 20)).toEqual([
      { kind: "page", page: 1 },
      { kind: "gap" },
      { kind: "page", page: 9 },
      { kind: "page", page: 10 },
      { kind: "page", page: 11 },
      { kind: "gap" },
      { kind: "page", page: 20 },
    ]);
  });

  it("clamps a page it cannot trust into the listing", () => {
    expect(pageWindow(0, 20)[0]).toEqual({ kind: "page", page: 1 });
    expect(pageWindow(99, 20).at(-1)).toEqual({ kind: "page", page: 20 });
    expect(pageWindow(1, 1)).toEqual([{ kind: "page", page: 1 }]);
    // A page count of zero cannot exist (an empty listing still has one page), and inventing pages
    // for it would be a claim about a library that is not there.
    expect(pageWindow(1, 0)).toEqual([{ kind: "page", page: 1 }]);
  });

  it("never emits two gaps or a gap at either end", () => {
    for (const page of [1, 2, 5, 10, 19, 20]) {
      const slots = pageWindow(page, 20);
      expect(slots[0]).toEqual({ kind: "page", page: 1 });
      expect(slots.at(-1)).toEqual({ kind: "page", page: 20 });
      for (let index = 1; index < slots.length; index += 1) {
        expect(slots[index].kind === "gap" && slots[index - 1].kind === "gap").toBe(false);
      }
    }
  });
});

describe("the finished state of a card", () => {
  it("marks a document as complete only when the fraction reached the end", () => {
    // The fraction comes from the recorded position against the recorded total, not from the
    // document's own page count: a stale count must not decide that a book is finished.
    expect(
      describeProgressView(entry({ pages: 100, progress: progress({ page: 99, total: 100 }) }), english)
        .complete,
    ).toBe(true);
    expect(
      describeProgressView(entry({ pages: 100, progress: progress({ page: 50, total: 100 }) }), english)
        .complete,
    ).toBe(false);
    expect(describeProgressView(entry({ progress: null }), english).complete).toBe(false);
    // A page with no known total has no fraction, so it cannot claim to be finished.
    expect(describeProgressView(entry({ pages: null, progress: progress({ page: 3 }) }), english).complete)
      .toBe(false);
  });
});


/**
 * R16. The author field is a combobox: one text input that also offers the names the library knows.
 *
 * The decision of WHAT to offer is pinned here, away from the field that draws it — the same split the
 * project uses for every rule a screen obeys.
 */
describe("the names the author field offers", () => {
  const library = ["Ann Druyan", "Carl Sagan", "Jorge Luis Borges", "Roberto Arlt"];

  it("offers every name when nothing has been typed", () => {
    expect(authorChoices(library, "")).toEqual(library);
  });

  it("narrows as the reader types, ignoring case and surrounding space", () => {
    // "ar" is inside "Carlo"? No — inside "Carl" and "Arlt"; matching is a substring, not a prefix,
    // which is what makes typing a surname work.
    expect(authorChoices(library, "ar")).toEqual(["Carl Sagan", "Roberto Arlt"]);
    expect(authorChoices(library, "  BORGES ")).toEqual(["Jorge Luis Borges"]);
  });

  it("never offers a name that is already on the list", () => {
    expect(authorChoices(library, "", ["Carl Sagan"])).toEqual([
      "Ann Druyan",
      "Jorge Luis Borges",
      "Roberto Arlt",
    ]);
  });

  it("keeps the caller's order, because the screen already sorted it in the reader's locale", () => {
    // Sorting here with the default locale would silently undo the Spanish ordering the screen asked
    // for ("Ávila" before "Borges").
    const ordered = ["Ávila", "Borges", "Cortázar"];
    expect(authorChoices(ordered, "")).toEqual(ordered);
  });

  it("drops blanks and duplicates rather than offering an empty row", () => {
    expect(authorChoices(["", "  ", "Borges", "Borges"], "")).toEqual(["Borges"]);
  });

  it("offers nothing when nothing matches", () => {
    expect(authorChoices(library, "zzz")).toEqual([]);
  });
});


/**
 * R19. Having an image and showing it are two facts.
 *
 * The card's cover comes from the stored source, and a library written before the source existed
 * falls back to the old rule — an image if there is one — so an upgrade never changes what a reader
 * sees.
 */
describe("the cover's source", () => {
  it("reports the stored source, and falls back to the old rule when it is absent", () => {
    expect(coverEditor(entry({ coverImage: "a.png", coverSource: "page" })).source).toBe("page");
    expect(coverEditor(entry({ coverImage: "a.png", coverSource: "image" })).source).toBe("image");
    // A pre-V10 payload: an image means image, no image means page.
    expect(coverEditor(entry({ coverImage: "a.png" })).source).toBe("image");
    expect(coverEditor(entry({ coverImage: null })).source).toBe("page");
    expect(coverEditor(entry({ coverImage: null, coverSource: null })).source).toBe("page");
  });

  it("keeps the page field filled even when the image is the cover", () => {
    // Switching back to the page must not require retyping a number the store already holds.
    const entry_ = entry({ coverPage: 4, coverImage: "a.png", coverSource: "image" });
    expect(coverEditor(entry_).page).toBe("5");
    expect(coverEditor(entry_).hasImage).toBe(true);
    expect(coverEditor(entry_).source).toBe("image");
  });
});


/**
 * R20. "The same page" has three ways to look different: the field is one-based, the store is
 * zero-based, and a number input can hand back a number where the last save stored a string.
 *
 * The user's report was the button staying on screen after pressing it. What is pinned here is the
 * rule that makes it disappear, and the corner cases around it: canonical spelling, blank meaning the
 * first page, and an invalid draft that must still look like a change.
 */
describe("comparing cover pages", () => {
  it("writes the canonical one-based text for a stored page", () => {
    expect(coverPageText(null)).toBe("");
    expect(coverPageText(0)).toBe("1");
    expect(coverPageText(4)).toBe("5");
  });

  it("treats the same page written differently as the same page", () => {
    expect(sameCoverPage("3", "3")).toBe(true);
    // The number a `<input type="number">` hands back.
    expect(sameCoverPage(3, "3")).toBe(true);
    // Leading zeros are not a different page.
    expect(sameCoverPage("03", "3")).toBe(true);
    // A blank field and an explicit first page are the same cover: `null` and `0` both show page 1.
    expect(sameCoverPage("", "")).toBe(true);
    expect(sameCoverPage("", "1")).toBe(true);
    expect(sameCoverPage("1", "")).toBe(true);
    expect(sameCoverPage(null, "1")).toBe(true);
  });

  it("treats a different page as a change", () => {
    expect(sameCoverPage("3", "2")).toBe(false);
    expect(sameCoverPage("", "2")).toBe(false);
    expect(sameCoverPage("10", "1")).toBe(false);
  });

  it("treats an invalid draft as a change, but not as a phantom one", () => {
    // "0" and "abc" are refused by `coverPageFromInput`; the form shows the refusal. What must not
    // happen is a refusal looking like "no change", or a draft identical to the saved text looking
    // like one.
    expect(sameCoverPage("0", "3")).toBe(false);
    expect(sameCoverPage("abc", "3")).toBe(false);
    expect(sameCoverPage("abc", "abc")).toBe(true);
  });
});

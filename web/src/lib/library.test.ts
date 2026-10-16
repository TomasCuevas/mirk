import { describe, expect, it, vi } from "vitest";
import { translate } from "./i18n";
import {
  CATEGORY_REFRESH_MS,
  LIBRARY_REFRESH_MS,
  STALE_AFTER_SECONDS,
  categoryEntries,
  currentUnixSeconds,
  describeProgressView,
  describeProvenance,
  describeLibraryMessage,
  describeProvider,
  describeProviderRegistry,
  describeScanSummary,
  displayTitle,
  filterEntries,
  paginate,
  progressFraction,
  sortEntries,
  startCategoryRefresh,
  startLibraryRefresh,
  type LibraryEntry,
  type Progress,
  type SortKey,
  titleEditor,
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
    author: null,
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
    expect(describeProvenance(progress({ source: "native" }), english)).toContain("oct");
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

describe("card titles", () => {
  // R31. The core resolves the displayed title before the interface sees it, so `displayTitle` reads
  // the value it was given rather than deciding anything itself — the fallback to the file's own name
  // is `oct-core`'s `Document::display_title`, where it has its own tests.
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

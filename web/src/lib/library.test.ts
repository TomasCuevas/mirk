import { describe, expect, it } from "vitest";
import { translate } from "./i18n";
import {
  STALE_AFTER_SECONDS,
  categoryEntries,
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
  titleFromPath,
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
    ...overrides,
  };
}

function entry(overrides: Partial<LibraryEntry> = {}): LibraryEntry {
  return {
    id: 1,
    path: "/library/document.pdf",
    title: null,
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

describe("card titles", () => {
  it("prefers the document's own title when it has one", () => {
    expect(displayTitle(entry({ title: "Cosmos" }))).toBe("Cosmos");
  });

  it("falls back to the file's own name, never the path", () => {
    const card = displayTitle(
      entry({ path: "/home/u/libros/Cosmos - Carl Sagan.pdf", title: null }),
    );
    expect(card).toBe("Cosmos - Carl Sagan");
    expect(card).not.toContain("/");
  });

  it("treats a blank embedded title as no title at all", () => {
    // PDF metadata can hold an empty title. Rendering a blank card would be as unhelpful as
    // rendering a path, so the file's own name is used instead.
    expect(displayTitle(entry({ path: "/u/report.pdf", title: "   " }))).toBe("report");
  });
});

describe("filtering the listing", () => {
  const entries = [
    entry({ id: 1, path: "/library/Cosmos.pdf", title: "Cosmos" }),
    entry({ id: 2, path: "/library/Deep Work.pdf", title: null }),
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
  it("shows the file's own name, never the path", () => {
    expect(titleFromPath("/home/u/libros/Cosmos - Carl Sagan.pdf")).toBe("Cosmos - Carl Sagan");
    expect(titleFromPath("/home/u/Comparación de Métodos.pdf")).toBe("Comparación de Métodos");
    // A path with a directory that also contains dots must not be truncated at the wrong dot.
    expect(titleFromPath("/home/u/libros.v2/report.pdf")).toBe("report");
    expect(titleFromPath("/home/u/libros.v2/report.v2.pdf")).toBe("report.v2");
  });

  it("keeps a name that has no extension, and a leading dot", () => {
    expect(titleFromPath("/home/u/no-extension")).toBe("no-extension");
    expect(titleFromPath("/home/u/.hidden")).toBe(".hidden");
    expect(titleFromPath("report.pdf")).toBe("report");
  });

  it("is what the card shows when the document has no title of its own", () => {
    // The defect this replaced: the card rendered the absolute path where a title belongs, for the
    // 12 of the maintainer's 42 documents whose PDFs carry no embedded title.
    const path = "/home/u/libros/Programación/gentleman-programming-book-es.pdf";
    expect(titleFromPath(path)).toBe("gentleman-programming-book-es");
    expect(titleFromPath(path)).not.toContain("/");
  });
});

describe("the home categories", () => {
  // R4 / B3 (L20, L22). The three carousels on Inicio are readings of the stored position, so the
  // selection is a pure function here and neither SQL nor markup decides it. "A long time" is
  // measured from `progress.updated_at`, which the UI receives as `Progress.recordedAt`.
  // `documents.last_opened_at` is deliberately absent from this file: it exists in the schema but
  // nothing writes it, so it is not a field the interface can even see.
  const now = 1_700_000_000;

  function recorded(
    page: number,
    total: number | null,
    recordedAt: number | null = now,
  ): Progress {
    return progress({ page, total, source: "evince-gvfs", confidence: "exact", recordedAt });
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

  it("counts a stale record from the record's own timestamp, and has nothing to measure without one", () => {
    const stale = entry({ id: 1, progress: recorded(11, 80, now - STALE_AFTER_SECONDS) });
    const fresh = entry({ id: 2, progress: recorded(11, 80, now - 60) });
    // A document with no reading record at all carries no timestamp, so it cannot be "stale" any
    // more than it can be "in progress": it is not measured here rather than guessed at.
    const never = entry({ id: 3, progress: null });

    expect(categoryEntries([stale, fresh, never], "stale", now).map((e) => e.id)).toEqual([1]);
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

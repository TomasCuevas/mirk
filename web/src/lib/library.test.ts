import { describe, expect, it } from "vitest";
import { translate } from "./i18n";
import {
  describeEntryProgress,
  describeLibraryMessage,
  describeProvider,
  describeProviderRegistry,
  describeScanSummary,
  titleFromPath,
  type LibraryEntry,
  type Progress,
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

describe("progress sentences", () => {
  it("states the position with its total and its provenance", () => {
    expect(
      describeEntryProgress(
        progress({ page: 11, total: 80, source: "evince-gvfs", confidence: "exact" }),
        english,
      ),
    ).toBe("p. 12 of 80, recorded by Evince");
  });

  it("never invents a total or a percentage when the total is unknown", () => {
    const sentence = describeEntryProgress(
      progress({ page: 11, total: null, source: "evince-gvfs", confidence: "exact" }),
      english,
    );
    expect(sentence).toBe("p. 12, recorded by Evince");
    expect(sentence).not.toContain("%");
    expect(sentence).not.toMatch(/\bof\b/);
  });

  it("says so when there is no progress at all", () => {
    expect(describeEntryProgress(null, english)).toBe("No reading progress recorded");
  });

  it("renders every provenance combination differently", () => {
    const rendered = [
      describeEntryProgress(progress({ source: "native", confidence: "exact" }), english),
      describeEntryProgress(progress({ source: "manual", confidence: "manual" }), english),
      describeEntryProgress(progress({ source: "evince-gvfs", confidence: "exact" }), english),
      describeEntryProgress(progress({ source: "evince-gvfs", confidence: "close-time" }), english),
      describeEntryProgress(progress({ source: "evince-gvfs", confidence: "unknown" }), english),
    ];
    expect(new Set(rendered).size).toBe(rendered.length);
  });

  it("names the known sources and falls back to the raw id for unknown ones", () => {
    expect(describeEntryProgress(progress({ source: "native" }), english)).toContain("oct");
    expect(
      describeEntryProgress(progress({ source: "manual", confidence: "manual" }), english),
    ).toContain("you");
    expect(describeEntryProgress(progress({ source: "evince-gvfs" }), english)).toContain("Evince");
    expect(describeEntryProgress(progress({ source: "zathura-history" }), english)).toContain(
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
      describeEntryProgress(
        progress({ page: 11, total: 80, source: "evince-gvfs" }),
        spanish,
      ),
    ).not.toBe(
      describeEntryProgress(progress({ page: 11, total: 80, source: "evince-gvfs" }), english),
    );
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

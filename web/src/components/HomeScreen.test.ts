import { render } from "svelte/server";
import { describe, expect, it } from "vitest";
import HomeScreen from "./HomeScreen.svelte";
import { translate } from "../lib/i18n";
import { homeCategories, type LibraryEntry } from "../lib/library";
import { categoryTitle } from "../lib/screens";

const english = translate("en");

function homeProps(overrides: { entries?: LibraryEntry[]; inFlight?: boolean } = {}) {
  return {
    entries: [] as LibraryEntry[],
    inFlight: false,
    error: null as string | null,
    locale: "en",
    t: english,
    onOpenFolder: () => {},
    onOpenDocument: () => {},
    onOpenCategory: () => {},
    onNavigate: () => {},
    ...overrides,
  };
}

/** A document with no reading record: the state that makes Inicio's carousels empty. */
function entry(overrides: Partial<LibraryEntry> = {}): LibraryEntry {
  return withProgress(overrides, null);
}

/** The same document WITH a recorded reading, which is what puts a card in a carousel. */
function read(overrides: Partial<LibraryEntry> = {}): LibraryEntry {
  return withProgress(overrides, {
    page: 0,
    total: 10,
    source: "native",
    confidence: "exact",
    recordedAt: null,
    readAt: null,
  });
}

function withProgress(
  overrides: Partial<LibraryEntry>,
  progress: LibraryEntry["progress"],
): LibraryEntry {
  return {
    id: 1,
    path: "/library/a.pdf",
    title: "A",
    userTitle: null,
    author: null,
    userAuthor: null,
    pages: 10,
    progress,
    ...overrides,
  };
}

/**
 * R2 and R4 (B1–B4). The carousel decisions are pure and tested in `library.test.ts`; what this pins
 * is the screen itself — that nothing explanatory is left in the header, that all three categories
 * are present with their definitions, that an empty carousel says so, and that each one carries the
 * button that opens its full list.
 */
describe("Inicio", () => {
  it("keeps its header to what names the screen, and nothing else (R2)", () => {

    // A library that already shows something, because an EMPTY one now legitimately carries a first-run
    // note under the heading (R34) — and R2's rule is about the header itself: the heading names the
    // screen and no prose is added to it. Everything before the first carousel is that header.
    const { body } = render(HomeScreen, { props: homeProps({ entries: [read()] }) });
    const header = body.slice(body.indexOf("<h1"), body.indexOf('class="carousel'));
    expect(header).toContain(">Home<");
    expect(header).not.toContain("<p");
  });

  it("shows all three carousels, and no definition under their headings (B1, L70)", () => {
    // The user asked for the definition sentences to go: a heading that already says "En curso" or
    // "Terminados" does not need a line explaining the words, and three of them read like a manual.
    // The guard is the absence itself, so the sentences cannot quietly come back.
    const { body } = render(HomeScreen, { props: homeProps() });

    for (const category of homeCategories) {
      expect(body).toContain(categoryTitle(category, english));
    }
    expect(body).not.toContain('class="definition');
    expect(body).not.toContain("Documents whose recorded position");
    expect(body).not.toContain("Documentos cuya posición registrada");
  });

  it("explains an empty first run, and offers the one action that fills it (R34)", () => {
    // Nothing has been found yet. The person has just installed this, lands on Inicio, and sees three
    // empty carousels: the missing fact is a folder being watched, and that is what the note says.
    const { body } = render(HomeScreen, { props: homeProps() });

    expect(body).toContain(english("home.firstRun.emptyLibrary.title"));
    expect(body).toContain(english("home.firstRun.emptyLibrary.body"));
    expect(body).toContain(english("home.firstRun.emptyLibrary.action"));
    // And NOT the other reason: two sentences for one state would be the screen guessing at a cause.
    expect(body).not.toContain(english("home.firstRun.noRecords.title"));
  });

  it("shows the loader, not the empty note, while a reload is in flight (L47)", () => {
    // The defect the user found: after removing and re-adding a watched folder the listing is
    // momentarily empty while the scan runs, and the screen rendered its empty state — the note that
    // says there is no watched folder at all. Nothing is missing; the work is in progress. An empty
    // listing may only be presented AS empty while nothing is in flight.
    const { body } = render(HomeScreen, { props: homeProps({ entries: [], inFlight: true }) });

    expect(body).toContain(english("library.loading"));
    expect(body).not.toContain(english("home.firstRun.emptyLibrary.title"));
    expect(body).not.toContain(english("home.firstRun.emptyLibrary.body"));
    // The three carousels would each say they are empty, which is the same false sentence.
    expect(body).not.toContain(english("home.categoryEmpty"));
  });

  it("never covers a listing that already has content with a loader (L47)", () => {
    // The fifteen-second re-read and a scan of a second folder both run while documents are already
    // on screen. Content wins over the loader: no spinner, no flash, no empty grid.
    const { body } = render(HomeScreen, { props: homeProps({ entries: [read()], inFlight: true }) });

    expect(body).not.toContain(english("library.loading"));
    expect(body).toMatch(/<article/g);
  });

  it("names the OTHER reason an empty Inicio can have: documents and no reading recorded (R34)", () => {
    // This is the state the user actually found themselves in, and the one that made them doubt the
    // carousels worked at all: the documents are there, and nothing has ever recorded a reading, because
    // this application never opens a document by itself — it observes reading done elsewhere.
    const { body } = render(HomeScreen, { props: homeProps({ entries: [entry()] }) });

    expect(body).toContain(english("home.firstRun.noRecords.title"));
    expect(body).toContain(english("home.firstRun.noRecords.body"));
    expect(body).toContain(english("home.firstRun.noRecords.action"));
    expect(body).not.toContain(english("home.firstRun.emptyLibrary.title"));
  });

  it("says nothing about a first run once a carousel has something in it (R34)", () => {
    // The note belongs to an empty screen only. Leaving it on a screen that already shows a document
    // would be the application explaining itself to somebody who is using it.
    const { body } = render(HomeScreen, { props: homeProps({ entries: [read()] }) });

    expect(body).not.toContain(english("home.firstRun.emptyLibrary.title"));
    expect(body).not.toContain(english("home.firstRun.noRecords.title"));
  });

  it("gives an empty carousel the card's cover shape without reserving its text block (B1)", () => {
    // One in-progress document fills the first carousel, so this render holds a real card and the two
    // empty slots at once — which is what makes the shapes directly comparable.
    const entry: LibraryEntry = {
      id: 1,
      path: "/library/a.pdf",
      title: "A",
      userTitle: null,
      author: null,
      userAuthor: null,
      pages: 10,
      progress: {
        page: 0,
        total: 10,
        source: "native",
        confidence: "exact",
        recordedAt: null,
        readAt: null,
      },
    };
    const { body } = render(HomeScreen, { props: homeProps({ entries: [entry] }) });

    // A real card is drawn once and the other two carousels state they are empty...
    expect(body.match(/<article/g)).toHaveLength(1);
    expect(body.split(english("home.categoryEmpty")).length - 1).toBe(homeCategories.length - 1);

    const styleOfClass = (name: string): string | null => {
      for (const tag of body.match(/<[^>]*>/g) ?? []) {
        const classes = tag.match(/class="([^"]*)"/)?.[1]?.split(/\s+/) ?? [];
        if (classes.includes(name)) {
          return tag.match(/style="([^"]*)"/)?.[1] ?? null;
        }
      }
      return null;
    };

    // ...and each empty slot carries the card's cover shape, not a second design. The text block below
    // is deliberately NOT reserved at a fixed height: reserving it was what made the card look
    // stretched and left the message floating in the emptiness it created, so an empty carousel is
    // now shorter than one holding cards (L31).
    expect(body.split('class="empty-cover').length - 1).toBe(homeCategories.length - 1);
    expect(styleOfClass("empty-cover")).toBe(styleOfClass("cover"));
    expect(styleOfClass("empty-cover")).toBe("aspect-ratio: 9 / 11");
    expect(styleOfClass("empty-body")).toBe(styleOfClass("body"));
    expect(styleOfClass("empty-body")).toBeNull();
    // The tile width is one value, declared on each carousel and read by both the real card's strip
    // and the empty slot.
    expect(body.split('style="--tile: 13rem"').length - 1).toBe(homeCategories.length);
  });

  it("puts a full-list button at the top of every carousel (B2)", () => {
    const { body } = render(HomeScreen, { props: homeProps() });

    expect(body.split(english("home.openCategory")).length - 1).toBe(homeCategories.length);
    for (const category of homeCategories) {
      expect(body).toContain(
        english("home.openCategoryLabel", { category: categoryTitle(category, english) }),
      );
    }
  });

  it("lays a carousel out as a sideways-scrolling strip (B1)", () => {
    const entry: LibraryEntry = {
      id: 1,
      path: "/library/a.pdf",
      title: "A",
      userTitle: null,
      author: null,
      userAuthor: null,
      pages: 10,
      progress: {
        page: 0,
        total: 10,
        source: "native",
        confidence: "exact",
        recordedAt: null,
        readAt: null,
      },
    };

    const { body } = render(HomeScreen, { props: homeProps({ entries: [entry] }) });

    expect(body).toMatch(/class="grid [^"]*strip"/);
  });
});

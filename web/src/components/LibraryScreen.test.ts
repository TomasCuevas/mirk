import { render } from "svelte/server";
import { describe, expect, it } from "vitest";
import LibraryScreen from "./LibraryScreen.svelte";
import { translate } from "../lib/i18n";
import type { LibraryEntry } from "../lib/library";

const english = translate("en");

function libraryProps(overrides: { entries?: LibraryEntry[]; inFlight?: boolean } = {}) {
  return {
    entries: [] as LibraryEntry[],
    inFlight: false,
    error: null as string | null,
    locale: "en",
    t: english,
    onOpenFolder: () => {},
    onOpenDocument: () => {},
    onResetScroll: () => {},
    ...overrides,
  };
}

/** One document with a recorded reading, which is what puts a card in the grid. */
function entry(overrides: Partial<LibraryEntry> = {}): LibraryEntry {
  return {
    id: 1,
    path: "/library/a.pdf",
    title: "A",
    userTitle: null,
    authors: [],
    readerAuthors: [],
    pages: 10,
    progress: {
      page: 0,
      total: 10,
      source: "native",
      confidence: "exact",
      recordedAt: null,
      readAt: null,
    },
    ...overrides,
  };
}

/**
 * Biblioteca, and the proof that the folder-action outcome chain is gone rather than half-removed.
 *
 * The screen used to take a `folderStatus` prop and thread it into the grid on behalf of the card —
 * whose two sentences that rendered the outcome had already been removed at the user's request (L27,
 * L30). The prop is gone with the chain now, so rendering this screen with no `folderStatus` among its
 * props is what says the removal is complete: a required prop left unsupplied would start out
 * `undefined`, the grid forward would be the only consumer, and this render is the one thing that
 * would not draw. The screen's own decisions (empty vs loading vs content) are pinned in
 * `library.test.ts`; what is pinned here is that the screen still draws at all without the chain.
 */
describe("Biblioteca without the folder-action outcome", () => {
  it("renders its heading and empty state with no folder status prop", () => {
    const { body } = render(LibraryScreen, { props: libraryProps() });

    expect(body).toContain(english("library.heading"));
    expect(body).toContain(english("library.empty"));
  });

  it("renders a card and the controls when the listing has documents", () => {
    const { body } = render(LibraryScreen, { props: libraryProps({ entries: [entry()] }) });

    // A real card, drawn through the shared grid, with no status passed down to it.
    expect(body).toMatch(/<article/g);
    expect(body).toContain(english("library.countAll", { total: 1 }));
  });

  it("shows the loader, not the empty sentence, while work is in flight (L47)", () => {
    const { body } = render(LibraryScreen, { props: libraryProps({ entries: [], inFlight: true }) });

    expect(body).toContain(english("library.loading"));
    expect(body).not.toContain(english("library.empty"));
  });
});

/**
 * S1 (R39): the screen gains an author selector beside the text filter and the sort.
 *
 * The render is server-side, so this cannot choose an author — the choices are pinned in
 * `library.test.ts`, and the act of choosing in `LibraryScreen.dom.test.ts`. What is drawn here is the
 * control itself: its label, an "all" entry, and one entry per author the cards show. Rendering it is
 * the only thing that says the selector was added to the screen at all rather than only to the module
 * behind it, and it is also what fails if the third control ever replaces one of the other two — the
 * text box's placeholder and the sort's labels are asserted beside it.
 */
describe("Biblioteca's author selector (R39)", () => {
  const entries = [
    entry({ id: 1, title: "Cosmos", authors: ["Carl Sagan"] }),
    entry({ id: 2, title: "Ficciones", authors: ["Jorge Luis Borges"] }),
  ];

  it("draws the selector with the authors the cards show and an 'all' entry", () => {
    const { body } = render(LibraryScreen, { props: libraryProps({ entries }) });

    expect(body).toContain(english("library.authorLabel"));
    expect(body).toContain(english("library.authorAll"));
    // S1: "all" is what it starts on, and the render says so by selecting that option.
    expect(body).toContain(`<option value="" selected="">${english("library.authorAll")}</option>`);
    expect(body).toContain('value="Carl Sagan"');
    expect(body).toContain('value="Jorge Luis Borges"');
  });

  it("keeps the text filter and the sort beside it, so the three compose", () => {
    const { body } = render(LibraryScreen, { props: libraryProps({ entries }) });

    expect(body).toContain(english("library.filterLabel"));
    expect(body).toContain(english("library.sortLabel"));
    expect(body).toContain('type="search"');
  });

  it("offers no author when no card has one, and still offers 'all'", () => {
    const { body } = render(LibraryScreen, {
      props: libraryProps({ entries: [entry({ id: 3, authors: [] })] }),
    });

    expect(body).toContain(english("library.authorAll"));
    expect(body.match(/<option/g)?.length).toBe(4); // three sorts and the one "all"
    // A shelf where no card has an author is the boundary that would empty the selector if the
    // "all" entry were not drawn unconditionally.
    expect(body).toContain('value=""');
  });
});

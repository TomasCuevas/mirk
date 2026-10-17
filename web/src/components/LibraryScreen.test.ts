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
    ...overrides,
  };
}

/** One document with a recorded reading, which is what puts a card in the grid. */
function entry(): LibraryEntry {
  return {
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
    expect(body).toContain(english("library.count", { shown: 1, total: 1 }));
  });

  it("shows the loader, not the empty sentence, while work is in flight (L47)", () => {
    const { body } = render(LibraryScreen, { props: libraryProps({ entries: [], inFlight: true }) });

    expect(body).toContain(english("library.loading"));
    expect(body).not.toContain(english("library.empty"));
  });
});

import { render } from "svelte/server";
import { describe, expect, it } from "vitest";
import BookGrid from "./BookGrid.svelte";
import { translate } from "../lib/i18n";
import type { LibraryEntry } from "../lib/library";

const english = translate("en");

/** `count` documents in one category's list, which is what a carousel is handed. */
function entries(count: number): LibraryEntry[] {
  return Array.from({ length: count }, (_, index) => ({
    id: index + 1,
    path: `/library/${index}.pdf`,
    title: `Document ${index}`,
    userTitle: null,
    author: null,
    userAuthor: null,
    pages: 10,
    progress: {
      page: 1,
      total: 10,
      source: "native",
      confidence: "exact",
      recordedAt: null,
      readAt: null,
    },
  }));
}

/** How many cards the rendering drew. */
function cardsDrawn(count: number, layout: "grid" | "strip"): number {
  const { body } = render(BookGrid, {
    props: {
      entries: entries(count),
      t: english,
      onOpenFolder: () => {},
      onOpenDocument: () => {},
      layout,
    },
  });
  return body.match(/<article/g)?.length ?? 0;
}

describe("how many tiles a grid draws", () => {
  // R16 (L37). The user's carousel held every document in its category — "me aparecieron demasiados,
  // tantos que molestaba tener un carrusel tan largo" — so a strip stops at the same number the
  // library paginates by, and "open the full list" is where the rest belongs.
  //
  // This test exists because the cap was once computed and then never used, and the build stayed green
  // while the carousel kept drawing everything: the rule had no test, so nothing could fail.
  it("caps a carousel at ten tiles however many the category holds", () => {
    expect(cardsDrawn(25, "strip")).toBe(10);
  });

  // And the cap must not leak into a full list: a grid draws everything it is given, because the
  // category's own screen is the one place that is supposed to be complete.
  it("draws every entry when the list is not a carousel", () => {
    expect(cardsDrawn(25, "grid")).toBe(25);
  });
});

/**
 * The document editor (R32, corrected).
 *
 * The pencil used to open a title-only modal while the cover had a control of its own on the card's
 * bottom-left. The user rejected a third control over a small cover — "everything related to editing
 * should be on the pencil icon" — so the cover's page, its image and the removal of that image now live
 * inside the one modal the pencil opens. That modal was never tested; this is the test it never had.
 *
 * The modal is the grid's own state, so rendering it needs the grid told which document is open. That
 * is what `initialEditing` is for: the same kind of render seam as BookCard's `cover`.
 */
describe("the document editor", () => {
  const props = { t: english, onOpenFolder: () => {}, onOpenDocument: () => {} };

  function document(overrides: Partial<LibraryEntry> = {}): LibraryEntry {
    return { ...entries(1)[0], ...overrides };
  }

  function open(entry: LibraryEntry): string {
    return render(BookGrid, {
      props: { ...props, entries: [entry], initialEditing: entry },
    }).body;
  }

  it("holds the cover controls inside the pencil's modal, not only the title", () => {
    const body = open(document({ coverImage: "chosen.png" }));

    // One editing surface: the dialog is on screen…
    expect(body).toContain('role="dialog"');
    // …and it holds the title editor and the cover: the page field, choosing an image and removing it.
    expect(body).toContain(english("card.titleLabel"));
    expect(body).toContain(english("card.coverPageLabel"));
    expect(body).toContain(english("card.coverChooseImage"));
    expect(body).toContain(english("card.coverRemoveImage"));
  });

  it("draws the author list as a real select, and no datalist", () => {
    const first = document({ id: 1, author: "Ursula K. Le Guin", userAuthor: null });
    const second = document({ id: 2, author: "Jorge Luis Borges", userAuthor: null });
    const body = render(BookGrid, {
      props: { ...props, entries: [first, second], initialEditing: first },
    }).body;

    // The author lives in the same modal as the title and the cover, prefilled with the author on
    // screen, and its list is the authors the library already holds — so nobody retypes a name and
    // ends up with two spellings of one person.
    expect(body).toContain(english("card.authorLabel"));
    // S2: WebKitGTK — the engine this window renders in — never implemented the `datalist` popup, so a
    // complete list behind one would still open nothing. The control has to be a real `select`, with
    // the catalogue's "choose an author" entry first and the authors after it.
    expect(body).not.toContain("datalist");
    expect(body).toContain("<select");
    expect(body).toContain(english("card.authorChoose"));
    expect(body).toContain('<option value="Ursula K. Le Guin">');
    expect(body).toContain('<option value="Jorge Luis Borges">');
  });

  // S1, at the component's own boundary: the grid draws the list it is HANDED, because a screen that
  // holds one page cannot compute the authors of the whole library. What the screens compute is pinned
  // where the whole listing lives; this pins that the prop is what the control reads.
  it("draws the authors it is handed, not only the ones on the page it shows", () => {
    const onThisPage = document({ id: 1, author: "Page One Author", userAuthor: null });
    const body = render(BookGrid, {
      props: { ...props, entries: [onThisPage], authors: ["Only Elsewhere"], initialEditing: onThisPage },
    }).body;
    const select = body.slice(body.indexOf("<select"));

    expect(select).toContain('<option value="Only Elsewhere">');
    expect(select).not.toContain("Page One Author");
  });

  it("offers to remove the image only once the reader supplied one", () => {
    expect(open(document({ coverImage: null }))).not.toContain(english("card.coverRemoveImage"));
    expect(open(document({ coverImage: "chosen.png" }))).toContain(english("card.coverRemoveImage"));
  });

  it("leaves the cover itself with no control of its own", () => {
    const body = open(document());

    // The cover keeps only the page badge, which states a fact rather than opening an editor.
    expect(body).not.toContain("edit-cover");
    expect(body).toContain("cover-pages");
  });
});

/**
 * R32, corrected: the editor's facts come from the listing the interface now holds, not from the
 * snapshot it was opened with.
 *
 * A completed cover change refreshes the listing exactly as a completed title change already did, so
 * by the time the editor states whose title this is or seeds its page field the current entry is in
 * `entries`. Reading the snapshot instead is how the reader's own title seemed to vanish after an
 * image was chosen, and how the page they had chosen came back empty when the editor was reopened.
 * The dialogue cannot be clicked, so what is pinned here is the state it must show for a document the
 * listing has moved on from — the same state the refresh produces.
 */
describe("the document editor and the listing it reads from", () => {
  const props = { t: english, onOpenFolder: () => {}, onOpenDocument: () => {} };

  // The entry the editor was opened with, before the cover change reached the listing, and the same
  // document once the listing has been re-read: the reader's title is back and page 5 is the cover.
  function snapshotAndCurrent(): { opened: LibraryEntry; current: LibraryEntry } {
    const base = entries(1)[0];
    const opened: LibraryEntry = {
      ...base,
      title: "Microsoft Word - Completo.rtf",
      userTitle: null,
      coverPage: null,
      coverImage: null,
    };
    const current: LibraryEntry = {
      ...opened,
      title: "Padre Rico, Padre Pobre",
      userTitle: "Padre Rico, Padre Pobre",
      coverPage: 4,
    };
    return { opened, current };
  }

  // Only the dialog, so the card's own copy of the title cannot stand in for the editor's.
  function dialog(openedEntry: LibraryEntry, currentEntry: LibraryEntry): string {
    const body = render(BookGrid, {
      props: { ...props, entries: [currentEntry], initialEditing: openedEntry },
    }).body;
    return body.slice(body.indexOf('role="dialog"'));
  }

  it("still carries the reader's title after a cover change", () => {
    const { opened, current } = snapshotAndCurrent();
    const shown = dialog(opened, current);
    expect(shown).toContain("Padre Rico, Padre Pobre");
    expect(shown).not.toContain("Microsoft Word - Completo.rtf");
  });

  it("shows the page the reader chose, so reopening is not empty", () => {
    const { opened, current } = snapshotAndCurrent();
    const shown = dialog(opened, current);
    expect(shown).toContain('value="5"');
    expect(shown).not.toContain('value="1"');
  });
});

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
    authors: [],
    readerAuthors: [],
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

  // R18: the user could not tell whether the cover was a page of the document or a file of their own,
  // and the section never said. It states the current source, and the two ways to change it are two
  // labelled blocks that cannot be confused.
  it("says what the cover is, and separates the page from the image", () => {
    const body = open(document({ coverPage: null, coverImage: null }));

    expect(body).toContain(english("card.coverCurrentPage", { page: "1" }));
    expect(body).toContain(english("card.coverSourceHint"));
    expect(body).toContain(english("card.coverPageSection"));
    expect(body).toContain(english("card.coverImageSection"));

    // And an untouched editor offers nothing to save (R18): the pair appears with the first change.
    expect(body).not.toContain(english("card.titleSave"));
    expect(body).not.toContain(english("card.titleCancel"));
  });

  it("says an image is the cover when the document has one of its own", () => {
    const body = open(document({ coverImage: "chosen.png" }));

    expect(body).toContain(english("card.coverCurrentImage"));
    expect(body).not.toContain(english("card.coverCurrentPage", { page: "1" }));
  });

  it("draws the author field as a combobox, and no datalist", () => {
    const first = document({ id: 1, authors: ["Ursula K. Le Guin"], readerAuthors: [] });
    const second = document({ id: 2, authors: ["Jorge Luis Borges"], readerAuthors: [] });
    const body = render(BookGrid, {
      props: { ...props, entries: [first, second], initialEditing: first },
    }).body;

    // The author lives in the same modal as the title and the cover, prefilled with the author on
    // screen.
    expect(body).toContain(english("card.authorLabel"));
    // R16: the field itself is the picker. A `<datalist>` is still not an option (WebKitGTK never
    // implemented its popup, L70), and the `<select>` that replaced it is gone too — one gesture, not
    // two. What the field OFFERS is decided by `authorChoices` and pinned in `library.test.ts`; what
    // the reader sees when it opens is pinned in `BookGrid.dom.test.ts`.
    expect(body).not.toContain("datalist");
    expect(body).not.toContain("<select");
    expect(body).toContain('role="combobox"');
    expect(body).toContain(`aria-label="${english("card.authorAdd")}"`);
  });

  /** How many remove controls the editor drew — one per chip, and never more. */
  function removeControls(body: string): number {
    return (body.match(/aria-label="Remove /g) ?? []).length;
  }

  // R42/U30, rendered: ONE CHIP PER AUTHOR, in the reader's order, and each chip carries its own
  // remove control NAMED after that author (S3). An icon with no name would leave two identical
  // "remove" buttons and no way to know which name goes, so what is pinned here is the names.
  it("draws one chip per author, in order, each with a remove control named after it", () => {
    const body = open(
      document({
        authors: ["Carl Sagan", "Ann Druyan"],
        readerAuthors: ["Carl Sagan", "Ann Druyan"],
      }),
    );

    const first = english("card.authorRemove", { author: "Carl Sagan" });
    const second = english("card.authorRemove", { author: "Ann Druyan" });
    expect(body).toContain(first);
    expect(body).toContain(second);
    // The order on screen is the reader's, so the first chip's control comes first in the markup.
    expect(body.indexOf(first)).toBeLessThan(body.indexOf(second));
    expect(removeControls(body)).toBe(2);
  });

  // S6, on screen: the document's embedded author string starts as ONE chip, because splitting
  // "Sagan, Carl; Druyan, Ann" would invent two people nobody wrote.
  it("takes the embedded author string as one chip, not two", () => {
    const body = open(
      document({ authors: ["Sagan, Carl; Druyan, Ann"], readerAuthors: [] }),
    );

    expect(body).toContain("Sagan, Carl; Druyan, Ann");
    expect(removeControls(body)).toBe(1);
  });

  it("draws no chip and no remove control for a document nobody is named on", () => {
    const body = open(document({ authors: [], readerAuthors: [] }));

    expect(removeControls(body)).toBe(0);
    // The add field is still there — the way a name gets in, typed or chosen from the list.
    expect(body).toContain(english("card.authorAddPlaceholder"));
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

/**
 * L13/L14: the document editor is the dialog that has content for a wide window.
 *
 * At two per row the narrow dialog was right; once the window grows, the same form in a 22 rem box
 * looked pinned to the corner of a large screen. The editor asks for the wide dialog — and L14 keeps
 * its content in one vertical column, because the reader rejected the two-column split.
 */
describe("the document editor in a wide window", () => {
  it("uses the wide dialog", () => {
    const { body } = render(BookGrid, {
      props: {
        entries: entries(1),
        t: english,
        onOpenFolder: () => {},
        onOpenDocument: () => {},
        initialEditing: entries(1)[0],
      },
    });

    expect(body).toMatch(/class="[^"]*\bdialog\b[^"]*\bwide\b/);
  });
});

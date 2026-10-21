import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import LibraryScreen from "./LibraryScreen.svelte";
import { translate } from "../lib/i18n";
import type { LibraryEntry } from "../lib/library";

/**
 * Choosing an author, in a simulated browser (R39, S1 and S3).
 *
 * Every other test of this screen renders it server-side, which produces the HTML and runs nothing: it
 * can see that the selector was drawn, never that choosing an author does anything. The one link
 * nobody could fail — the change that has to reach the listing — would stay unchecked, and a future
 * edit could drop the handler, or wire it to the sort, with the whole suite green. This file closes
 * that hole with the same simulated browser the card's click and the editor's save use (L64).
 *
 * Everything asserted here is what the reader sees: which cards remain, the author line on them, and
 * the count sentence underneath. Nothing reaches into the component's state.
 */

const english = translate("en");

function entry(overrides: Partial<LibraryEntry> = {}): LibraryEntry {
  return {
    id: 1,
    path: "/library/document.pdf",
    title: "document",
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
 * The library this file filters: two authors, a document with no author at all, and page counts chosen
 * so the pages order differs from the title order — which is what makes the sort's share of S3 visible.
 */
const shelf: LibraryEntry[] = [
  entry({ id: 1, path: "/library/cosmos.pdf", title: "Cosmos", authors: ["Carl Sagan"], pages: 10 }),
  entry({ id: 2, path: "/library/contact.pdf", title: "Contact", authors: ["Carl Sagan"], pages: 300 }),
  entry({
    id: 3,
    path: "/library/ficciones.pdf",
    title: "Ficciones",
    authors: ["Jorge Luis Borges"],
    pages: 200,
  }),
  entry({ id: 4, path: "/library/anonymous.pdf", title: "Anónimo", authors: [], pages: 40 }),
];

/** The titles the grid is actually drawing, in order. */
function titles(): string[] {
  return [...window.document.querySelectorAll("article .title-text")].map(
    (node) => node.textContent?.trim() ?? "",
  );
}

/** The author lines the cards are drawing, which is where an author's name is really on the screen. */
function authorLines(): string[] {
  return [...window.document.querySelectorAll("article .author")].map(
    (node) => node.textContent?.trim() ?? "",
  );
}

/**
 * The author selector, found the way a reader would: by the names among its options.
 *
 * The screen draws two selects (the sort and this one), so the authors themselves identify it — which
 * also means this test fails if the selector stops offering them rather than quietly finding the sort.
 */
function authorSelect(): HTMLSelectElement | undefined {
  return [...window.document.querySelectorAll("select")].find(
    (select) => select.querySelector('option[value="Jorge Luis Borges"]') !== null,
  );
}

/** The screen's count sentence, which is one of the pieces of text that follows the narrowing. */
function count(): string {
  // R15: the count is the line under the title now, not the tail of the controls row.
  return window.document.querySelector(".lede")?.textContent?.trim() ?? "";
}

function searchBox(): HTMLInputElement {
  const box = window.document.querySelector('input[type="search"]');
  if (box === null) {
    throw new Error("the library screen draws its text filter");
  }
  return box as HTMLInputElement;
}

/** Picks an option the way the browser does when the reader picks one: set the value, then `change`. */
function choose(select: HTMLSelectElement | undefined, value: string): void {
  if (select === undefined) {
    throw new Error("the screen draws the control this test needs");
  }
  select.value = value;
  // `bubbles: true` because that is what a browser sends: Svelte delegates these listeners to the
  // root, so an event that does not travel up reaches nobody — an earlier version of this helper sent
  // a plain `new Event("change")` and silently tested nothing.
  select.dispatchEvent(new Event("change", { bubbles: true }));
  flushSync();
}

/** Types into the text filter, which a browser reports as an `input` event. */
function type(query: string): void {
  const box = searchBox();
  box.value = query;
  box.dispatchEvent(new Event("input", { bubbles: true }));
  flushSync();
}

describe("choosing an author in Biblioteca", () => {
  let component: ReturnType<typeof mount> | null = null;

  function render(): void {
    component = mount(LibraryScreen, {
      target: window.document.body,
      props: {
        entries: shelf,
        inFlight: false,
        error: null,
        locale: "en",
        t: english,
        onOpenFolder: () => {},
        onOpenDocument: () => {},
    onResetScroll: () => {},
      },
    });
    flushSync();
  }

  afterEach(() => {
    if (component !== null) {
      unmount(component);
      component = null;
    }
    window.document.body.innerHTML = "";
  });

  it("starts on 'all', with every card on screen", () => {
    render();
    const select = authorSelect();
    expect(select, "the screen draws an author selector").toBeDefined();

    // S1: "all" is where it starts, and it means what a blank text box means — the whole listing, in
    // the order the sort already chose for it.
    expect(select?.value).toBe("");
    expect(titles()).toEqual(["Anónimo", "Contact", "Cosmos", "Ficciones"]);
    expect(count()).toBe(english("library.countAll", { total: 4 }));
  });

  it("leaves only the chosen author's cards, and states the narrowed count", () => {
    render();

    choose(authorSelect(), "Jorge Luis Borges");

    // Only that author's card remains: the other author's two documents and the authorless one are
    // gone, because a list under one name cannot hold a document that carries another.
    expect(titles()).toEqual(["Ficciones"]);
    expect(authorLines()).toEqual([english("library.author", { author: "Jorge Luis Borges" })]);
    expect(count()).toBe(english("library.count", { shown: 1, total: 4 }));

    // And back to the whole shelf, so the selector is not a one-way door.
    choose(authorSelect(), "");
    expect(titles()).toEqual(["Anónimo", "Contact", "Cosmos", "Ficciones"]);
  });

  it("composes with the text filter, so both narrow the same grid", () => {
    render();

    choose(authorSelect(), "Carl Sagan");
    type("con");

    // Not the union of the two controls: "Cosmos" matches the box but not what is left after the
    // author, and only "Contact" survives both.
    expect(titles()).toEqual(["Contact"]);
    expect(count()).toBe(english("library.count", { shown: 1, total: 4 }));

    // Clearing the text box keeps the author, which is what "the three compose" means.
    type("");
    expect(titles()).toEqual(["Contact", "Cosmos"]);
  });

  it("shows the narrowed listing in the sort's order, so the third control still applies", () => {
    render();
    choose(authorSelect(), "Carl Sagan");

    // Title order over the narrowed listing: "Contact" before "Cosmos", which is not the order the
    // rows arrived in, so the author narrowed the listing and the sort still decided what it looks
    // like. The sort control itself is not pressed here: Svelte reads a select's value with
    // `querySelector(":checked")`, which this simulated browser answers with `null` even when an
    // option is selected, so a change dispatched at the sort would be read back as the first option.
    // The sort's share of S3 is pinned in `library.test.ts`, over the real functions.
    expect(titles()).toEqual(["Contact", "Cosmos"]);
  });
});

/**
 * R40 / S1: the editor's author list comes from the WHOLE library, not from the page it was opened on.
 *
 * This is the user's own report — *"solo aparecen los autores que esten en los libros de la pagina
 * actual y no de todos los PDF disponibles y cargados"* — and the defect was a scope bug: the editor
 * lives in the grid, the grid derived its list from the `entries` it was handed, and the library screen
 * hands it ONE PAGE of ten. An author whose only document sat on page two did not exist.
 */
describe("the editor's author list on a paged library", () => {
  let component: ReturnType<typeof mount> | null = null;

  afterEach(() => {
    if (component !== null) {
      unmount(component);
      component = null;
    }
    window.document.body.innerHTML = "";
  });

  // Eleven documents so the listing really has two pages: the first ten fill page one in title order
  // (`page-01`…`page-10`), and the eleventh — the only one by this author — lands on page two.
  const ONLY_ON_PAGE_TWO = "Only On Page Two";
  const paged: LibraryEntry[] = [
    ...Array.from({ length: 10 }, (_, index) =>
      entry({
        id: index + 1,
        path: `/library/page-${index + 1}.pdf`,
        title: `page-${String(index + 1).padStart(2, "0")}`,
        authors: ["Page One Author"],
      }),
    ),
    entry({
      id: 11,
      path: "/library/last.pdf",
      title: "page-11",
      authors: [ONLY_ON_PAGE_TWO],
    }),
  ];

  function renderPaged(): void {
    component = mount(LibraryScreen, {
      target: window.document.body,
      props: {
        entries: paged,
        inFlight: false,
        error: null,
        locale: "en",
        t: english,
        onOpenFolder: () => {},
        onOpenDocument: () => {},
    onResetScroll: () => {},
      },
    });
    flushSync();
  }

  /** A card's pencil, found the way the reader finds it: by the document it edits. */
  function editButton(title: string): HTMLButtonElement | undefined {
    return [
      ...window.document.querySelectorAll<HTMLButtonElement>("article button.edit-document"),
    ].find(
      (button) =>
        button.getAttribute("aria-label") === english("card.editDocument", { title }),
    );
  }

  /** The values the editor's author list is offering. */
  /** R16: the field is a combobox now, so the list it opens is the dialog's own listbox. */
  function offeredAuthors(): string[] {
    const field = window.document.querySelector<HTMLInputElement>(
      '[role="dialog"] input[role="combobox"]',
    );
    field?.dispatchEvent(new FocusEvent("focus"));
    flushSync();
    return [
      ...window.document.querySelectorAll<HTMLButtonElement>(
        '[role="dialog"] #author-choices button[role="option"]',
      ),
    ].map((option) => option.textContent?.trim() ?? "");
  }

  it("offers the author of a document that sits on another page", () => {
    renderPaged();

    // The page really hides that document: ten cards are drawn and none of them is by that author.
    expect(titles()).toHaveLength(10);
    expect(authorLines()).not.toContain(
      english("library.author", { author: ONLY_ON_PAGE_TWO }),
    );

    // The editor is opened from a card that IS on page one…
    editButton("page-01")?.click();
    flushSync();

    // …and the list offers the author whose only document is on page two, because the screen computed
    // it from the whole listing (S1).
    expect(offeredAuthors()).toContain(ONLY_ON_PAGE_TWO);
  });
});

/**
 * R42/S7: a document can be written by several people, and the selector and the search read the
 * DISPLAYED list — the one the core resolved — so a co-author is as findable as the first name.
 *
 * Both names of the co-authored document are offered by the selector, choosing either of them leaves
 * that document, and the free-text box finds it by either name too. What the reader sees is what is
 * asserted: the options, the cards that remain, and the sentence on the card.
 */
describe("a co-authored document in Biblioteca (R42/S7)", () => {
  let component: ReturnType<typeof mount> | null = null;

  const coauthored: LibraryEntry[] = [
    entry({
      id: 1,
      path: "/library/cosmos.pdf",
      title: "Cosmos",
      authors: ["Carl Sagan", "Ann Druyan"],
      pages: 10,
    }),
    entry({
      id: 2,
      path: "/library/contact.pdf",
      title: "Contact",
      authors: ["Carl Sagan"],
      pages: 300,
    }),
    entry({
      id: 3,
      path: "/library/ficciones.pdf",
      title: "Ficciones",
      authors: ["Jorge Luis Borges"],
      pages: 200,
    }),
  ];

  function render(): void {
    component = mount(LibraryScreen, {
      target: window.document.body,
      props: {
        entries: coauthored,
        inFlight: false,
        error: null,
        locale: "en",
        t: english,
        onOpenFolder: () => {},
        onOpenDocument: () => {},
    onResetScroll: () => {},
      },
    });
    flushSync();
  }

  afterEach(() => {
    if (component !== null) {
      unmount(component);
      component = null;
    }
    window.document.body.innerHTML = "";
  });

  it("offers both co-authors, and draws the joined sentence on the card", () => {
    render();
    const options = [...(authorSelect()?.querySelectorAll("option") ?? [])].map(
      (option) => option.value,
    );

    // Every displayed author is offered, the second name of a document included.
    expect(options).toContain("Carl Sagan");
    expect(options).toContain("Ann Druyan");
    expect(authorLines()).toContain(
      english("library.author", { author: "Carl Sagan and Ann Druyan" }),
    );
  });

  it("narrows to the same document through either co-author", () => {
    render();

    choose(authorSelect(), "Ann Druyan");
    expect(titles()).toEqual(["Cosmos"]);

    choose(authorSelect(), "Carl Sagan");
    expect(titles()).toEqual(["Contact", "Cosmos"]);
  });

  it("finds the document by a co-author in the free-text search", () => {
    render();

    type("druyan");
    expect(titles()).toEqual(["Cosmos"]);
  });
});

/**
 * Turning a page is new content, so it asks the shell to start at the top (L73).
 *
 * The user's report, after the navigation reset had already landed: *"cuando hago la paginacion en la
 * seccion biblioteca, el scroll no se resetea al top"*. Pagination is not navigation — `view` does not
 * change — so the reset that answers navigation never fires, and page two opened wherever page one was
 * scrolled to. The screen cannot reach the shell's scrolling container, so it asks through the prop it
 * is given; this file proves the ask happens, on both pager buttons, once per click.
 */
describe("a page turn asks the shell for the top", () => {
  let component: ReturnType<typeof mount> | null = null;

  afterEach(() => {
    if (component !== null) {
      unmount(component);
      component = null;
    }
    window.document.body.innerHTML = "";
  });

  /** Twenty documents at ten per page: exactly two pages, so both buttons are reachable. */
  function shelf(): LibraryEntry[] {
    return Array.from({ length: 20 }, (_, index) =>
      entry({ id: index + 1, title: `Document ${String(index + 1).padStart(2, "0")}` }),
    );
  }

  /** R15: the pager's arrows carry no text any more, so the accessible name is what finds them. */
  function pagerButton(label: string): HTMLButtonElement {
    const button = [...window.document.querySelectorAll("button")].find(
      (candidate) => candidate.getAttribute("aria-label") === label,
    );
    if (button === undefined) {
      throw new Error(`the screen draws its "${label}" control`);
    }
    return button as HTMLButtonElement;
  }

  it("asks once when the reader goes to the next page", () => {
    const onResetScroll = vi.fn();
    component = mount(LibraryScreen, {
      target: window.document.body,
      props: {
        entries: shelf(),
        inFlight: false,
        error: null,
        locale: "en",
        t: english,
        onOpenFolder: () => {},
        onOpenDocument: () => {},
        onResetScroll,
      },
    });
    flushSync();

    expect(onResetScroll).not.toHaveBeenCalled();
    pagerButton(english("library.nextPage")).click();
    flushSync();

    expect(onResetScroll).toHaveBeenCalledTimes(1);
  });

  it("asks once when the reader goes back to the previous page", () => {
    const onResetScroll = vi.fn();
    component = mount(LibraryScreen, {
      target: window.document.body,
      props: {
        entries: shelf(),
        inFlight: false,
        error: null,
        locale: "en",
        t: english,
        onOpenFolder: () => {},
        onOpenDocument: () => {},
        onResetScroll,
      },
    });
    flushSync();

    pagerButton(english("library.nextPage")).click();
    flushSync();
    onResetScroll.mockClear();

    pagerButton(english("library.previousPage")).click();
    flushSync();

    expect(onResetScroll).toHaveBeenCalledTimes(1);
  });
});

/**
 * A pager that cannot move is a control with no meaning.
 *
 * The library in this file is four documents at ten per page: one page. Both arrows are already
 * disabled, but drawing `< 1 >` still tells the reader there is something to turn to, and it does so
 * above the grid, where it holds the space a filter row would use. The pager stays exactly where it
 * is whenever there IS a page to turn — that placement was the reader's own request (L22, L27) — so
 * what this proves is the single-page case only.
 */
describe("a single page draws no pager", () => {
  it("draws no page controls when every document fits on one page", () => {
    const component = mount(LibraryScreen, {
      target: window.document.body,
      props: {
        entries: shelf,
        inFlight: false,
        error: null,
        locale: "en",
        t: english,
        onOpenFolder: () => {},
        onOpenDocument: () => {},
        onResetScroll: () => {},
      },
    });
    flushSync();

    const pager = window.document.querySelector(
      `nav[aria-label="${english("library.pagination")}"]`,
    );

    expect(pager).toBeNull();
    unmount(component);
  });
});

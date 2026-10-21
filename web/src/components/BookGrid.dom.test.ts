import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import BookGrid from "./BookGrid.svelte";
import { translate } from "../lib/i18n";
import type { LibraryEntry } from "../lib/library";

/**
 * The editor's save contract, exercised by an actual click (L64).
 *
 * Every other test of this editor renders it server-side, which cannot press a button: the HTML is
 * produced and nothing runs. That left the one link in the chain nobody could fail — the call that
 * carries the reader's answer to the store — so a future edit could drop it, or add a second one and
 * write twice, with the whole suite still green. This file closes that hole with a simulated browser,
 * the dependency the user authorised for exactly this reason.
 */

// Only `invoke` is replaced. `runningInTauri` stays the REAL function, and the test sets the global it
// looks for — so the guard that keeps the editor honest outside the desktop shell is exercised too,
// rather than assumed.
const { invoke } = vi.hoisted(() => ({
  invoke: vi.fn(
    async (_command: string, _args?: unknown): Promise<unknown> => null,
  ),
}));

vi.mock("../lib/tauri", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/tauri")>();
  return { ...actual, invoke };
});

const english = translate("en");

function entry(): LibraryEntry {
  return {
    id: 7,
    path: "/library/seven.pdf",
    title: "Document Seven",
    userTitle: null,
    authors: [],
    readerAuthors: [],
    pages: 10,
    progress: {
      page: 3,
      total: 10,
      source: "native",
      confidence: "exact",
      recordedAt: null,
      readAt: null,
    },
  };
}

/** The save calls that reached the backend, in order. Cover renders are a different command. */
function saves(): unknown[][] {
  return invoke.mock.calls.filter((call) => call[0] === "set_document_naming");
}

function mountEditor(
  document_: LibraryEntry,
  authors?: readonly string[],
): ReturnType<typeof mount> {
  const component = mount(BookGrid, {
    target: window.document.body,
    props: {
      entries: [document_],
      t: english,
      onOpenFolder: () => {},
      onOpenDocument: () => {},
      initialEditing: document_,
      ...(authors === undefined ? {} : { authors }),
    },
  });
  flushSync();
  return component;
}

function saveButton(): HTMLButtonElement | undefined {
  return [...window.document.body.querySelectorAll("button")].find(
    (button) => button.textContent?.trim() === english("card.titleSave"),
  );
}

function cancelButton(): HTMLButtonElement | undefined {
  return [...window.document.body.querySelectorAll("button")].find(
    (button) => button.textContent?.trim() === english("card.titleCancel"),
  );
}

/**
 * The editor's add-a-name field, found the way the reader sees it: by its accessible name.
 *
 * It is the ADD field since R42/U30 — the chips are the list's value — so it starts empty even when
 * the document already has authors, and pressing Enter in it adds a chip instead of saving.
 */
function authorAddField(): HTMLInputElement | undefined {
  const field = window.document.querySelector(`input[aria-label="${english("card.authorAdd")}"]`);
  return field === null ? undefined : (field as HTMLInputElement);
}

/** Types a name the way a browser reports typing, then presses Enter to add it as a chip. */
function typeAuthor(name: string): void {
  const field = authorAddField();
  expect(field, "the editor renders the add-a-name field").toBeDefined();
  field!.value = name;
  field!.dispatchEvent(new Event("input", { bubbles: true }));
  flushSync();
  field!.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
  flushSync();
}

/** Types into the title field, the way the reader does. */
function typeTitle(value: string): void {
  const field = window.document.body.querySelector<HTMLInputElement>(
    `input[aria-label="${english("card.titleLabel")}"], input[aria-label="${english("card.titleLabelYours")}"]`,
  );
  expect(field, "the editor renders the title field").toBeDefined();
  field!.value = value;
  field!.dispatchEvent(new Event("input", { bubbles: true }));
  flushSync();
}

/** The chips on screen, in the order they are drawn. */
function chipNames(): string[] {
  return [...window.document.body.querySelectorAll(".author-chip-name")].map(
    (chip) => chip.textContent?.trim() ?? "",
  );
}

/** A chip's own remove control, found by the accessible name that names its author. */
function removeControl(name: string): HTMLButtonElement | undefined {
  const button = window.document.body.querySelector(
    `button[aria-label="${english("card.authorRemove", { author: name })}"]`,
  );
  return button === null ? undefined : (button as HTMLButtonElement);
}

/** The card's pencil, which opens the one editor (R31/R32). */
/**
 * The editor's author list, found by its accessible name — the same way the reader finds it.
 *
 * It used to be located by one of the names among its options, which quietly tied every test to that
 * one name: the co-author test hands a different library and found no list at all.
 */
/** R16: the field IS the picker, so the control to find is the combobox itself. */
function authorField(): HTMLInputElement | undefined {
  const field = window.document.body.querySelector('input[role="combobox"]');
  return field === null ? undefined : (field as HTMLInputElement);
}

/** The names the field is offering right now, in order. */
function offeredNames(): string[] {
  return [...window.document.body.querySelectorAll("#author-choices button[role='option']")].map(
    (option) => option.textContent?.trim() ?? "",
  );
}

/** Focus opens the list, the way a reader sees it the moment they click the field. */
function openChoices(): void {
  const field = authorField();
  expect(field, "the editor renders the author combobox").toBeDefined();
  field!.dispatchEvent(new FocusEvent("focus"));
  flushSync();
}

/** Clicks a name in the open list — the pointer path, not the keyboard one. */
function chooseFromList(name: string): void {
  openChoices();
  const option = [...window.document.body.querySelectorAll<HTMLButtonElement>(
    "#author-choices button[role='option']",
  )].find((candidate) => candidate.textContent?.trim() === name);
  expect(option, `the field offers "${name}"`).toBeDefined();
  option!.click();
  flushSync();
}

describe("the editor's Save, pressed for real", () => {
  let component: ReturnType<typeof mount> | null = null;

  afterEach(() => {
    if (component !== null) {
      unmount(component);
      component = null;
    }
    window.document.body.innerHTML = "";
    invoke.mockReset();
    invoke.mockImplementation(async () => null);
  });

  function prepare(authors?: readonly string[]): LibraryEntry {
    // Inside the desktop shell, which is the only place a save can happen: the real guard reads this.
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    const document_ = entry();
    component = mountEditor(document_, authors);
    return document_;
  }

  it("carries the title and the author to the store in ONE command, once", async () => {
    const document_ = prepare();
    // R18: the pair exists only while there is something to save, so the test makes a change first —
    // which is also the real path a reader takes.
    expect(saveButton(), "an untouched editor offers nothing to save").toBeUndefined();
    typeTitle("A title of my own");
    expect(saveButton(), "the editor renders a Save button once there is a change").toBeDefined();

    saveButton()?.click();

    // Wait for the successful save's own observable end — the pair disappears because the store now
    // holds the draft — before counting. Without this the count raced the second call a double write
    // would make.
    await vi.waitFor(() => expect(saveButton()).toBeUndefined());

    // R18: and the form is still on screen. The user asked for exactly this: saving must not take the
    // editor away, because the next change is usually right there.
    expect(window.document.body.textContent).toContain(english("card.documentEditor"));

    // One command carrying BOTH facts. Written as two commands it was two writes, and a failure
    // between them left the title saved while the screen implied nothing had been (L55).
    expect(saves()).toHaveLength(1);
    expect(saves()[0]).toEqual([
      "set_document_naming",
      { id: document_.id, title: "A title of my own", authors: [] },
    ]);
  });

  it("keeps the editor open, the chips and the reason when the write fails", async () => {
    prepare();
    typeAuthor("Carl Sagan");
    invoke.mockRejectedValueOnce(new Error("the store refused the write"));

    saveButton()?.click();

    // The failure is visible in the reader's own words, and the editor is still there — closing it
    // would claim a save that did not happen. The draft survives too: nothing was applied, so nothing
    // of what the reader built is lost.
    await vi.waitFor(() =>
      expect(window.document.body.textContent).toContain("the store refused"),
    );
    expect(saveButton()).toBeDefined();
    expect(chipNames()).toEqual(["Carl Sagan"]);
  });

  // Spec U30/3: Restore still says NOTHING about the authors. It restores the document's own title and
  // leaves the reader's list alone, so the argument must not travel — `undefined` is what
  // `JSON.stringify` drops, and that is exactly the distinction the command reads.
  it("says nothing about the authors when the reader restores the document's own title", async () => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    const document_: LibraryEntry = {
      ...entry(),
      title: "My own title",
      userTitle: "My own title",
      authors: ["Carl Sagan"],
      readerAuthors: ["Carl Sagan"],
    };
    component = mountEditor(document_);
    expect(chipNames()).toEqual(["Carl Sagan"]);

    // R16: the control is an icon with its own accessible name, so the name is how it is found.
    const restore = window.document.body.querySelector<HTMLButtonElement>(
      `button[aria-label="${english("card.titleRestore")}"]`,
    );
    expect(restore, "the restore control exists for a title the reader set").toBeDefined();
    restore!.click();
    flushSync();

    // R18: it asks first, and nothing is written by the question itself.
    expect(window.document.body.textContent).toContain(english("card.restoreConfirmTitle"));
    expect(saves()).toHaveLength(0);

    const confirm = [...window.document.body.querySelectorAll("button")].find(
      (button) => button.textContent?.trim() === english("card.restoreConfirmAction"),
    );
    expect(confirm, "the confirmation offers the action").toBeDefined();
    confirm!.click();
    await vi.waitFor(() => expect(saves()).toHaveLength(1));

    expect(saves()).toHaveLength(1);
    // What actually crosses the bridge is the serialised arguments: no `authors` key at all.
    expect(JSON.stringify(saves()[0][1])).toBe(JSON.stringify({ id: document_.id, title: null }));
  });

  // U30/R42/S3, pressed for real: the list the reader built BY HAND reaches the store in their order,
  // and the removal took out exactly the chip it named.
  it("adds a typed name and a listed one, removes the first, and saves the rest in order", async () => {
    const document_ = prepare(["Jorge Luis Borges", "Ursula K. Le Guin"]);

    typeAuthor("Carl Sagan");
    expect(chipNames()).toEqual(["Carl Sagan"]);

    chooseFromList("Ursula K. Le Guin");

    // A third name, typed, so that what remains after the removal is MORE than one: "Ursula" then
    // "Ann" is exactly the order the reader gave, and no sort could produce it — with a single name
    // left, an assertion about order could not fail however the list were ordered.
    typeAuthor("Ann Druyan");
    expect(chipNames()).toEqual(["Carl Sagan", "Ursula K. Le Guin", "Ann Druyan"]);

    removeControl("Carl Sagan")?.click();
    flushSync();
    expect(chipNames()).toEqual(["Ursula K. Le Guin", "Ann Druyan"]);

    saveButton()?.click();
    await vi.waitFor(() => expect(saveButton()).toBeUndefined());

    // ONE command, carrying the chips' order — not the library's order, not alphabetical.
    expect(saves()).toHaveLength(1);
    expect(saves()[0]).toEqual([
      "set_document_naming",
      {
        id: document_.id,
        title: document_.title,
        authors: ["Ursula K. Le Guin", "Ann Druyan"],
      },
    ]);
  });

  // The two refusals, at the surface where they matter: a blank adds nothing and the field keeps what
  // was typed, and a name already on the list is not added twice — the existing chip keeps its place.
  it("refuses a blank and a duplicate, and leaves what the reader typed in the field", () => {
    prepare();

    typeAuthor("   ");
    expect(chipNames()).toEqual([]);
    expect(authorAddField()?.value).toBe("   ");

    typeAuthor("Carl Sagan");
    expect(chipNames()).toEqual(["Carl Sagan"]);

    typeAuthor("  carl sagan ");
    expect(chipNames()).toEqual(["Carl Sagan"]);
    expect(authorAddField()?.value).toBe("  carl sagan ");
  });

  // R18: Cancelar no longer closes the editor — it discards the draft and leaves the reader where they
  // were, which is the only reading that makes sense once the pair is hidden whenever there is nothing
  // to discard. Nothing reaches the store either way.
  it("discards the draft and keeps the editor open when the reader cancels", () => {
    prepare();
    typeAuthor("Carl Sagan");
    expect(chipNames()).toEqual(["Carl Sagan"]);

    cancelButton()?.click();
    flushSync();

    // The dialog is still on screen, the chips are back to what the store holds, and the pair is gone
    // because there is nothing left to save or discard.
    expect(window.document.body.textContent).toContain(english("card.documentEditor"));
    expect(chipNames()).toEqual([]);
    expect(authorAddField()?.value).toBe("");
    expect(saveButton(), "nothing left to save").toBeUndefined();
    expect(saves()).toHaveLength(0);
  });

  // S2, pressed for real. `datalist` cannot be exercised in WebKitGTK, so the list is a select beside
  // the field; choosing a name there adds a chip, and Save still sends ONE command carrying it.
  it("adds the name chosen from the list as a chip, and saves it in the same single write", async () => {
    const document_ = prepare(["Ursula K. Le Guin", "Jorge Luis Borges"]);
    // The pointer path: focus the field, click the name in the list it opens.
    chooseFromList("Jorge Luis Borges");

    // The chip carries the name, and the add field stays empty: it is how the NEXT name arrives.
    expect(chipNames()).toEqual(["Jorge Luis Borges"]);
    expect(authorAddField()?.value).toBe("");

    saveButton()?.click();
    await vi.waitFor(() => expect(saveButton()).toBeUndefined());

    // …and that name is what reached the store, in the one and only write.
    expect(saves()).toHaveLength(1);
    expect(saves()[0]).toEqual([
      "set_document_naming",
      { id: document_.id, title: document_.title, authors: ["Jorge Luis Borges"] },
    ]);
  });

  // R16: once a name is on the list, the field stops offering it — a suggestion that would be refused
  // as a duplicate is a dead entry. Removing the chip brings it back, which is the loop the old
  // "reset the select" test was protecting.
  it("stops offering a name that is already on the list, and offers it again once removed", () => {
    prepare(["Jorge Luis Borges"]);

    openChoices();
    expect(offeredNames()).toContain("Jorge Luis Borges");

    chooseFromList("Jorge Luis Borges");
    expect(chipNames()).toEqual(["Jorge Luis Borges"]);

    // Still focused, the list now has nothing left to offer for this document.
    expect(offeredNames()).not.toContain("Jorge Luis Borges");

    removeControl("Jorge Luis Borges")?.click();
    flushSync();
    expect(chipNames()).toEqual([]);

    openChoices();
    expect(offeredNames()).toContain("Jorge Luis Borges");
  });

  // R16: the keyboard path and the pointer path must land on the same value. ArrowDown highlights the
  // first name and Enter takes it — without typing a single character.
  it("takes a highlighted name with the arrow keys and Enter", () => {
    prepare(["Ursula K. Le Guin"]);

    openChoices();
    const field = authorField()!;
    field.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));
    flushSync();
    field.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    flushSync();

    expect(chipNames()).toEqual(["Ursula K. Le Guin"]);
    expect(authorAddField()?.value).toBe("");
  });

  // R16: typing narrows the list to what matches — the whole point of a combobox over a select.
  it("narrows the list as the reader types", () => {
    prepare(["Jorge Luis Borges", "Ursula K. Le Guin", "Ann Druyan"]);

    openChoices();
    const field = authorField()!;
    field.value = "bor";
    field.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();

    expect(offeredNames()).toEqual(["Jorge Luis Borges"]);
  });

  // R17: the user's report was that changing the cover left the old picture in the editor. The
  // preview and the card are refreshed by the same answer now, and this presses that link: the store
  // is asked for the cover again after the page changes, and the image on screen is the new one.
  it("refreshes the cover preview after the page changes", async () => {
    prepare();
    const cover = "data:image/png;base64,AAAA";
    invoke.mockImplementation(async (command: string) =>
      command === "document_cover" ? cover : null,
    );

    // R18: the page save appears only when the page differs from the stored one.
    const page = window.document.body.querySelector<HTMLInputElement>(
      `input[aria-label="${english("card.coverPageLabel")}"]`,
    );
    expect(page, "the editor renders the page field").toBeDefined();
    page!.value = "3";
    page!.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();

    const savePage = [...window.document.body.querySelectorAll("button")].find(
      (button) => button.textContent?.trim() === english("card.coverPageSave"),
    );
    expect(savePage, "the editor offers the page save").toBeDefined();
    savePage!.click();

    await vi.waitFor(() => {
      const image = window.document.body.querySelector(".cover-preview img");
      expect(image?.getAttribute("src")).toBe(cover);
    });
  });
});

/**
 * R19, pressed for real: which cover source is in force, and the control that switches it back.
 *
 * The user's report was that an image, once chosen, could not be replaced by a page, and that after
 * replacing it there was no way back to the image without choosing the file again. Both halves are
 * exercised here: the button that offers the saved image, and the page button that takes over while
 * the image stays stored.
 */
describe("the cover's two sources", () => {
  let component: ReturnType<typeof mount> | null = null;

  afterEach(() => {
    if (component !== null) {
      unmount(component);
      component = null;
    }
    window.document.body.innerHTML = "";
  });

  it("brings the stored image back without choosing it again", async () => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    const document_: LibraryEntry = {
      ...entry(),
      coverPage: 2,
      coverImage: "mine.png",
      coverSource: "page",
    };
    component = mountEditor(document_);
    flushSync();

    const useImage = [...window.document.body.querySelectorAll("button")].find(
      (button) => button.textContent?.trim() === english("card.coverUseImage"),
    );
    expect(useImage, "the editor offers the saved image").toBeDefined();
    useImage!.click();

    await vi.waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("use_cover_image", { id: document_.id }),
    );
  });

  // R20: the whole loop the user asked for, pressed. The page button appears while the image is in
  // force, disappears the moment the page takes over, comes back when the page number changes, and
  // comes back again when the image returns.
  it("hides the page button once the page takes over, and brings it back with the image", async () => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    const document_: LibraryEntry = {
      ...entry(),
      coverPage: 2,
      coverImage: "mine.png",
      coverSource: "image",
    };
    component = mountEditor(document_);
    flushSync();

    const button = (label: string) =>
      [...window.document.body.querySelectorAll("button")].find(
        (candidate) => candidate.textContent?.trim() === label,
      );

    expect(button(english("card.coverPageSave")), "the page can take over").toBeDefined();
    button(english("card.coverPageSave"))!.click();

    // Gone: the store holds the page now, and the image is still stored beside it.
    await vi.waitFor(() => expect(button(english("card.coverPageSave"))).toBeUndefined());
    expect(invoke).toHaveBeenCalledWith("set_cover_page", { id: document_.id, page: 2 });
    expect(button(english("card.coverUseImage")), "the image can come back").toBeDefined();

    // A different number is a new change, so the button returns.
    const page = window.document.body.querySelector<HTMLInputElement>(
      `input[aria-label="${english("card.coverPageLabel")}"]`,
    );
    expect(page, "the editor renders the page field").toBeDefined();
    page!.value = "5";
    page!.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    expect(button(english("card.coverPageSave")), "a new page is a change").toBeDefined();

    // Save it, then take the image back: the page button returns because the image is in force.
    button(english("card.coverPageSave"))!.click();
    await vi.waitFor(() => expect(button(english("card.coverPageSave"))).toBeUndefined());
    button(english("card.coverUseImage"))!.click();
    await vi.waitFor(() =>
      expect(button(english("card.coverPageSave")), "the page can take over again").toBeDefined(),
    );
  });

  it("does not offer the saved image while the image is already the cover", () => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    component = mountEditor({
      ...entry(),
      coverPage: 2,
      coverImage: "mine.png",
      coverSource: "image",
    });
    flushSync();

    const useImage = [...window.document.body.querySelectorAll("button")].find(
      (button) => button.textContent?.trim() === english("card.coverUseImage"),
    );
    expect(useImage).toBeUndefined();

    // And the page button IS offered, because taking the cover back to a page is always meaningful
    // while an image is in force — even with the number unchanged.
    const usePage = [...window.document.body.querySelectorAll("button")].find(
      (button) => button.textContent?.trim() === english("card.coverPageSave"),
    );
    expect(usePage, "the page can take over").toBeDefined();
  });
});

/**
 * R15. The carousel's arrows are its controls.
 *
 * The user asked for arrows on Inicio's carousels and for the scrollbar under them to go. The
 * scrollbar is CSS; the arrows are behaviour, and behaviour that no server-side render can check:
 * this mounts a strip, gives it the widths a real layout would give it, presses both arrows and
 * asserts the distance and the direction. happy-dom has no layout, so the geometry is stated here
 * explicitly rather than guessed from the engine.
 */
describe("the carousel arrows", () => {
  let component: ReturnType<typeof mount> | null = null;

  afterEach(() => {
    if (component !== null) {
      unmount(component);
      component = null;
    }
    window.document.body.innerHTML = "";
  });

  function mountStrip(): { grid: HTMLElement; scrollBy: ReturnType<typeof vi.fn> } {
    component = mount(BookGrid, {
      target: window.document.body,
      props: {
        entries: [entry(), { ...entry(), id: 8, path: "/library/eight.pdf" }],
        t: english,
        onOpenFolder: () => {},
        onOpenDocument: () => {},
        layout: "strip",
      },
    });
    flushSync();

    const grid = window.document.querySelector("ul.grid") as HTMLElement;
    const scrollBy = vi.fn();
    grid.scrollBy = scrollBy as unknown as typeof grid.scrollBy;
    // One tile is 168 px wide and the strip's gap is 12 px, so one step is 180 px.
    (grid.firstElementChild as HTMLElement).getBoundingClientRect = () =>
      ({ width: 168 }) as DOMRect;
    Object.defineProperty(grid, "clientWidth", { value: 400, configurable: true });
    Object.defineProperty(grid, "scrollWidth", { value: 800, configurable: true });
    // The strip measured itself at mount, when happy-dom reported a zero-width box; asking it to
    // measure again is what a real scroll event does, and it is what enables the next arrow.
    grid.dispatchEvent(new Event("scroll"));
    flushSync();
    return { grid, scrollBy };
  }

  it("moves the strip by one tile when the next arrow is pressed", () => {
    const { scrollBy } = mountStrip();

    const next = window.document.querySelector(".strip-arrow.next") as HTMLButtonElement;
    next.click();

    expect(scrollBy).toHaveBeenCalledTimes(1);
    expect(scrollBy.mock.calls[0][0]).toMatchObject({ left: 180 });
  });

  it("refuses a step that would leave the strip where it already is", () => {
    const { grid, scrollBy } = mountStrip();

    // Both ends are visible at once, and the previous arrow is disabled until the strip has moved.
    const previous = window.document.querySelector(".strip-arrow.prev") as HTMLButtonElement;
    expect(previous.disabled).toBe(true);
    previous.click();
    expect(scrollBy).not.toHaveBeenCalled();

    // Once scrolled to the end, the next arrow is the disabled one and the previous one works.
    grid.scrollLeft = 400;
    grid.dispatchEvent(new Event("scroll"));
    flushSync();

    const next = window.document.querySelector(".strip-arrow.next") as HTMLButtonElement;
    expect(next.disabled).toBe(true);
    expect(previous.disabled).toBe(false);
    previous.click();
    expect(scrollBy).toHaveBeenCalledTimes(1);
    expect(scrollBy.mock.calls[0][0]).toMatchObject({ left: -180 });
  });
});

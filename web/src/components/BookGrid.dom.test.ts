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
function pencil(): HTMLButtonElement | undefined {
  const button = window.document.body.querySelector("button.edit-document");
  return button === null ? undefined : (button as HTMLButtonElement);
}

/**
 * The editor's author list, found by its accessible name — the same way the reader finds it.
 *
 * It used to be located by one of the names among its options, which quietly tied every test to that
 * one name: the co-author test hands a different library and found no list at all.
 */
function authorList(): HTMLSelectElement | undefined {
  const select = window.document.body.querySelector(
    `select[aria-label="${english("card.authorChoose")}"]`,
  );
  return select === null ? undefined : (select as HTMLSelectElement);
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
    expect(saveButton(), "the editor renders a Save button").toBeDefined();

    saveButton()?.click();

    // Wait for the successful save's own observable end — the editor closes — before counting. Without
    // this the count raced the second call a double write would make, and a version of this test that
    // passed against two writes proved nothing: the first correction of this file did exactly that.
    await vi.waitFor(() => expect(saveButton()).toBeUndefined());

    // One command carrying BOTH facts. Written as two commands it was two writes, and a failure
    // between them left the title saved while the screen implied nothing had been (L55).
    expect(saves()).toHaveLength(1);
    expect(saves()[0]).toEqual([
      "set_document_naming",
      { id: document_.id, title: document_.title, authors: [] },
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

    const restore = [...window.document.body.querySelectorAll("button")].find(
      (button) => button.textContent?.trim() === english("card.titleRestore"),
    );
    expect(restore, "the restore control exists for a title the reader set").toBeDefined();
    restore!.click();
    await vi.waitFor(() => expect(saveButton()).toBeUndefined());

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

    const list = authorList();
    expect(list, "the editor draws a list of the library's authors").toBeDefined();
    list!.value = "Ursula K. Le Guin";
    list!.dispatchEvent(new Event("change", { bubbles: true }));
    flushSync();

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

  it("discards the chips and the typed name when the reader cancels", () => {
    prepare();
    typeAuthor("Carl Sagan");
    expect(chipNames()).toEqual(["Carl Sagan"]);

    cancelButton()?.click();
    flushSync();

    // Cancel discards the whole draft: the editor is gone, nothing reached the store…
    expect(saveButton()).toBeUndefined();
    expect(chipNames()).toEqual([]);
    expect(saves()).toHaveLength(0);

    // …and reopening the editor seeds from the document again, not from the draft that was thrown
    // away — the document has no author of its own, so there is no chip and the field is empty.
    pencil()?.click();
    flushSync();
    expect(saveButton()).toBeDefined();
    expect(chipNames()).toEqual([]);
    expect(authorAddField()?.value).toBe("");
  });

  // S2, pressed for real. `datalist` cannot be exercised in WebKitGTK, so the list is a select beside
  // the field; choosing a name there adds a chip, and Save still sends ONE command carrying it.
  it("adds the name chosen from the list as a chip, and saves it in the same single write", async () => {
    const document_ = prepare(["Ursula K. Le Guin", "Jorge Luis Borges"]);
    const list = authorList();
    expect(list, "the editor draws a list of the library's authors").toBeDefined();

    // Chosen the way a browser reports it: set the value, then send the `change` that travels up.
    list!.value = "Jorge Luis Borges";
    list!.dispatchEvent(new Event("change", { bubbles: true }));
    flushSync();

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

  // The list is a picker, never a second copy of the value: it goes back to its "choose an author"
  // entry, so a reader who removes a chip can pick the same name again and a real browser sends the
  // `change` that adds it (L70).
  it("returns the list to its choose-an-author entry, so a removed name can be added again", () => {
    prepare(["Jorge Luis Borges"]);

    authorList()!.value = "Jorge Luis Borges";
    authorList()!.dispatchEvent(new Event("change", { bubbles: true }));
    flushSync();
    expect(authorList()?.value).toBe("");
    expect(chipNames()).toEqual(["Jorge Luis Borges"]);

    removeControl("Jorge Luis Borges")?.click();
    flushSync();
    expect(chipNames()).toEqual([]);

    authorList()!.value = "Jorge Luis Borges";
    authorList()!.dispatchEvent(new Event("change", { bubbles: true }));
    flushSync();
    expect(chipNames()).toEqual(["Jorge Luis Borges"]);
    expect(authorList()?.value).toBe("");
  });
});

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
    author: null,
    userAuthor: null,
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

function mountEditor(document_: LibraryEntry): ReturnType<typeof mount> {
  const component = mount(BookGrid, {
    target: window.document.body,
    props: {
      entries: [document_],
      t: english,
      onOpenFolder: () => {},
      onOpenDocument: () => {},
      initialEditing: document_,
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

  function prepare(): LibraryEntry {
    // Inside the desktop shell, which is the only place a save can happen: the real guard reads this.
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    const document_ = entry();
    component = mountEditor(document_);
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
      { id: document_.id, title: document_.title, author: null },
    ]);
  });

  it("keeps the editor open and says so when the write fails", async () => {
    prepare();
    invoke.mockRejectedValueOnce(new Error("the store refused the write"));

    saveButton()?.click();

    // The failure is visible in the reader's own words, and the editor is still there — closing it
    // would claim a save that did not happen.
    await vi.waitFor(() =>
      expect(window.document.body.textContent).toContain("the store refused"),
    );
    expect(saveButton()).toBeDefined();
  });
});

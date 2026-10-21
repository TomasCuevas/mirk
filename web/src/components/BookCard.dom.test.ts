import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import BookCard from "./BookCard.svelte";
import { translate } from "../lib/i18n";
import type { LibraryEntry } from "../lib/library";

/**
 * The card's open action, pressed for real (R37).
 *
 * The rest of the card's suite renders it server-side, which can see the markup but cannot press
 * anything: the HTML is produced and nothing runs. That left the one link nobody could fail — the
 * click that has to reach the application — so a future edit could drop the handler, or attach it
 * to the wrong element, with the whole suite still green. This file closes that hole with the same
 * simulated browser the editor's save test uses (L64).
 */

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
    progress: null,
  };
}

describe("the card opens the document", () => {
  let component: ReturnType<typeof mount> | null = null;
  const onOpenDocument = vi.fn();

  afterEach(() => {
    if (component !== null) {
      unmount(component);
      component = null;
    }
    window.document.body.innerHTML = "";
    onOpenDocument.mockReset();
  });

  function render(): LibraryEntry {
    const document_ = entry();
    component = mount(BookCard, {
      target: window.document.body,
      props: {
        entry: document_,
        t: english,
        onOpenFolder: () => {},
        onOpenDocument,
        onEditDocument: () => {},
        cover: null,
      },
    });
    flushSync();
    return document_;
  }

  function control(className: string): HTMLButtonElement | null {
    return window.document.body.querySelector<HTMLButtonElement>(`.${className}`);
  }

  it("opens the document when the cover is clicked", () => {
    const document_ = render();

    control("cover-open")?.click();
    flushSync();

    expect(onOpenDocument).toHaveBeenCalledTimes(1);
    expect(onOpenDocument).toHaveBeenCalledWith(document_);
  });

  it("opens the document when the title is clicked", () => {
    const document_ = render();

    control("title-open")?.click();
    flushSync();

    expect(onOpenDocument).toHaveBeenCalledTimes(1);
    expect(onOpenDocument).toHaveBeenCalledWith(document_);
  });

  it("leaves the folder and the editor alone: they are different actions", () => {
    render();

    control("open-folder")?.click();
    control("edit-document")?.click();
    flushSync();

    // Neither of the corner controls opens the document: they are about the document's folder and
    // its metadata, and a click on them must not also open the file.
    expect(onOpenDocument).not.toHaveBeenCalled();
  });
});

/**
 * R15. A finished document is a state, not a number like any other.
 *
 * The user asked for the 100 % pill to be wider and differently coloured so the end of a book is
 * obvious at a glance. What is pinned here is the class that carries that decision, because the
 * colour lives in the stylesheet and the DECISION — this card is finished — must come from
 * `describeProgressView`, the one place that decides what a fraction means.
 */
describe("a finished document says so", () => {
  let component: ReturnType<typeof mount> | null = null;

  afterEach(() => {
    if (component !== null) {
      unmount(component);
      component = null;
    }
    window.document.body.innerHTML = "";
  });

  function withProgress(page: number, total: number): void {
    component = mount(BookCard, {
      target: window.document.body,
      props: {
        entry: { ...entry(), progress: { page, total, source: "native", confidence: "exact", recordedAt: null, readAt: null } },
        t: english,
        onOpenFolder: () => {},
        onOpenDocument: () => {},
        onEditDocument: () => {},
        cover: null,
      },
    });
    flushSync();
  }

  it("marks the 100 % pill as complete", () => {
    withProgress(9, 10);

    const marker = window.document.querySelector(".marker");
    expect(marker?.textContent?.trim()).toBe("100%");
    expect(marker?.classList.contains("complete")).toBe(true);
  });

  it("leaves a document in progress without the complete pill", () => {
    withProgress(4, 10);

    const marker = window.document.querySelector(".marker");
    expect(marker?.textContent?.trim()).toBe("50%");
    expect(marker?.classList.contains("complete")).toBe(false);
  });
});

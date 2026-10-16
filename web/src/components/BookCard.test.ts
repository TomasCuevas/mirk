import { render } from "svelte/server";
import { describe, expect, it } from "vitest";
import BookCard from "./BookCard.svelte";
import { translate } from "../lib/i18n";
import type { LibraryEntry } from "../lib/library";

const english = translate("en");

function entry(overrides: Partial<LibraryEntry> = {}): LibraryEntry {
  return {
    id: 1,
    path: "/library/document.pdf",
    title: "Cosmos",
    userTitle: null,
    author: null,
    pages: 300,
    progress: null,
    ...overrides,
  };
}

/**
 * R9 (L22): the containing-folder action is an icon at the top of the card, with an accessible name
 * instead of a visible label.
 *
 * The writer who implemented this reported that the markup could not be tested without adding a
 * dependency. It can: `svelte/server` renders the component to HTML in the node environment the suite
 * already uses, so no DOM and nothing new is needed. A rule that nothing checks is a rule that drifts,
 * and this one has already moved once — the action used to be a text button at the bottom.
 */
describe("the containing-folder action", () => {
  it("is one of the card's two controls, at the top, named rather than labelled", () => {
    const { body } = render(BookCard, {
      props: {
        entry: entry(),
        t: english,
        status: null,
        onOpenFolder: () => {},
        onEditTitle: () => {},
      },
    });

    const card = body.slice(body.indexOf("<article"), body.indexOf("</article>"));
    // R31 added the title-edit pill on the cover's other corner, so the folder action is one of two
    // controls; its own shape and name are what this test pins.
    expect(card.match(/<button/g)).toHaveLength(2);

    const start = card.indexOf("open-folder");
    const buttonStart = card.lastIndexOf("<button", start);
    const button = card.slice(
      buttonStart,
      card.indexOf("</button>", start) + "</button>".length,
    );

    // Anchored to the cover, not in a row of its own: the first attempt gave the action its own row
    // above the cover and the user rejected it as looking loose there (L27).
    expect(card.indexOf("cover-wrap")).toBeLessThan(start);
    expect(start).toBeLessThan(card.indexOf('class="title'));
    // An icon is not a name, so the name is stated for assistive technology…
    expect(button).toContain('aria-label="Open containing folder"');
    // …and the drawing is hidden from it, so the name is not read out twice.
    expect(button).toContain('aria-hidden="true"');
    // Icon only: strip the drawing and every tag, and nothing readable is left.
    expect(button.replace(/<svg[\s\S]*?<\/svg>/g, "").replace(/<[^>]*>/g, "").trim()).toBe("");
  });

  it("says nothing at all about the folder action, in any status", () => {
    const props = { entry: entry(), t: english, onOpenFolder: () => {}, onEditTitle: () => {} };
    // Compared with Svelte's own SSR comment markers stripped: those are an implementation detail of how
    // a block renders, not what the reader is told — an earlier version of this test failed on them.
    const said = (html: string) => html.replace(/<!--[\s\S]*?-->/g, "");
    const quiet = said(render(BookCard, { props: { ...props, status: null } }).body);

    // L27 removed the green "folder opened" sentence, and L30 removed the failure sentence too, so the
    // card carries no status text in either case. Recorded as the user's decision, not as a preference
    // of the maintainer's: the folder window opening is its own confirmation, and a failure is silent.
    expect(
      said(
        render(BookCard, {
          props: { ...props, status: { kind: "ok", folder: "/tmp", viewer: "Files" } },
        }).body,
      ),
    ).toBe(quiet);
    expect(
      said(
        render(BookCard, {
          props: { ...props, status: { kind: "error", message: "no such folder" } },
        }).body,
      ),
    ).toBe(quiet);
  });
});

/**
 * R31: the reader can set a title, and the card says whether the title it shows is theirs.
 *
 * The pill mirrors the folder action on the cover's other corner, and the note appears only when the
 * reader's title is the one displayed — the distinction is the whole point of keeping the reader's
 * title apart from the document's own.
 */
describe("the reader's title", () => {
  it("puts the title-edit pill on the cover's top-left, named after the document", () => {
    const { body } = render(BookCard, {
      props: {
        entry: entry({ title: "Cosmos" }),
        t: english,
        status: null,
        onOpenFolder: () => {},
        onEditTitle: () => {},
      },
    });

    const card = body.slice(body.indexOf("<article"), body.indexOf("</article>"));
    const start = card.indexOf("edit-title");
    const buttonStart = card.lastIndexOf("<button", start);
    const button = card.slice(
      buttonStart,
      card.indexOf("</button>", start) + "</button>".length,
    );

    // On the cover, before the folder action, so the two corners do not swap or overlap.
    expect(card.indexOf("cover-wrap")).toBeLessThan(start);
    expect(start).toBeLessThan(card.indexOf("open-folder"));
    // The accessible name names the document rather than saying a bare "edit".
    expect(button).toContain('aria-label="Edit the title of Cosmos"');
    expect(button).toContain('aria-hidden="true"');
    expect(button.replace(/<svg[\s\S]*?<\/svg>/g, "").replace(/<[^>]*>/g, "").trim()).toBe("");
  });

  it("shows the reader's title without printing a note about whose it is", () => {
    // The user asked for the note to go: on a title they had just typed themselves it was noise. What
    // replaces it is in the EDITOR — the field says whose title is being edited, and the restore control
    // exists only when there is one to restore — which `titleEditor` keeps in one place and
    // `library.test.ts` pins. This test is the inverse guard: the phrase must not come back onto a card.
    const props = { t: english, status: null, onOpenFolder: () => {}, onEditTitle: () => {} };

    const named = render(BookCard, {
      props: { ...props, entry: entry({ title: "Mine", userTitle: "Mine" }) },
    }).body;
    expect(named).toContain(">Mine<");
    expect(named).not.toContain("your title");
    expect(named).not.toContain(english("card.titleLabelYours"));
  });
});

/**
 * R32: a card shows the document's real cover when the application has one, and its existing
 * placeholder otherwise.
 *
 * The card asks the application for its cover on mount, and SSR does not run lifecycle hooks, so a
 * server render always shows the placeholder — which is what the first test pins. Passing `cover`
 * directly is how the image branch is pinned without a webview, and an empty string is the case
 * that would otherwise become `src=""`.
 */
describe("the card's cover", () => {
  const props = { t: english, status: null, onOpenFolder: () => {}, onEditTitle: () => {} };

  it("shows its placeholder, and no image, until there is a cover", () => {
    const { body } = render(BookCard, { props: { ...props, entry: entry() } });

    expect(body).toContain(english("card.cover"));
    expect(body).not.toContain("<img");
  });

  it("shows the real cover, with no placeholder, when the application has one", () => {
    const source = "data:image/png;base64,AAAA";
    const { body } = render(BookCard, {
      props: { ...props, entry: entry(), cover: source },
    });

    expect(body).toContain(`src="${source}"`);
    expect(body).not.toContain(english("card.cover"));
  });

  it("keeps the placeholder when the answer is no cover at all", () => {
    // An empty string is not an image: `src=""` would ask the browser for the page itself.
    const { body } = render(BookCard, {
      props: { ...props, entry: entry(), cover: "" },
    });

    expect(body).toContain(english("card.cover"));
    expect(body).not.toContain("<img");
  });
});

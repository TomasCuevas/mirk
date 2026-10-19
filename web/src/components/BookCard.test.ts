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
    userAuthor: null,
    pages: 300,
    progress: null,
    ...overrides,
  };
}

/**
 * How many controls the card carries with a given class.
 *
 * The class attribute also carries Svelte's scoping class, so a plain `class="cover-open"` never
 * matches: the name is looked for INSIDE the attribute, the way the markup really spells it.
 */
function controlsWith(card: string, className: string): number {
  const matches = card.match(new RegExp(`class="[^"]*\\b${className}\\b[^"]*"`, "g"));
  return matches === null ? 0 : matches.length;
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
        onOpenFolder: () => {},
        onOpenDocument: () => {},
        onEditDocument: () => {},
      },
    });

    const card = body.slice(body.indexOf("<article"), body.indexOf("</article>"));
    // R31 added the document-editor pill on the cover's other corner, and R37 added the two ways to
    // OPEN the document — the cover and the title. The corner controls are still the folder action
    // and the pencil: a third pill over a small cover was the user's complaint (R32) and it has not
    // come back. The cover and the title are not pills on the cover; they are the cover and the
    // title themselves.
    expect(card.match(/<button/g)).toHaveLength(4);
    expect(controlsWith(card, "open-folder")).toBe(1);
    expect(controlsWith(card, "edit-document")).toBe(1);

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
});

/**
 * R31: the reader can set a title, and the card says whether the title it shows is theirs.
 *
 * The pill mirrors the folder action on the cover's other corner, and the note appears only when the
 * reader's title is the one displayed — the distinction is the whole point of keeping the reader's
 * title apart from the document's own.
 */
describe("the reader's title", () => {
  it("puts the document-editor pill on the cover's top-left, named after the document", () => {
    const { body } = render(BookCard, {
      props: {
        entry: entry({ title: "Cosmos" }),
        t: english,
        onOpenFolder: () => {},
        onOpenDocument: () => {},
        onEditDocument: () => {},
      },
    });

    const card = body.slice(body.indexOf("<article"), body.indexOf("</article>"));
    const start = card.indexOf("edit-document");
    const buttonStart = card.lastIndexOf("<button", start);
    const button = card.slice(
      buttonStart,
      card.indexOf("</button>", start) + "</button>".length,
    );

    // On the cover, before the folder action, so the two corners do not swap or overlap.
    expect(card.indexOf("cover-wrap")).toBeLessThan(start);
    expect(start).toBeLessThan(card.indexOf("open-folder"));
    // The accessible name names the DOCUMENT rather than only its title, because the one modal it
    // opens edits the title AND the cover.
    expect(button).toContain('aria-label="Edit the document Cosmos"');
    expect(button).toContain('aria-hidden="true"');
    expect(button.replace(/<svg[\s\S]*?<\/svg>/g, "").replace(/<[^>]*>/g, "").trim()).toBe("");
  });

  it("names the document it edits in both languages", () => {
    const inLocale = (locale: "en" | "es", expected: string) => {
      const { body } = render(BookCard, {
        props: {
          entry: entry({ title: "Cosmos" }),
          t: translate(locale),
          onOpenFolder: () => {},
          onOpenDocument: () => {},
          onEditDocument: () => {},
        },
      });
      expect(body, locale).toContain(`aria-label="${expected}"`);
    };

    // The pencil edits the document, not only its title, and the Spanish says so in the neutral
    // register this project is unifying on — never voseo.
    inLocale("en", "Edit the document Cosmos");
    inLocale("es", "Editar el documento Cosmos");
  });

  it("shows the reader's title without printing a note about whose it is", () => {
    // The user asked for the note to go: on a title they had just typed themselves it was noise. What
    // replaces it is in the EDITOR — the field says whose title is being edited, and the restore control
    // exists only when there is one to restore — which `titleEditor` keeps in one place and
    // `library.test.ts` pins. This test is the inverse guard: the phrase must not come back onto a card.
    const props = {
      t: english,
      onOpenFolder: () => {},
      onOpenDocument: () => {},
      onEditDocument: () => {},
    };

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
  const props = {
    t: english,
    onOpenFolder: () => {},
    onOpenDocument: () => {},
    onEditDocument: () => {},
  };

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

/**
 * R32, corrected by R37: the cover's CORNER carries no control of its own.
 *
 * A third control over a small cover was the user's complaint — "another icon was added over the
 * cover" — and the pencil already means "edit this document". The cover's
 * page, its image and the removal of that image now live inside the modal the pencil opens, and what
 * is left on the cover states facts: the page count at the bottom-right, and nothing else. This test
 * is the inverse guard that keeps the control from coming back.
 *
 * R37 changed what the cover IS, not what its corner holds: the cover became the way to open the
 * document, and it is the cover's own rectangle rather than a pill in one of its corners. The
 * complaint this test guards against — a control laid OVER the cover — is still what it forbids.
 */
describe("the cover's own corner", () => {
  const props = {
    t: english,
    onOpenFolder: () => {},
    onOpenDocument: () => {},
    onEditDocument: () => {},
  };

  it("carries no control, only the page badge", () => {
    const { body } = render(BookCard, { props: { ...props, entry: entry({ title: "Cosmos" }) } });
    const card = body.slice(body.indexOf("<article"), body.indexOf("</article>"));

    expect(card).not.toContain("edit-cover");
    // The two corner controls are the document-editor pencil and the folder action; the badge is
    // a fact, not a third control.
    expect(controlsWith(card, "open-folder")).toBe(1);
    expect(controlsWith(card, "edit-document")).toBe(1);
    expect(card).toContain("cover-pages");
    // R37: the cover itself is now a control — the document's own body, opened by clicking it.
    // It is a different thing from a corner pill, and it carries no icon: it IS the cover.
    expect(controlsWith(card, "cover-open")).toBe(1);
  });
});

/**
 * R37: the card opens the document, from the cover and from the title.
 *
 * The reader asked for it: clicking the card opens the PDF in the system's own application for it.
 * The two ways in are the cover — the document's body — and the title — the document's name — and
 * both say the same thing, so both name the document: a control that says only "open" says nothing
 * about which of the dozens of cards on the screen it opens.
 */
describe("the card opens the document", () => {
  const props = {
    t: english,
    onOpenFolder: () => {},
    onOpenDocument: () => {},
    onEditDocument: () => {},
  };

  it("offers the cover and the title as the two ways in, both naming the document", () => {
    const { body } = render(BookCard, { props: { ...props, entry: entry({ title: "Cosmos" }) } });
    const card = body.slice(body.indexOf("<article"), body.indexOf("</article>"));

    // The cover carries the accessible name, because it holds no text of its own.
    expect(controlsWith(card, "cover-open")).toBe(1);
    expect(card).toContain('aria-label="Open Cosmos"');

    // The title IS the name, so it needs no second one: the button's own text is its name.
    const titleStart = card.indexOf("title-open");
    expect(titleStart).toBeGreaterThan(-1);
    const title = card.slice(titleStart, card.indexOf("</button>", titleStart));
    expect(title).toContain("Cosmos");
    expect(title).not.toContain("aria-label");
  });
});

/**
 * R38: the fill is coloured by how far along the reading is, and the bar is not.
 *
 * The user asked for the PROGRESS to change colour — red, orange, yellow, green — and for the bar to
 * keep its own. So the band lives on the fill's class, the track keeps its neutral background, and the
 * number on the bar still states the percentage: the colour is a second channel, never the only one.
 */
describe("the progress fill's colour", () => {
  const props = {
    t: english,
    onOpenFolder: () => {},
    onOpenDocument: () => {},
    onEditDocument: () => {},
  };

  function cardAt(page: number, total: number): string {
    const { body } = render(BookCard, {
      props: {
        ...props,
        entry: entry({
          progress: {
            page,
            total,
            source: "native",
            confidence: "exact",
            recordedAt: null,
            readAt: null,
          },
        }),
      },
    });
    return body.slice(body.indexOf("<article"), body.indexOf("</article>"));
  }

  it("colours the fill, not the track, and keeps the number as the statement", () => {
    const card = cardAt(90, 100);

    // The band is a class on the FILL — the element that stands for what was read. It is found by its
    // class and not by the word `fill`: the pencil's drawing carries `fill="none"` and would be hit
    // first, which is exactly how the first version of this test passed a string it never meant.
    const fillStart = card.indexOf('class="fill');
    expect(fillStart).toBeGreaterThan(-1);
    const fill = card.slice(fillStart, card.indexOf("</span>", fillStart));
    expect(fill).toContain("band-nearly");

    // The track's own element carries no band: the user asked for the progress to change colour.
    const bar = card.slice(card.indexOf('class="bar'), card.indexOf("</div>"));
    expect(bar).not.toContain("band-");

    // And the number is still there, so the colour is redundant information rather than the only one.
    // Page 90 of 100 reads 91 %: the card counts pages from one, so the fraction is (page + 1) / total.
    expect(card).toContain("91%");
  });

  it("moves through the four bands as the reading advances", () => {
    expect(cardAt(10, 100)).toContain("band-started");
    expect(cardAt(30, 100)).toContain("band-some");
    expect(cardAt(60, 100)).toContain("band-most");
    expect(cardAt(80, 100)).toContain("band-nearly");
  });
});

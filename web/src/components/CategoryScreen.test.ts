import { render } from "svelte/server";
import { describe, expect, it } from "vitest";
import CategoryScreen from "./CategoryScreen.svelte";
import { translate } from "../lib/i18n";
import type { HomeCategory, LibraryEntry } from "../lib/library";
import { categoryDefinition, categoryTitle } from "../lib/screens";

const english = translate("en");

function categoryProps(
  overrides: { category?: HomeCategory; entries?: LibraryEntry[]; inFlight?: boolean } = {},
) {
  return {
    category: "inProgress" as HomeCategory,
    entries: [] as LibraryEntry[],
    inFlight: false,
    error: null as string | null,
    t: english,
    onOpenFolder: () => {},
    onOpenDocument: () => {},
    onBack: () => {},
    ...overrides,
  };
}

/**
 * The category screen correction. The screen has to name the category and its definition, and it has
 * to offer a way back that does not depend on the bottom navigation.
 */
describe("a category's own screen", () => {
  it("names the category and states its definition (B4)", () => {
    const { body } = render(CategoryScreen, { props: categoryProps() });

    expect(body).toContain(categoryTitle("inProgress", english));
    expect(body).toContain(categoryDefinition("inProgress", english));
  });

  it("offers a back control at the top, named from the catalogue", () => {
    // The control returns to Inicio by calling the navigation App already uses for the navbar, so
    // there is no second navigation mechanism to invent here — the screen only renders the control.
    const { body } = render(CategoryScreen, { props: categoryProps() });

    const back = body.slice(body.indexOf("<button"), body.indexOf("</button>") + 9);
    expect(back).toContain(`>${english("category.back")}<`);
    // Above the heading, so it reads as a way back rather than another action on the list.
    expect(body.indexOf("<button")).toBeLessThan(body.indexOf("<h1"));
  });
});

/**
 * L47 on the category screen. It is the last screen that still decided on its own: it rendered its
 * empty sentence while a scan was in flight, which is the same false "there is nothing" the user saw
 * on Inicio after re-adding a watched folder. It now obeys the same pure decision as Inicio and
 * Biblioteca.
 *
 * The empty sentence this screen renders is `home.categoryEmpty`, which is NOT the one Biblioteca
 * uses — so these assertions name that sentence, not `library.empty`.
 */
describe("a category's empty state while work is in flight (L47)", () => {
  it("shows the loader, not the category's empty sentence, while a scan is in flight", () => {
    expect(english("home.categoryEmpty")).not.toBe(english("library.empty"));

    const { body } = render(CategoryScreen, { props: categoryProps({ inFlight: true }) });

    expect(body).toContain(english("library.loading"));
    expect(body).not.toContain(english("home.categoryEmpty"));
  });

  it("says the category is empty once nothing is in flight", () => {
    const { body } = render(CategoryScreen, { props: categoryProps({ inFlight: false }) });

    expect(body).toContain(english("home.categoryEmpty"));
    expect(body).not.toContain(english("library.loading"));
  });

  it("never covers a category that already has content with a loader", () => {
    // Content wins: the silent fifteen-second re-read and a scan of another folder both run while
    // documents are already on this screen, and covering them would be the flicker the rule forbids.
    const reading: LibraryEntry = {
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
    const { body } = render(CategoryScreen, {
      props: categoryProps({ entries: [reading], inFlight: true }),
    });

    expect(body).not.toContain(english("library.loading"));
    expect(body).not.toContain(english("home.categoryEmpty"));
    expect(body).toMatch(/<article/g);
  });
});

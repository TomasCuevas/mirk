import { render } from "svelte/server";
import { describe, expect, it } from "vitest";
import CategoryScreen from "./CategoryScreen.svelte";
import { translate } from "../lib/i18n";
import type { HomeCategory, LibraryEntry } from "../lib/library";
import { categoryDefinition, categoryTitle } from "../lib/screens";

const english = translate("en");

function categoryProps(overrides: { category?: HomeCategory } = {}) {
  return {
    category: "inProgress" as HomeCategory,
    entries: [] as LibraryEntry[],
    loading: false,
    error: null as string | null,
    t: english,
    folderStatus: null,
    onOpenFolder: () => {},
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

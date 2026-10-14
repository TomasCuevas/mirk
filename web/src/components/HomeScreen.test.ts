import { render } from "svelte/server";
import { describe, expect, it } from "vitest";
import HomeScreen from "./HomeScreen.svelte";
import { translate } from "../lib/i18n";
import { homeCategories, type LibraryEntry } from "../lib/library";
import { categoryDefinition, categoryTitle } from "../lib/screens";

const english = translate("en");

function homeProps(overrides: { entries?: LibraryEntry[] } = {}) {
  return {
    entries: [] as LibraryEntry[],
    loading: false,
    error: null as string | null,
    t: english,
    folderStatus: null,
    onOpenFolder: () => {},
    onOpenCategory: () => {},
    ...overrides,
  };
}

/**
 * R2 and R4 (B1–B4). The carousel decisions are pure and tested in `library.test.ts`; what this pins
 * is the screen itself — that nothing explanatory is left in the header, that all three categories
 * are present with their definitions, that an empty carousel says so, and that each one carries the
 * button that opens its full list.
 */
describe("Inicio", () => {
  it("keeps its header to what names the screen, and nothing else (R2)", () => {
    const { body } = render(HomeScreen, { props: homeProps() });

    // Everything before the first carousel is the header, and it holds the heading and no prose.
    const header = body.slice(body.indexOf("<h1"), body.indexOf('class="carousel'));
    expect(header).toContain(">Home<");
    expect(header).not.toContain("<p");
  });

  it("shows all three carousels, each stating its own definition (B1, B4)", () => {
    const { body } = render(HomeScreen, { props: homeProps() });

    for (const category of homeCategories) {
      expect(body).toContain(categoryTitle(category, english));
      expect(body).toContain(categoryDefinition(category, english));
    }
  });

  it("gives an empty carousel the card's cover shape without reserving its text block (B1)", () => {
    // One in-progress document fills the first carousel, so this render holds a real card and the two
    // empty slots at once — which is what makes the shapes directly comparable.
    const entry: LibraryEntry = {
      id: 1,
      path: "/library/a.pdf",
      title: "A",
      author: null,
      pages: 10,
      progress: { page: 0, total: 10, source: "native", confidence: "exact", recordedAt: null },
    };
    const { body } = render(HomeScreen, { props: homeProps({ entries: [entry] }) });

    // A real card is drawn once and the other two carousels state they are empty...
    expect(body.match(/<article/g)).toHaveLength(1);
    expect(body.split(english("home.categoryEmpty")).length - 1).toBe(homeCategories.length - 1);

    const styleOfClass = (name: string): string | null => {
      for (const tag of body.match(/<[^>]*>/g) ?? []) {
        const classes = tag.match(/class="([^"]*)"/)?.[1]?.split(/\s+/) ?? [];
        if (classes.includes(name)) {
          return tag.match(/style="([^"]*)"/)?.[1] ?? null;
        }
      }
      return null;
    };

    // ...and each empty slot carries the card's cover shape, not a second design. The text block below
    // is deliberately NOT reserved at a fixed height: reserving it was what made the card look
    // stretched and left the message floating in the emptiness it created, so an empty carousel is
    // now shorter than one holding cards (L31).
    expect(body.split('class="empty-cover').length - 1).toBe(homeCategories.length - 1);
    expect(styleOfClass("empty-cover")).toBe(styleOfClass("cover"));
    expect(styleOfClass("empty-cover")).toBe("aspect-ratio: 9 / 11");
    expect(styleOfClass("empty-body")).toBe(styleOfClass("body"));
    expect(styleOfClass("empty-body")).toBeNull();
    // The tile width is one value, declared on each carousel and read by both the real card's strip
    // and the empty slot.
    expect(body.split('style="--tile: 13rem"').length - 1).toBe(homeCategories.length);
  });

  it("puts a full-list button at the top of every carousel (B2)", () => {
    const { body } = render(HomeScreen, { props: homeProps() });

    expect(body.split(english("home.openCategory")).length - 1).toBe(homeCategories.length);
    for (const category of homeCategories) {
      expect(body).toContain(
        english("home.openCategoryLabel", { category: categoryTitle(category, english) }),
      );
    }
  });

  it("lays a carousel out as a sideways-scrolling strip (B1)", () => {
    const entry: LibraryEntry = {
      id: 1,
      path: "/library/a.pdf",
      title: "A",
      author: null,
      pages: 10,
      progress: { page: 0, total: 10, source: "native", confidence: "exact", recordedAt: null },
    };

    const { body } = render(HomeScreen, { props: homeProps({ entries: [entry] }) });

    expect(body).toMatch(/class="grid [^"]*strip"/);
  });
});

import { render } from "svelte/server";
import { describe, expect, it } from "vitest";
import BookGrid from "./BookGrid.svelte";
import { translate } from "../lib/i18n";
import type { LibraryEntry } from "../lib/library";

const english = translate("en");

/** `count` documents in one category's list, which is what a carousel is handed. */
function entries(count: number): LibraryEntry[] {
  return Array.from({ length: count }, (_, index) => ({
    id: index + 1,
    path: `/library/${index}.pdf`,
    title: `Document ${index}`,
    author: null,
    pages: 10,
    progress: { page: 1, total: 10, source: "native", confidence: "exact", recordedAt: null },
  }));
}

/** How many cards the rendering drew. */
function cardsDrawn(count: number, layout: "grid" | "strip"): number {
  const { body } = render(BookGrid, {
    props: {
      entries: entries(count),
      t: english,
      folderStatus: null,
      onOpenFolder: () => {},
      layout,
    },
  });
  return body.match(/<article/g)?.length ?? 0;
}

describe("how many tiles a grid draws", () => {
  // R16 (L37). The user's carousel held every document in its category — "me aparecieron demasiados,
  // tantos que molestaba tener un carrusel tan largo" — so a strip stops at the same number the
  // library paginates by, and "open the full list" is where the rest belongs.
  //
  // This test exists because the cap was once computed and then never used, and the build stayed green
  // while the carousel kept drawing everything: the rule had no test, so nothing could fail.
  it("caps a carousel at ten tiles however many the category holds", () => {
    expect(cardsDrawn(25, "strip")).toBe(10);
  });

  // And the cap must not leak into a full list: a grid draws everything it is given, because the
  // category's own screen is the one place that is supposed to be complete.
  it("draws every entry when the list is not a carousel", () => {
    expect(cardsDrawn(25, "grid")).toBe(25);
  });
});

import { render } from "svelte/server";
import { describe, expect, it } from "vitest";
import BookCard from "./BookCard.svelte";
import { translate } from "../lib/i18n";
import type { LibraryEntry } from "../lib/library";

const english = translate("en");

function entry(): LibraryEntry {
  return {
    id: 1,
    path: "/library/document.pdf",
    title: "Cosmos",
    author: null,
    pages: 300,
    progress: null,
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
  it("is the card's only control, at the top, named rather than labelled", () => {
    const { body } = render(BookCard, {
      props: { entry: entry(), t: english, status: null, onOpenFolder: () => {} },
    });

    const card = body.slice(body.indexOf("<article"), body.indexOf("</article>"));
    const button = card.slice(card.indexOf("<button"), card.indexOf("</button>"));

    // One control only, so the full-width text button that used to sit at the bottom is gone.
    expect(card.match(/<button/g)).toHaveLength(1);
    // Anchored to the cover, not in a row of its own: the first attempt gave the action its own row
    // above the cover and the user rejected it as looking loose there (L27).
    expect(card.indexOf('class="cover-wrap"')).toBeLessThan(card.indexOf("<button"));
    expect(card.indexOf("<button")).toBeLessThan(card.indexOf('class="title'));
    // An icon is not a name, so the name is stated for assistive technology…
    expect(button).toContain('aria-label="Open containing folder"');
    // …and the drawing is hidden from it, so the name is not read out twice.
    expect(button).toContain('aria-hidden="true"');
    // Icon only: strip the drawing and every tag, and nothing readable is left.
    expect(button.replace(/<svg[\s\S]*?<\/svg>/g, "").replace(/<[^>]*>/g, "").trim()).toBe("");
  });

  it("says nothing at all about the folder action, in any status", () => {
    const props = { entry: entry(), t: english, onOpenFolder: () => {} };
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

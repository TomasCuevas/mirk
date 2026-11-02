import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it } from "vitest";
import SuggestionsScreen from "./SuggestionsScreen.svelte";
import { translate } from "../lib/i18n";
import type { DetectorBelief, Suggestion } from "../lib/suggestions";

/**
 * The suggestions screen, mounted in a simulated browser (S1–S7 of `odd/tasks/suggestions-screen.md`).
 *
 * Everything else about this screen is a pure function with its own test — the search, the order, the page
 * cut. What only this file can see is the wiring: that typing in the box really filters the cards, that
 * the pager really moves the window of the queue, that the two decisions on a card really reach the shell,
 * and — the one that must never regress — that no control on this screen can edit a suggestion (S6).
 *
 * Every assertion is about what the reader sees or does. Nothing reaches into the component's state, and
 * the one non-visual check — which commands a card asks the desktop for — is the observable half of S6:
 * "no command is exposed that would let a suggestion be edited" is a statement about the command surface,
 * and the only way to pin it is to look at what was called.
 */

const english = translate("en");

function suggestion(id: number, name: string, historyAt: number | null): Suggestion {
  return {
    id,
    path: `/library/${name}`,
    seenCount: 2,
    firstSeenAt: 1_776_000_000,
    historyAt,
  };
}

/** A queue whose history dates run the same way as its ids, so "most recent first" reads as 11 → 1. */
function queue(count: number): Suggestion[] {
  return Array.from({ length: count }, (_, index) =>
    suggestion(index + 1, `doc-${String(index + 1).padStart(2, "0")}.pdf`, index + 1),
  );
}

describe("the suggestions screen", () => {
  let component: ReturnType<typeof mount> | null = null;
  const commands: string[] = [];
  const decided: { id: number; accept: boolean }[] = [];
  const scrolled: number[] = [];
  /** How many times the reader asked to go back to Biblioteca (L10). */
  const backs: number[] = [];

  afterEach(() => {
    if (component !== null) {
      unmount(component);
      component = null;
    }
    commands.length = 0;
    decided.length = 0;
    scrolled.length = 0;
    backs.length = 0;
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
    delete (window as unknown as Record<string, unknown>).__TAURI_EVENT_PLUGIN_INTERNALS__;
    window.document.body.innerHTML = "";
  });

  // L10: the reader's own report — "no tiene un boton para volver a la biblioteca directamente" — so the
  // screen answers the question itself instead of sending them to the bar. The destination is named, not
  // guessed: the arrow is decoration and the text plus aria-label say where the click goes.
  it("offers a way back to Biblioteca, and uses it once", () => {
    render({ suggestions: [] });

    const back = window.document.querySelector("button.back");
    expect(back?.textContent?.trim()).toBe(english("suggestions.back"));
    expect(back?.getAttribute("aria-label")).toBe(english("suggestions.backLabel"));

    (back as HTMLElement).click();
    flushSync();

    expect(backs).toHaveLength(1);
  });

  /**
   * The desktop the cards talk to: one command with an answer, so a cover really arrives and nothing else
   * is invented. Anything a card asked for is recorded, which is how S6's second half is checked.
   */
  function fakeDesktop(): void {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {
      invoke: (command: string) => {
        commands.push(command);
        if (command === "suggestion_cover") {
          return Promise.resolve("data:image/png;base64,AAA");
        }
        return Promise.resolve(null);
      },
      transformCallback: () => 1,
      unregisterCallback: () => {},
      unregisterListener: () => {},
      convertFileSrc: (path: string) => path,
    };
    (window as unknown as Record<string, unknown>).__TAURI_EVENT_PLUGIN_INTERNALS__ = {
      unregisterListener: () => {},
    };
  }

  function render(
    options: {
      suggestions?: Suggestion[] | null;
      readError?: string | null;
      decisionError?: string | null;
      historyError?: string | null;
      detector?: DetectorBelief;
    } = {},
  ): void {
    // A test can render the same screen more than once (the three states, then the failure); the previous
    // mount is torn down first, so nothing from one state survives into the next.
    if (component !== null) {
      unmount(component);
      component = null;
      window.document.body.innerHTML = "";
    }
    fakeDesktop();
    component = mount(SuggestionsScreen, {
      target: window.document.body,
      props: {
        suggestions: options.suggestions ?? null,
        readError: options.readError ?? null,
        decisionError: options.decisionError ?? null,
        historyError: options.historyError ?? null,
        detector: options.detector ?? "installed",
        deciding: false,
        locale: "en",
        layout: "columns-2",
        t: english,
        onDecide: async (id: number, accept: boolean) => {
          decided.push({ id, accept });
        },
        onOpenFolder: () => {},
        onResetScroll: () => {
          scrolled.push(1);
        },
        onBack: () => {
          backs.push(1);
        },
      },
    });
    flushSync();
  }

  /** The heading of every card on screen, in the order they are drawn. */
  function titles(): string[] {
    return Array.from(window.document.querySelectorAll("article h2")).map(
      (heading) => heading.textContent?.trim() ?? "",
    );
  }

  /** Waits for the per-card cover answer to land, which travels through the command bridge. */
  async function settle(): Promise<void> {
    await new Promise((resolve) => setTimeout(resolve, 20));
    flushSync();
  }

  // S1: one card per suggestion, in the reader's own order — the history's date, newest first (S4).
  it("draws one card per suggestion, most recently opened first", () => {
    render({ suggestions: [suggestion(1, "older.pdf", 1_700_000_000), suggestion(2, "newer.pdf", 1_760_000_000)] });

    expect(titles()).toEqual(["newer", "older"]);
    // The preset title (S5) and, beside it, the path as the card's context datum.
    expect(window.document.body.textContent).toContain("/library/newer.pdf");
  });

  it("shows the cover the desktop produced for the card", async () => {
    render({ suggestions: [suggestion(1, "cosmos.pdf", 1)] });
    await settle();

    const cover = window.document.querySelector("article img");
    expect(cover?.getAttribute("src")).toBe("data:image/png;base64,AAA");
    expect(commands).toContain("suggestion_cover");
  });

  // S3: the search reads the preset title first and the path after it, case- and accent-blind.
  it("filters the cards by title and by path, ignoring case and accents", () => {
    render({
      suggestions: [
        suggestion(1, "Cosmos.pdf", 3),
        suggestion(2, "Anónimo.pdf", 2),
        suggestion(3, "Descargas/contact.pdf", 1),
      ],
    });

    const search = window.document.querySelector('input[type="search"]') as HTMLInputElement;
    const type = (text: string): void => {
      search.value = text;
      search.dispatchEvent(new Event("input", { bubbles: true }));
      flushSync();
    };

    type("anonimo");
    expect(titles()).toEqual(["Anónimo"]);

    // "Descargas" is only in the path: the title of that suggestion is "contact".
    type("descargas");
    expect(titles()).toEqual(["contact"]);

    type("");
    expect(titles()).toEqual(["Cosmos", "Anónimo", "contact"]);
  });

  it("names the search that emptied the grid", () => {
    render({ suggestions: [suggestion(1, "cosmos.pdf", 1)] });

    const search = window.document.querySelector('input[type="search"]') as HTMLInputElement;
    search.value = "borges";
    search.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();

    expect(titles()).toEqual([]);
    expect(window.document.body.textContent).toContain(
      english("suggestions.noMatches", { query: "borges" }),
    );
  });

  // S2: the queue pages with the library's own cut, and the pager is not drawn when there is nothing to turn.
  it("turns a page, and asks the shell for the top of the screen", () => {
    render({ suggestions: queue(11) });

    // Ten per page in a two-column window, so the newest ten fill page one and the oldest waits on page two.
    expect(titles()).toHaveLength(10);
    expect(titles()[0]).toBe("doc-11");

    const next = window.document.querySelector('[aria-label="Next page"]') as HTMLButtonElement;
    next.click();
    flushSync();

    expect(titles()).toEqual(["doc-01"]);
    expect(scrolled).toHaveLength(1);
  });

  it("draws no pager while the queue fits on one page", () => {
    render({ suggestions: queue(10) });

    expect(titles()).toHaveLength(10);
    expect(window.document.querySelector("nav.pager")).toBeNull();
  });

  // S7: the two decisions still mean what they meant, and they are on the card.
  it("records the two decisions from the card", () => {
    render({ suggestions: [suggestion(7, "cosmos.pdf", 1)] });

    // S6/L57: the two decisions are icons, so each control's accessible name is the only thing that says
    // what it does, and it names the document it acts on. These assertions came with the cards from the
    // panel test that used to pin them in Ajustes; the panel is gone, the rule is not.
    const accept = window.document.querySelector("article .accept");
    const discard = window.document.querySelector("article .discard");
    expect(accept?.getAttribute("aria-label")).toBe(
      english("suggestion.acceptLabel", { path: "/library/cosmos.pdf" }),
    );
    expect(discard?.getAttribute("aria-label")).toBe(
      english("suggestion.discardLabel", { path: "/library/cosmos.pdf" }),
    );

    (window.document.querySelector("article .accept") as HTMLButtonElement).click();
    flushSync();
    (window.document.querySelector("article .discard") as HTMLButtonElement).click();
    flushSync();

    expect(decided).toEqual([
      { id: 7, accept: true },
      { id: 7, accept: false },
    ]);
  });

  /**
   * S6, which is the rule this screen exists to keep: nothing about a suggestion is editable.
   *
   * The card shows a title and a cover and offers two decisions; there is no field for the title, no
   * chips for authors (a suggestion has none), no cover or page editor, and — the half a rendered DOM
   * cannot show on its own — no command is asked for that would perform one. The order selector is the
   * only combobox, and the only text field is the search box, which filters the view rather than the data.
   */
  it("offers no way to edit a suggestion, and no command that would", async () => {
    render({ suggestions: [suggestion(1, "cosmos.pdf", 1)] });
    await settle();

    // A title or author field would be a textbox; a cover or page picker would be a second combobox.
    expect(window.document.querySelector('input[type="text"]')).toBeNull();
    expect(window.document.querySelector("textarea")).toBeNull();
    expect(window.document.querySelector("[contenteditable]")).toBeNull();
    expect(window.document.querySelectorAll("select")).toHaveLength(1);
    expect(window.document.querySelectorAll("input")).toHaveLength(1);

    // The accessible name of every control says what it does, and none of them edits anything.
    const names = Array.from(window.document.querySelectorAll("button")).map(
      (button) => button.getAttribute("aria-label") ?? button.textContent?.trim() ?? "",
    );
    for (const name of names) {
      expect(name).not.toMatch(/edit|rename|change cover|page count/i);
    }

    // Nothing beyond the cover and the two decisions was ever asked for.
    expect(new Set(commands)).toEqual(new Set(["suggestion_cover"]));
  });

  // S1/L56: three states, never two — and "cannot tell" is not "nothing is waiting".
  it("keeps still-reading, nothing-waiting and no-detector apart", () => {
    render({ suggestions: null });
    expect(window.document.body.textContent).toContain(english("suggestion.loading"));

    render({ suggestions: [] });
    expect(window.document.body.textContent).toContain(english("suggestion.none"));

    render({ suggestions: [], detector: "unsupported" });
    expect(window.document.body.textContent).toContain(english("suggestion.noDetector"));
    expect(window.document.body.textContent).not.toContain(english("suggestion.none"));
  });

  /**
   * S10/L61: a history that could not be read is said beside the list, never instead of it — and never
   * twice, because a platform with no detector already has its own sentence for the same absence.
   */
  it("states a failed history read beside the queue, and leaves the detector's sentence alone", () => {
    render({ suggestions: [suggestion(1, "cosmos.pdf", 1)], historyError: "permission denied" });

    expect(window.document.body.textContent).toContain(
      english("suggestions.historyError", { message: "permission denied" }),
    );
    // The queue is still there to decide about: the source failing does not hide the cards.
    expect(titles()).toEqual(["cosmos"]);

    render({ suggestions: [], detector: "unsupported", historyError: "not supported" });
    expect(window.document.body.textContent).not.toContain("permission denied");
    expect(window.document.body.textContent).toContain(english("suggestion.noDetector"));
  });

  it("replaces the grid with the failure when the queue itself could not be read", () => {
    render({ suggestions: [], readError: "the store is locked" });

    expect(window.document.body.textContent).toContain(
      english("suggestion.readError", { message: "the store is locked" }),
    );
    // T4: a read that failed is not a decision that was attempted, so it does not borrow the decision's
    // sentence — that one says "the decision could not be recorded", which would be false here.
    expect(window.document.body.textContent).not.toContain(
      english("suggestion.error", { message: "the store is locked" }),
    );
    expect(window.document.body.textContent).not.toContain(english("suggestion.none"));
  });

  /**
   * T4's copy debt, the other half. The two failures are two facts and they keep two sentences: a decision
   * that was not recorded still says so, and never claims the queue could not be read.
   */
  it("states a decision that was not recorded with the decision's own sentence", () => {
    render({ suggestions: [], decisionError: "the file is gone" });

    expect(window.document.body.textContent).toContain(
      english("suggestion.error", { message: "the file is gone" }),
    );
    expect(window.document.body.textContent).not.toContain(
      english("suggestion.readError", { message: "the file is gone" }),
    );
  });
});

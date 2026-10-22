import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it } from "vitest";
import App from "../App.svelte";
import { DEFAULT_WINDOW_LAYOUT, gridColumns } from "../lib/window";

/**
 * The scroll position of the shell, exercised in a simulated browser.
 *
 * The user's report: *"si se ha hecho scroll en una pantalla y luego se navega a otra, esta nueva
 * pantalla no resetea el scroll, permanece con el scroll ya desplazado"*. The shell has ONE scrolling
 * container — the screen between the top bar and the navbar — so its `scrollTop` outlives the screen
 * that filled it unless navigation resets it. No other test can see this: the server render used
 * elsewhere produces HTML and runs nothing, and a pure function cannot hold a scroll offset.
 *
 * The app is mounted OUTSIDE the desktop shell on purpose (`runningInTauri()` is false with no global
 * set): that is the state in which it renders its screens without asking a store for a listing, which
 * is all this file needs. What it asserts is the reader's experience — go somewhere, scroll, navigate,
 * and land at the top of the new screen.
 */

const shell = {
  mount(): ReturnType<typeof mount> {
    return mount(App, { target: window.document.body });
  },
};

/** The one scrolling container: the screen between the top bar and the navbar. */
function scroller(): HTMLElement {
  const main = window.document.querySelector("main");
  if (main === null) {
    throw new Error("the shell renders its scrolling screen");
  }
  return main as HTMLElement;
}

/** Scrolls the shell the way a reader does, then answers whether the value really stayed. */
function scrollTo(offset: number): void {
  const main = scroller();
  main.scrollTop = offset;
  flushSync();
  // A simulated browser keeps the property it was given; the assertion below is what proves the reset.
  expect(main.scrollTop).toBe(offset);
}

describe("a new screen starts at its own top", () => {
  let component: ReturnType<typeof mount> | null = null;

  afterEach(() => {
    if (component !== null) {
      unmount(component);
      component = null;
    }
    window.document.body.innerHTML = "";
    window.location.hash = "";
  });

  it("resets the scroll when the reader navigates to another screen", () => {
    component = shell.mount();
    flushSync();

    scrollTo(640);

    // A navbar click writes the hash and the app answers it; the hash listener is the one real path.
    window.location.hash = "#/library";
    window.dispatchEvent(new HashChangeEvent("hashchange"));
    flushSync();

    expect(scroller().scrollTop).toBe(0);
  });

  it("resets it for a sub-screen too, which is a screen of its own (R43)", () => {
    component = shell.mount();
    flushSync();

    scrollTo(320);

    window.location.hash = "#/settings/reading";
    window.dispatchEvent(new HashChangeEvent("hashchange"));
    flushSync();

    expect(scroller().scrollTop).toBe(0);
  });

  it("keeps the reader's place while nothing navigates", () => {
    // The reset must be a RESPONSE to navigation, not a periodic rewind: a reader scrolling a long
    // library must not be pulled back to the top by anything else.
    component = shell.mount();
    flushSync();

    scrollTo(240);
    flushSync();

    expect(scroller().scrollTop).toBe(240);
  });
});

/**
 * S4: the grid's column count is a property of the shell, not of each screen.
 *
 * Every screen that lists documents hands its entries to the same grid, so the layout is published once
 * on the shell's own element and the grid reads it. Without this test the rule would live only in a CSS
 * file, where nothing can fail when the wiring is forgotten and the window widens while the cards stay
 * at two per row.
 */
describe("the window layout reaches the grid", () => {
  let component: ReturnType<typeof mount> | null = null;

  afterEach(() => {
    if (component !== null) {
      unmount(component);
      component = null;
    }
    window.document.body.innerHTML = "";
    window.location.hash = "";
  });

  it("publishes the layout in force as the grid's column count", () => {
    component = shell.mount();
    flushSync();

    const app = window.document.querySelector(".app");
    expect(app).not.toBeNull();
    expect((app as HTMLElement).style.getPropertyValue("--grid-columns")).toBe(
      gridColumns(DEFAULT_WINDOW_LAYOUT),
    );
  });
});

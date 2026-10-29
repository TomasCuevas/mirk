import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it } from "vitest";
import App from "../App.svelte";
import { DEFAULT_WINDOW_LAYOUT, gridColumns } from "../lib/window";
import { translate } from "../lib/i18n";

const english = translate("en");

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

/**
 * S3 of `odd/tasks/navigation.md`: the stored layout decides where the navigation lives.
 *
 * The decision itself is a pure rule (`navMode`, tested in `window.test.ts`) and the two arrangements
 * are pinned in `Navbar.test.ts`; what only this file can see is that the shell wires the rule to the
 * component — that full screen renders the sidebar and no bottom bar, and that a windowed layout keeps
 * the bar. The fake desktop process answers the startup reads so the layout really arrives from the
 * store, which is the same path the running application takes.
 */
describe("the navigation follows the window layout", () => {
  let component: ReturnType<typeof mount> | null = null;

  afterEach(() => {
    if (component !== null) {
      unmount(component);
      component = null;
    }
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
    delete (window as unknown as Record<string, unknown>).__TAURI_EVENT_PLUGIN_INTERNALS__;
    window.document.body.innerHTML = "";
    window.location.hash = "";
  });

  function fakeDesktop(layout: string): void {
    const answers: Record<string, unknown> = {
      get_language: null,
      get_window_layout: layout,
      list_library: [],
      progress_providers: [],
      watched_folders: [],
      list_suggestions: [],
      suggestion_detector: { id: "none", availability: "unsupported" },
      "plugin:event|listen": 1,
      "plugin:event|unlisten": null,
    };
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {
      invoke: (command: string) =>
        command in answers
          ? Promise.resolve(answers[command])
          : Promise.reject(new Error(`unmocked command ${command}`)),
      transformCallback: () => 1,
      unregisterCallback: () => {},
      unregisterListener: () => {},
      convertFileSrc: (path: string) => path,
    };
    (window as unknown as Record<string, unknown>).__TAURI_EVENT_PLUGIN_INTERNALS__ = {
      unregisterListener: () => {},
    };
    // `@tauri-apps/api/event` unregisters through its own global, separate from the command bridge.
    (window as unknown as Record<string, unknown>).__TAURI_EVENT_PLUGIN_INTERNALS__ = {
      unregisterListener: () => {},
    };
  }

  async function settle(): Promise<void> {
    // The layout arrives through `invoke`, which loads the Tauri API module first: two promise hops and
    // a timer are enough for it to land and for the shell to re-render.
    await new Promise((resolve) => setTimeout(resolve, 20));
    flushSync();
  }

  it("keeps the bottom bar in a windowed layout", async () => {
    // R7 of `odd/tasks/desktop-chrome.md` made full screen the default, so a windowed layout is now a
    // stored choice rather than the state with nothing stored: the fake desktop answers with one.
    fakeDesktop("columns-3");
    component = shell.mount();
    await settle();

    const nav = window.document.querySelector("nav");
    expect(nav?.className).toContain("bottom");
    expect(nav?.className).not.toContain("side");
    // L15: a windowed layout keeps the shipped root, so nothing about its type or spacing moves.
    expect(window.document.documentElement.style.fontSize).toBe("16px");
  });

  it("renders the left sidebar and no bottom bar in full screen", async () => {
    fakeDesktop("auto");
    component = shell.mount();
    await settle();

    const nav = window.document.querySelector("nav");
    expect(nav?.className).toContain("side");
    expect(nav?.className).not.toContain("bottom");
    // The sidebar is a column of the shell, beside the screen rather than on top of it.
    expect(window.document.querySelector("main")?.parentElement?.className).toContain("side");
    expect(window.document.querySelectorAll("nav").length).toBe(1);

    // L10/L15: full screen keeps cards at the fixed layouts' size instead of shrinking them to fit more,
    // and that size follows the larger root the layout draws on.
    const app = window.document.querySelector(".app") as HTMLElement;
    expect(Number.parseFloat(app.style.getPropertyValue("--grid-card-min"))).toBeCloseTo(
      (194.2 * 18) / 16,
      3,
    );
    expect(window.document.documentElement.style.fontSize).toBe("18px");
  });
});

/**
 * The top bar's three controls reach the desktop process.
 *
 * The buttons are the only way to reach those actions from the interface, and a handler that quietly
 * stops invoking them looks exactly like a window manager that ignores the request. This pins the
 * invocation — the command each control names — so the wiring cannot rot unnoticed.
 */
describe("the window controls", () => {
  let component: ReturnType<typeof mount> | null = null;
  const invoked: string[] = [];
  const calls: { command: string; args?: unknown }[] = [];

  afterEach(() => {
    if (component !== null) {
      unmount(component);
      component = null;
    }
    invoked.length = 0;
    calls.length = 0;
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
    delete (window as unknown as Record<string, unknown>).__TAURI_EVENT_PLUGIN_INTERNALS__;
    window.document.body.innerHTML = "";
    window.location.hash = "";
  });

  function fakeDesktop(extra: Record<string, unknown> = {}): void {
    const answers: Record<string, unknown> = {
      get_language: null,
      get_window_layout: null,
      list_library: [],
      progress_providers: [],
      watched_folders: [],
      list_suggestions: [],
      suggestion_detector: { id: "none", availability: "unsupported" },
      "plugin:event|listen": 1,
      "plugin:event|unlisten": null,
      ...extra,
    };
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {
      invoke: (command: string, args?: unknown) => {
        invoked.push(command);
        calls.push({ command, args });
        return Promise.resolve(command in answers ? answers[command] : null);
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

  it("invokes the command each control names", async () => {
    fakeDesktop();
    component = shell.mount();
    await new Promise((resolve) => setTimeout(resolve, 20));
    flushSync();

    const controls = window.document.querySelectorAll("header button");
    expect(controls.length).toBe(4);
    for (const control of Array.from(controls)) {
      (control as HTMLButtonElement).click();
    }
    await new Promise((resolve) => setTimeout(resolve, 20));

    expect(invoked).toContain("minimize_window");
    expect(invoked).toContain("hide_window");
    expect(invoked).toContain("close_app");
  });

  // S4/S5 and R7: maximise is not its own window command — it chooses the stored layout. With full
  // screen the stored default, the first press is the restore one, and it lands on three per row.
  it("restores the default to three per row and maximises again from there", async () => {
    fakeDesktop();
    component = shell.mount();
    await new Promise((resolve) => setTimeout(resolve, 20));
    flushSync();

    // R6: minimise, hide, maximise, close — maximise is the third control.
    const maximize = () =>
      Array.from(window.document.querySelectorAll("header button"))[2] as HTMLButtonElement;

    // R7: nothing stored means full screen, so the control offers the way back.
    expect(maximize().getAttribute("aria-label")).toBe(english("window.restore"));

    maximize().click();
    await new Promise((resolve) => setTimeout(resolve, 20));
    flushSync();
    expect(calls).toContainEqual({ command: "set_window_layout", args: { layout: "columns-3" } });
    expect(maximize().getAttribute("aria-label")).toBe(english("window.maximize"));

    maximize().click();
    await new Promise((resolve) => setTimeout(resolve, 20));
    flushSync();
    expect(calls).toContainEqual({ command: "set_window_layout", args: { layout: "auto" } });
    expect(maximize().getAttribute("aria-label")).toBe(english("window.restore"));
  });

  // S5: a reader who did choose a preset gets that preset back, not the restore default.
  it("restores the preset the reader chose last", async () => {
    fakeDesktop({ get_window_layout: "columns-4" });
    component = shell.mount();
    await new Promise((resolve) => setTimeout(resolve, 20));
    flushSync();

    const maximize = () =>
      Array.from(window.document.querySelectorAll("header button"))[2] as HTMLButtonElement;

    maximize().click();
    await new Promise((resolve) => setTimeout(resolve, 20));
    flushSync();
    expect(calls).toContainEqual({ command: "set_window_layout", args: { layout: "auto" } });

    maximize().click();
    await new Promise((resolve) => setTimeout(resolve, 20));
    flushSync();
    expect(calls).toContainEqual({ command: "set_window_layout", args: { layout: "columns-4" } });
  });
});

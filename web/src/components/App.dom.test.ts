import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import App from "../App.svelte";
import { DEFAULT_WINDOW_LAYOUT, gridColumns, WINDOW_LAYOUT_CHANGED_EVENT } from "../lib/window";
import { translate } from "../lib/i18n";
import { LIBRARY_REFRESH_MS } from "../lib/library";

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
      get_window_preset: null,
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
      get_window_preset: null,
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

/**
 * S2 and S4: the window's own gestures change the layout, and the interface follows.
 *
 * Two gestures live outside the interface's buttons. A double click on the top bar is the reader asking
 * for the other window state, and dragging the window out of full screen is the compositor unmaximising
 * it — the desktop process rewrites the stored layout in both cases, so the grid must follow either one
 * instead of drawing a layout the window has left.
 *
 * The fakes are the ones this file already uses, with one addition: `transformCallback` keeps the handler
 * the event plugin registers, which is how a test hands the shell the event the process would emit. The
 * `plugin:event|listen` answer was already there for the tray's screen requests.
 */
describe("the layout the window's own gestures change", () => {
  let component: ReturnType<typeof mount> | null = null;
  const calls: { command: string; args?: unknown }[] = [];
  const stored: { layout: string | null; preset: string | null } = { layout: null, preset: null };
  const callbacks = new Map<number, (event: { payload: unknown }) => void>();
  const listeners: { event: string; handler: (event: { payload: unknown }) => void }[] = [];
  let nextCallback = 0;

  afterEach(() => {
    if (component !== null) {
      unmount(component);
      component = null;
    }
    calls.length = 0;
    stored.layout = null;
    stored.preset = null;
    callbacks.clear();
    listeners.length = 0;
    nextCallback = 0;
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
    delete (window as unknown as Record<string, unknown>).__TAURI_EVENT_PLUGIN_INTERNALS__;
    window.document.body.innerHTML = "";
    window.location.hash = "";
  });

  function fakeDesktop(layout: string | null, preset: string | null = null): void {
    stored.layout = layout;
    stored.preset = preset;
    const answers: Record<string, unknown> = {
      get_language: null,
      list_library: [],
      progress_providers: [],
      watched_folders: [],
      list_suggestions: [],
      suggestion_detector: { id: "none", availability: "unsupported" },
      "plugin:event|listen": 1,
      "plugin:event|unlisten": null,
    };
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {
      invoke: (command: string, args?: Record<string, unknown>) => {
        calls.push({ command, args });
        // The store is read on every ask, so a test can change it the way the desktop process does.
        if (command === "get_window_layout") {
          return Promise.resolve(stored.layout);
        }
        // The preset is a second stored fact, resolved by the process: it never answers `auto`.
        if (command === "get_window_preset") {
          return Promise.resolve(stored.preset);
        }
        if (command === "plugin:event|listen") {
          const handler = callbacks.get(Number(args?.handler));
          if (handler !== undefined) {
            listeners.push({ event: String(args?.event), handler });
          }
        }
        return Promise.resolve(command in answers ? answers[command] : null);
      },
      transformCallback: (callback: (event: { payload: unknown }) => void) => {
        nextCallback += 1;
        callbacks.set(nextCallback, callback);
        return nextCallback;
      },
      unregisterCallback: () => {},
      unregisterListener: () => {},
      convertFileSrc: (path: string) => path,
    };
    (window as unknown as Record<string, unknown>).__TAURI_EVENT_PLUGIN_INTERNALS__ = {
      unregisterListener: () => {},
    };
  }

  async function settle(): Promise<void> {
    await new Promise((resolve) => setTimeout(resolve, 20));
    flushSync();
  }

  /** The grid's column count, which is what the shell publishes from the layout it believes in. */
  function columns(): string {
    const app = window.document.querySelector(".app") as HTMLElement | null;
    if (app === null) {
      throw new Error("the shell publishes the layout in force");
    }
    return app.style.getPropertyValue("--grid-columns");
  }

  /** A press on the bar, with the click count a browser reports after a second press. */
  function pressBar(detail: number): void {
    const header = window.document.querySelector("header");
    if (header === null) {
      throw new Error("the bar is the window's handle");
    }
    header.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, button: 0, detail }));
    flushSync();
  }

  /** Announces the layout the desktop process stored, the way it does after a drag out of full screen. */
  function emitLayoutChanged(layout: string): void {
    for (const listener of listeners) {
      if (listener.event === WINDOW_LAYOUT_CHANGED_EVENT) {
        listener.handler({ payload: layout });
      }
    }
  }

  // S4: with nothing stored the window opens full screen, so the double click asks for the restore
  // default — three per row — and it goes through the same stored choice the maximise control makes.
  it("toggles to three per row on a double press while full screen", async () => {
    fakeDesktop(null);
    component = shell.mount();
    await settle();
    expect(columns()).toBe(gridColumns("auto"));

    pressBar(2);
    await settle();

    expect(calls).toContainEqual({ command: "set_window_layout", args: { layout: "columns-3" } });
    expect(columns()).toBe("3");
    // The second press asks for the toggle, so nothing hands the window to the drag loop.
    expect(calls.map((call) => call.command)).not.toContain("start_drag");
  });

  // S3: the layout and the preset are different facts, and the interface reads both. A reader whose
  // stored layout is full screen and whose preset is four per row must get four from the double press,
  // because that is the value the process's own drag would use. Reading only the layout gave three here
  // and four there — the two halves of one gesture disagreeing after a restart.
  it("toggles to the stored preset on a double press while full screen", async () => {
    fakeDesktop("auto", "columns-4");
    component = shell.mount();
    await settle();
    expect(columns()).toBe(gridColumns("auto"));

    pressBar(2);
    await settle();

    expect(calls).toContainEqual({ command: "set_window_layout", args: { layout: "columns-4" } });
    expect(columns()).toBe("4");
  });

  // S4/S5: a preset is in force, so the same gesture asks for full screen.
  it("toggles to full screen on a double press when a preset is in force", async () => {
    fakeDesktop("columns-4");
    component = shell.mount();
    await settle();
    expect(columns()).toBe("4");

    pressBar(2);
    await settle();

    expect(calls).toContainEqual({ command: "set_window_layout", args: { layout: "auto" } });
    expect(columns()).toBe(gridColumns("auto"));
  });

  // S2/T3: dragging the window out of full screen is the compositor's gesture, so the process rewrites
  // the stored layout and announces it. The interface re-reads the store rather than guessing, and a key
  // this build does not know is ignored instead of taken as a layout.
  it("follows a layout the desktop process changed behind the interface", async () => {
    fakeDesktop("auto");
    component = shell.mount();
    await settle();
    expect(columns()).toBe(gridColumns("auto"));

    emitLayoutChanged("columns-9");
    await settle();
    expect(columns()).toBe(gridColumns("auto"));

    stored.layout = "columns-4";
    emitLayoutChanged("columns-4");
    await settle();
    expect(columns()).toBe("4");
  });
});

/**
 * The suggestions screen inside the shell (S1, S8, S10 and S11 of `odd/tasks/suggestions-screen.md`).
 *
 * The screen's own rules are pinned where they live — the pure search, order and page functions, and the
 * mounted screen with its cards. What only the shell can show is what this file checks: that the screen has
 * an address and is NOT a fourth navbar entry (Biblioteca stays marked, per the rule L72 recorded), that the
 * desktop's history is read exactly once per run and before the first queue read, that the count the reader
 * sees comes from the QUEUE and never from what the history read returned, and that `NotSupported` still
 * reaches the reader as a statement about the machine rather than as "nothing is waiting".
 */
describe("the suggestions screen in the shell", () => {
  let component: ReturnType<typeof mount> | null = null;
  const calls: { command: string; args?: unknown }[] = [];

  afterEach(() => {
    if (component !== null) {
      unmount(component);
      component = null;
    }
    calls.length = 0;
    vi.useRealTimers();
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
    delete (window as unknown as Record<string, unknown>).__TAURI_EVENT_PLUGIN_INTERNALS__;
    window.document.body.innerHTML = "";
    window.location.hash = "";
  });

  function fakeDesktop(
    answers: Record<string, unknown> = {},
    rejects: readonly string[] = [],
  ): void {
    const base: Record<string, unknown> = {
      get_language: null,
      get_window_layout: "columns-2",
      get_window_preset: null,
      list_library: [],
      progress_providers: [],
      watched_folders: [],
      list_suggestions: [],
      suggestion_detector: { id: "none", availability: "installed" },
      suggestion_cover: null,
      "plugin:event|listen": 1,
      "plugin:event|unlisten": null,
      ...answers,
    };
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {
      invoke: (command: string, args?: unknown) => {
        calls.push({ command, args });
        if (rejects.includes(command)) {
          return Promise.reject(new Error(`${command} failed`));
        }
        return Promise.resolve(command in base ? base[command] : null);
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

  async function settle(): Promise<void> {
    await new Promise((resolve) => setTimeout(resolve, 20));
    flushSync();
  }

  /** Walks to a hash the way a link, a reload or Back would. */
  async function navigate(hash: string): Promise<void> {
    window.location.hash = hash;
    window.dispatchEvent(new HashChangeEvent("hashchange"));
    await settle();
  }

  const oneSuggestion = {
    id: 1,
    path: "/library/cosmos.pdf",
    seenCount: 2,
    firstSeenAt: 1_776_000_000,
    historyAt: 1_760_000_000,
  };

  it("is addressable at #/suggestions and shows the queue there", async () => {
    fakeDesktop({ list_suggestions: [oneSuggestion] });
    component = shell.mount();
    await settle();

    await navigate("#/suggestions");

    expect(window.document.querySelector("h1")?.textContent).toBe(english("suggestion.heading"));
    expect(window.document.body.textContent).toContain("/library/cosmos.pdf");
  });

  // S8/T3: the way in is Biblioteca, and the access is the reader's own click — not a hash they have to
  // know in advance. This is the only place the two halves meet: the library's access and the shell's
  // navigation, and a broken link between them would leave the screen reachable only by address.
  it("reaches the queue by clicking Biblioteca's access", async () => {
    fakeDesktop({ list_suggestions: [oneSuggestion] });
    component = shell.mount();
    await settle();

    await navigate("#/library");

    const access = window.document.querySelector(".suggestions-entry");
    expect(access?.textContent?.trim()).toBe(english("library.suggestionsPending", { count: 1 }));

    (access as HTMLElement).click();
    await settle();

    expect(window.location.hash).toBe("#/suggestions");
    expect(window.document.querySelector("h1")?.textContent).toBe(english("suggestion.heading"));
  });

  // L10: the reader's second report — "Tengo que cliquear en el navbar inferior para volver a la
  // biblioteca". The queue now answers the question itself, and the answer lands where the reader came
  // from, with the hash updated so Back and a reload agree.
  it("goes back to Biblioteca from the queue, without the bar", async () => {
    fakeDesktop({ list_suggestions: [oneSuggestion] });
    component = shell.mount();
    await settle();

    await navigate("#/suggestions");

    const back = window.document.querySelector("button.back");
    expect(back).not.toBeNull();
    (back as HTMLElement).click();
    await settle();

    expect(window.location.hash).toBe("#/library");
    expect(window.document.querySelector("h1")?.textContent).toBe(english("library.heading"));
  });

  // S7/S9: the decision is made on the screen the access opens, and only there. The screen only calls its
  // prop, so this is the half no screen-level test can see: the press reaches the shell's one decision path,
  // named for the document it acts on.
  it("decides the queue from the screen the access opens", async () => {
    fakeDesktop({ list_suggestions: [oneSuggestion] });
    component = shell.mount();
    await settle();

    await navigate("#/library");
    (window.document.querySelector(".suggestions-entry") as HTMLElement).click();
    await settle();

    (window.document.querySelector("article .accept") as HTMLButtonElement).click();
    await settle();

    expect(calls.filter((call) => call.command === "accept_suggestion")).toEqual([
      { command: "accept_suggestion", args: { id: 1 } },
    ]);
  });

  // S9/L4: the panel left Ajustes → Biblioteca, so what used to be asserted as present there is now
  // asserted as gone — on the same route, with the two blocks that stayed still present, so the assertion
  // cannot pass by the sub-screen failing to render at all.
  it("no longer shows the queue inside Ajustes → Biblioteca", async () => {
    fakeDesktop({ list_suggestions: [oneSuggestion] });
    component = shell.mount();
    await settle();

    await navigate("#/settings/library");

    const text = window.document.body.textContent ?? "";
    expect(text).toContain(english("settings.folders.heading"));
    expect(text).toContain(english("scan.heading"));
    expect(text).not.toContain(english("suggestion.heading"));
    expect(window.document.querySelector("article .accept")).toBeNull();
  });

  // S8 and L72: the screen is not a navbar entry, and Biblioteca is what the bar keeps marked while the
  // reader is on it — a bar that marked nothing would leave them with no answer to "where am I".
  it("adds no navbar entry, and keeps Biblioteca marked", async () => {
    fakeDesktop({ list_suggestions: [oneSuggestion] });
    component = shell.mount();
    await settle();

    await navigate("#/suggestions");

    const nav = window.document.querySelector("nav");
    expect(nav?.querySelectorAll("button")).toHaveLength(3);
    expect(nav?.querySelector('[aria-current="page"]')?.textContent?.trim()).toBe(
      english("nav.library"),
    );
  });

  // S10/T2: the history is read ONCE, before the first queue read. Reading it before the queue is what
  // keeps the screen from drawing the previous run's queue and correcting itself a moment later.
  it("reads the desktop's history once per run, before the first queue read", async () => {
    fakeDesktop();
    component = shell.mount();
    await settle();

    const order = calls.map((call) => call.command);

    expect(order.filter((command) => command === "observe_recent_documents")).toHaveLength(1);
    expect(order.indexOf("observe_recent_documents")).toBeLessThan(
      order.indexOf("list_suggestions"),
    );
  });

  /**
   * The other half of "once per run": the queue is still re-read on the library's own interval, and the
   * history is not. A read per tick would inflate `seen_count` for every document already waiting — the
   * interface inventing evidence that the reader opened something again.
   */
  it("re-reads the queue on the interval and never the history", async () => {
    fakeDesktop();
    vi.useFakeTimers();
    component = shell.mount();
    // The startup reads travel through dynamic imports, so they need a turn of the loop rather than only
    // the microtask queue.
    await vi.advanceTimersByTimeAsync(50);
    flushSync();

    const count = (command: string): number =>
      calls.filter((call) => call.command === command).length;
    expect(count("observe_recent_documents")).toBe(1);
    const queues = count("list_suggestions");

    await vi.advanceTimersByTimeAsync(LIBRARY_REFRESH_MS);
    flushSync();

    expect(count("list_suggestions")).toBeGreaterThan(queues);
    expect(count("observe_recent_documents")).toBe(1);
  });

  /**
   * T1b's constraint, pinned where it can fail: `observe_recent_documents` returns a number, and that number
   * is a diagnostic of the pass — it counts a decided path that spent the pass's window even though nothing
   * was recorded — so it is NOT the size of the queue and never reaches the reader. The count comes from
   * `list_suggestions`, which is the only thing that knows what is still pending.
   */
  it("counts the queue from the queue, never from what the history read returned", async () => {
    fakeDesktop({ list_suggestions: [oneSuggestion], observe_recent_documents: 99 });
    component = shell.mount();
    await settle();

    await navigate("#/suggestions");

    const text = window.document.body.textContent ?? "";
    expect(text).toContain(english("suggestions.countAll", { total: 1 }));
    expect(text).not.toContain("99");
  });

  /**
   * L61: `NotSupported` is distinguishable from "the queue is empty". The screen says what the machine
   * cannot do, not that there is nothing to decide — the two answers are different facts and only one of
   * them is true here.
   */
  it("says the machine cannot tell, rather than saying nothing is waiting", async () => {
    fakeDesktop({ suggestion_detector: { id: "none", availability: "unsupported" } });
    component = shell.mount();
    await settle();

    await navigate("#/suggestions");

    const text = window.document.body.textContent ?? "";
    expect(text).toContain(english("suggestion.noDetector"));
    expect(text).not.toContain(english("suggestion.none"));
  });

  /**
   * A history that could not be read is a failure of the SOURCE, so it is stated beside the queue and the
   * queue already read stays decidable — the failure is not an empty queue wearing a new sentence.
   */
  it("surfaces a failed history read without emptying the queue", async () => {
    fakeDesktop({ list_suggestions: [oneSuggestion] }, ["observe_recent_documents"]);
    component = shell.mount();
    await settle();

    await navigate("#/suggestions");

    const text = window.document.body.textContent ?? "";
    expect(text).toContain(
      english("suggestions.historyError", { message: "Error: observe_recent_documents failed" }),
    );
    expect(text).toContain("/library/cosmos.pdf");
  });
});

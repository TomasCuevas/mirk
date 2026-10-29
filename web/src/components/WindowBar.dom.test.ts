import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it } from "vitest";
import WindowBar from "./WindowBar.svelte";
import type { WindowControl } from "../lib/window";
import { translate } from "../lib/i18n";

const english = translate("en");

/**
 * R5 of `odd/tasks/desktop-chrome.md`: a press on a control must not become a window drag.
 *
 * The user's report: *"los botones de control de ventana siguen sin funcionar del todo bien. En
 * ocasiones funcionan pero hay que darle varios clic hasta que hagan algo"*. The bar starts the move
 * gesture on every press that is not on a button — and the target of a press on a control is normally
 * the `<svg>` or one of its `<path>`s. The guard asked for an `HTMLElement`, which an SVG element is
 * not, so the press fell through to `start_dragging`: the drag took the pointer, the click never
 * completed, and the control only answered on the presses that landed on its padding.
 *
 * Only a simulated browser can see this: the server render produces the markup and runs no handler.
 * The test presses the drawing itself, which is where a person aims.
 */

const bar = {
  mount(props: {
    maximized?: boolean;
    inTauri?: boolean;
    onAction?: (control: WindowControl) => void;
    onDrag?: () => void;
    onToggle?: () => void;
  } = {}): { actions: string[]; drags: number; toggles: number; destroy: () => void } {
    const actions: string[] = [];
    const calls = { drags: 0, toggles: 0 };
    const component = mount(WindowBar, {
      target: window.document.body,
      props: {
        t: english,
        inTauri: props.inTauri ?? true,
        maximized: props.maximized ?? false,
        onAction: (control: WindowControl) => {
          actions.push(control.action);
          props.onAction?.(control);
        },
        onDrag: () => {
          calls.drags += 1;
          props.onDrag?.();
        },
        onToggle: () => {
          calls.toggles += 1;
          props.onToggle?.();
        },
      },
    });
    flushSync();
    return {
      actions,
      get drags() {
        return calls.drags;
      },
      get toggles() {
        return calls.toggles;
      },
      destroy: () => unmount(component),
    };
  },
};

afterEach(() => {
  window.document.body.innerHTML = "";
});

function controls(): HTMLButtonElement[] {
  return Array.from(window.document.querySelectorAll("header button"));
}

/** A press on the bar, with the click count a browser reports — which is how a double click says so. */
function press(
  detail: number,
  target: Element | null = window.document.querySelector("header"),
): void {
  if (target === null) {
    throw new Error("the bar is the drag handle");
  }
  target.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, button: 0, detail }));
  flushSync();
}

describe("pressing a window control", () => {
  it("acts on the control and never starts a drag, even on the drawing itself", () => {
    const mounted = bar.mount();
    // R6: the tray control moved beside minimise, so it is the second button.
    const hide = controls()[1];
    const glyph = hide.querySelector("svg path");
    if (glyph === null) {
      throw new Error("every control draws a glyph");
    }

    glyph.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, button: 0 }));
    hide.click();
    flushSync();

    expect(mounted.drags).toBe(0);
    expect(mounted.actions).toEqual(["hide"]);
    mounted.destroy();
  });

  it("still lets the bar itself be the window's drag handle", () => {
    const mounted = bar.mount();

    press(1);

    expect(mounted.drags).toBe(1);
    expect(mounted.toggles).toBe(0);
    expect(mounted.actions).toEqual([]);
    mounted.destroy();
  });

  // S4: the first press of a double click already handed the window to the compositor's drag loop, and
  // that loop is what would swallow the second one — so the second press asks for the toggle instead of
  // dragging again. The bar only reports the gesture; the parent decides what it means.
  it("asks for the toggle on the second press of a double click instead of dragging", () => {
    const mounted = bar.mount();

    press(2);

    expect(mounted.toggles).toBe(1);
    expect(mounted.drags).toBe(0);
    expect(mounted.actions).toEqual([]);
    mounted.destroy();
  });

  // S5: the controls keep their meaning. A second press aimed at one of them is still a press on a
  // control, so it neither drags nor toggles.
  it("neither drags nor toggles on a second press aimed at a control", () => {
    const mounted = bar.mount();
    const glyph = controls()[1].querySelector("svg path");
    if (glyph === null) {
      throw new Error("every control draws a glyph");
    }

    press(2, glyph);

    expect(mounted.drags).toBe(0);
    expect(mounted.toggles).toBe(0);
    expect(mounted.actions).toEqual([]);
    mounted.destroy();
  });

  it("leaves the press alone outside the desktop shell, where there is no window to move", () => {
    const mounted = bar.mount({ inTauri: false });
    const header = window.document.querySelector("header");
    if (header === null) {
      throw new Error("the bar is the drag handle");
    }

    header.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, button: 0 }));
    flushSync();

    expect(mounted.drags).toBe(0);
    mounted.destroy();
  });
});

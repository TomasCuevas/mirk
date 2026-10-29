import { render } from "svelte/server";
import { describe, expect, it } from "vitest";
import WindowBar from "./WindowBar.svelte";
import { translate } from "../lib/i18n";

const english = translate("en");
const spanish = translate("es");

function barProps(maximized: boolean, locale: "en" | "es" = "en") {
  return {
    t: translate(locale),
    inTauri: true,
    maximized,
    onAction: () => {},
    onDrag: () => {},
  };
}

/**
 * S4 of `odd/tasks/desktop-chrome.md`: the bar's four controls, and the one whose words and drawing
 * depend on the window's state.
 *
 * The maximise control is a toggle: it says "maximise" while a windowed layout is in force and
 * "restore" while full screen is, and it stays marked while the window is maximised so the state is
 * visible without pressing it.
 */
describe("the top bar's controls", () => {
  it("draws the four controls with their drawings", () => {
    const { body } = render(WindowBar, { props: barProps(false) });

    expect((body.match(/<button/g) ?? []).length).toBe(4);
    expect((body.match(/<svg/g) ?? []).length).toBe(4);
    for (const label of [
      english("window.minimize"),
      english("window.maximize"),
      english("window.hide"),
      english("window.close"),
    ]) {
      expect(body).toContain(label);
    }
  });

  it("asks to maximise while a window is in force", () => {
    const { body } = render(WindowBar, { props: barProps(false) });

    expect(body).toContain(english("window.maximize"));
    expect(body).not.toContain(english("window.restore"));
    expect(body).toMatch(/aria-pressed="false"/);
  });

  it("offers to restore, and stays marked, while the window is maximised", () => {
    const { body } = render(WindowBar, { props: barProps(true) });

    expect(body).toContain(english("window.restore"));
    expect(body).not.toContain(english("window.maximize"));
    expect(body).toMatch(/aria-pressed="true"/);
    expect(body).toMatch(/class="[^"]*\bpressed\b/);
  });

  it("says it in Spanish too", () => {
    const { body } = render(WindowBar, { props: barProps(true, "es") });

    expect(body).toContain(spanish("window.restore"));
    expect(body).toContain(spanish("window.minimize"));
    expect(body).not.toContain(english("window.restore"));
  });
});

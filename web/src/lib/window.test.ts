import { describe, expect, it } from "vitest";
import { catalogues } from "./i18n";
import { windowControls } from "./window";

describe("the window controls", () => {
  it("offers exactly minimise, hide and close", () => {
    expect(windowControls.map((control) => control.action)).toEqual([
      "minimize",
      "hide",
      "close",
    ]);
  });

  it("offers no maximise control by any name", () => {
    // A2: the button does not exist, and neither does a control that would behave like one.
    const forbidden = /maximi|expand|fullscreen|full-screen/i;
    for (const control of windowControls) {
      expect(control.action).not.toMatch(forbidden);
      expect(control.command).not.toMatch(forbidden);
      expect(control.labelKey).not.toMatch(forbidden);
      expect(control.icon).not.toMatch(forbidden);
    }
  });

  it("calls a distinct command per control", () => {
    const commands = windowControls.map((control) => control.command);
    expect(new Set(commands).size).toBe(commands.length);
  });

  it("labels every control in both languages", () => {
    for (const control of windowControls) {
      expect(catalogues.en[control.labelKey]?.trim()).toBeTruthy();
      expect(catalogues.es[control.labelKey]?.trim()).toBeTruthy();
      expect(control.icon.trim()).not.toBe("");
    }
  });
});

import { describe, expect, it } from "vitest";
import { isLocale, translate } from "./i18n";

describe("the Spanish catalogue", () => {
  // R3 (L22). The user asked for "Ajustes" because it is shorter than "Configuración", and it has to
  // be the word in both places the screen is named — the navbar entry and the screen's own heading —
  // or the two disagree with each other.
  it("names the settings screen 'Ajustes' in the navbar and in its heading", () => {
    const t = translate("es");

    expect(t("nav.settings")).toBe("Ajustes");
    expect(t("settings.heading")).toBe("Ajustes");
  });
});

describe("the languages this build knows", () => {
  // The list is the type's own source, so the check cannot drift from `Locale`. It exists because a
  // locale arrives from outside the interface — the store, which the desktop process also writes —
  // and an unknown value must be refused rather than rendered as keys with no words behind them.
  it("accepts the languages it ships and refuses everything else", () => {
    expect(isLocale("en")).toBe(true);
    expect(isLocale("es")).toBe(true);

    expect(isLocale("de")).toBe(false);
    expect(isLocale("")).toBe(false);
    expect(isLocale(undefined)).toBe(false);
    expect(isLocale(3)).toBe(false);
  });
});

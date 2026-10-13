import { describe, expect, it } from "vitest";
import { translate } from "./i18n";

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

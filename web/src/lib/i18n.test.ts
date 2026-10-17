import { describe, expect, it } from "vitest";
import { catalogues, isLocale, translate, type Catalogue } from "./i18n";

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

/**
 * L52: the Spanish catalogue speaks with one voice.
 *
 * The interface's Spanish was written in two registers: the first strings address the reader with
 * voseo (the maintainer's own voice), and everything added since uses the neutral register —
 * impersonal, or "usted". A product that ships Spanish cannot address one reader two ways, so the
 * neutral register is the rule and the informal forms are banned from the catalogue. The guard reads
 * the VALUES, not the keys, so the next string somebody adds is checked the moment it lands.
 *
 * The forms are grouped by the shape they take, because the shapes are not interchangeable. A verb
 * form or a pronoun announces the register wherever it appears; an informal imperative is spelled
 * like the same verb's third person and can only be recognised where a command lives — opening a
 * sentence. "Acá" and "renglón" are NOT here: they are regional words, not a register, and this
 * project's reader is Rioplatense.
 */
describe("the Spanish catalogue speaks with one voice", () => {
  // The voseo forms this catalogue can plausibly contain: the present indicative endings (-ás/-és/-ís),
  // the preterite (-aste/-iste), and the second-person pronoun that states the register outright. The
  // list is explicit rather than a pattern like `\w+ás`, because "además" is a neutral adverb a broad
  // ending-matching rule would wrongly reject.
  const voseo = ["volvés", "fijaste", "elegiste", "debés", "podés", "tenés", "tú"] as const;

  // A different shape: possession and the prepositional pronoun. "tu biblioteca" claims the reader's
  // library, and "por ti" names the reader with no verb at all.
  const informalPronouns = ["tu", "tus", "ti", "tuyo", "tuya", "tuyos", "tuyas"] as const;

  // A third shape: the informal imperative. "Escanea una carpeta" is spelled like the third person of
  // the same verb, so an ending cannot give it away — but a command opens a sentence, and a descriptive
  // third person ("oct mira por dentro", "nunca copia ni mueve nada") does not sit there.
  const informalImperatives = [
    "escanea", "agrega", "añade", "selecciona", "elige", "abre", "arrastra", "marca",
    "quita", "cambia", "guarda", "escribe", "usa", "pulsa", "busca", "cierra", "mueve",
    "activa", "desactiva", "confirma", "introduce", "pega", "copia", "acepta", "deja",
    "borra", "cancela", "actualiza", "recarga",
  ] as const;

  // A letter boundary, not `\b`: JavaScript's `\w` is ASCII-only, so `\b` treats the accented "é"/"ú"
  // at the end of these forms as non-word characters and would fail to see a word boundary after them.
  // `\p{L}` knows about accents. The `i` flag lets a sentence-opening imperative be written capitalised.
  function containsForm(value: string, form: string): boolean {
    return new RegExp(`(?<!\\p{L})${form}(?!\\p{L})`, "iu").test(value);
  }

  // Where an imperative is honest about being one: only at the start of a sentence. Leading punctuation
  // ("¿", "¡", an opening quote) is peeled off first so "¿Escanea…?" is still seen.
  function opensWithForm(value: string, form: string): boolean {
    const sentences = value.split(/(?<=[.!?…])\s+/u);
    return sentences.some((sentence) => {
      const stripped = sentence.replace(/^[^\p{L}]+/u, "");
      return new RegExp(`^${form}(?!\\p{L})`, "iu").test(stripped);
    });
  }

  function offendersIn(catalogue: Catalogue): string[] {
    return Object.entries(catalogue).flatMap(([key, value]) => {
      const forms = [
        ...[...voseo, ...informalPronouns].filter((form) => containsForm(value, form)),
        ...informalImperatives.filter((form) => opensWithForm(value, form)),
      ];
      return forms.map((form) => `${key}: "${value}" uses "${form}"`);
    });
  }

  it("never addresses the reader in the informal second person", () => {
    expect(offendersIn(catalogues.es)).toEqual([]);
  });

  // The scan reads values, so the day somebody adds a string in the informal voice it is an offender
  // without anyone remembering to extend this test. The neutral rewrite on the same verb is left
  // alone, and so is "acá": this is a register rule, not a dialect rule.
  it("catches a future informal string and leaves its neutral twin", () => {
    expect(
      offendersIn({
        "future.informal": "Escanea tu carpeta para tus documentos acá.",
        "future.neutral": "Escanee su carpeta para sus documentos acá.",
      }),
    ).toEqual([
      'future.informal: "Escanea tu carpeta para tus documentos acá." uses "tu"',
      'future.informal: "Escanea tu carpeta para tus documentos acá." uses "tus"',
      'future.informal: "Escanea tu carpeta para tus documentos acá." uses "escanea"',
    ]);
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

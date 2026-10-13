/**
 * The one string catalogue for the interface (S6).
 *
 * Every word a person reads lives here, keyed, so no literal is scattered through a component and
 * a second language is a file rather than a rewrite. English is the default; Spanish is the first
 * real alternative, because the maintainer works in Spanish and an open-source product cannot
 * assume one language.
 *
 * Interpolation is deliberately tiny — `{name}` placeholders only. A full i18n runtime (plurals,
 * gender, ICU) is a later task; this is the smallest thing the first screen needs.
 */

/** The languages the first screen ships with. */
export type Locale = "en" | "es";

/** Values substituted into `{placeholders}`. */
export type MessageParams = Record<string, string | number>;

/** Looks a key up and interpolates its parameters. Missing keys come back as the key itself. */
export type Translate = (key: string, params?: MessageParams) => string;

/** A flat key/value catalogue. */
export interface Catalogue {
  readonly [key: string]: string;
}

const english: Catalogue = {
  "app.name": "oct",
  "app.outsideTauri": "The interface is running outside the desktop shell; nothing is connected.",

  "language.label": "Language",
  "language.en": "English",
  "language.es": "Español",

  "library.heading": "Library",
  "library.loading": "Loading the library…",
  "library.empty": "The library is empty. Scan a folder to add documents.",
  "library.error": "Could not load the library: {message}",
  "library.pages": "{count} pages",
  "library.pagesUnknown": "Page count unknown",
  "library.author": "by {author}",

  "progress.none": "No reading progress recorded",
  "progress.position": "p. {page}",
  "progress.positionOf": "p. {page} of {total}",
  "progress.by": "recorded by {viewer}",
  "progress.closed": "recorded when {viewer} closed",
  "progress.manual": "set by you",
  "progress.unknown": "provenance unknown",

  "source.native": "oct",
  "source.manual": "you",
  "source.evince-gvfs": "Evince",

  "provider.heading": "Progress providers",
  "provider.none": "No progress providers are available on this platform.",
  "provider.loading": "Checking which progress providers are available…",
  "provider.installed": "{viewer} is installed and reports {capability}.",
  "provider.unsupported": "{viewer} is unsupported on this platform.",
  "provider.missing": "{viewer} is missing ({capability} unavailable): {reason}",
  "provider.reason.evince-gvfs": "it needs the gio command and an installed Evince or Papers",
  "provider.reason.unknown": "it is not present on this machine",

  "capability.live": "live reading position",
  "capability.close-time": "progress recorded when the viewer closes",
  "capability.none": "no reading position",

  "scan.heading": "Scan a folder",
  "scan.folderLabel": "Folder path",
  "scan.placeholder": "/path/to/folder",
  "scan.action": "Scan",
  "scan.running": "Scanning…",
  "scan.summary": "Counted {counted} of {discovered} PDFs in {root}.",
  "scan.none": "No PDFs were found in {root}.",
  "scan.failure": "{path}: {reason}",
  "scan.error": "The scan failed: {message}",
};

const spanish: Catalogue = {
  "app.name": "oct",
  "app.outsideTauri":
    "La interfaz se está ejecutando fuera del entorno de escritorio; no hay nada conectado.",

  "language.label": "Idioma",
  "language.en": "English",
  "language.es": "Español",

  "library.heading": "Biblioteca",
  "library.loading": "Cargando la biblioteca…",
  "library.empty": "La biblioteca está vacía. Escanea una carpeta para añadir documentos.",
  "library.error": "No se pudo cargar la biblioteca: {message}",
  "library.pages": "{count} páginas",
  "library.pagesUnknown": "Número de páginas desconocido",
  "library.author": "de {author}",

  "progress.none": "No hay progreso de lectura registrado",
  "progress.position": "pág. {page}",
  "progress.positionOf": "pág. {page} de {total}",
  "progress.by": "registrado por {viewer}",
  "progress.closed": "registrado al cerrar {viewer}",
  "progress.manual": "lo fijaste tú",
  "progress.unknown": "procedencia desconocida",

  "source.native": "oct",
  "source.manual": "ti",
  "source.evince-gvfs": "Evince",

  "provider.heading": "Proveedores de progreso",
  "provider.none": "No hay proveedores de progreso disponibles en esta plataforma.",
  "provider.loading": "Verificando qué proveedores de progreso están disponibles…",
  "provider.installed": "{viewer} está instalado y reporta {capability}.",
  "provider.unsupported": "{viewer} no es compatible con esta plataforma.",
  "provider.missing": "Falta {viewer} ({capability} no disponible): {reason}",
  "provider.reason.evince-gvfs":
    "necesita el comando gio y una instalación de Evince o Papers",
  "provider.reason.unknown": "no está presente en esta máquina",

  "capability.live": "posición de lectura en vivo",
  "capability.close-time": "progreso registrado al cerrar el visor",
  "capability.none": "ninguna posición de lectura",

  "scan.heading": "Escanear una carpeta",
  "scan.folderLabel": "Ruta de la carpeta",
  "scan.placeholder": "/ruta/a/carpeta",
  "scan.action": "Escanear",
  "scan.running": "Escaneando…",
  "scan.summary": "Se contaron {counted} de {discovered} PDF en {root}.",
  "scan.none": "No se encontraron PDF en {root}.",
  "scan.failure": "{path}: {reason}",
  "scan.error": "El escaneo falló: {message}",
};

/** Every catalogue, indexed by locale. */
export const catalogues: Record<Locale, Catalogue> = { en: english, es: spanish };

/** Builds a translator for `locale`, falling back to English and then to the key itself. */
export function translate(locale: Locale): Translate {
  const catalogue = catalogues[locale] ?? english;
  return (key, params) => {
    const template = catalogue[key] ?? english[key] ?? key;
    return interpolate(template, params);
  };
}

function interpolate(template: string, params?: MessageParams): string {
  if (params === undefined) {
    return template;
  }
  return template.replace(/\{(\w+)\}/g, (match, name: string) => {
    const value = params[name];
    return value === undefined ? match : String(value);
  });
}

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

  "nav.ariaLabel": "Screens",
  "nav.home": "Home",
  "nav.library": "Library",
  "nav.settings": "Settings",

  "home.heading": "Home",
  "home.placeholder":
    "Home will summarise your reading. These summaries need queries that do not exist yet, so nothing here is estimated.",
  "home.inProgress.title": "Documents in progress",
  "home.inProgress.detail": "Documents you have started and not finished.",
  "home.finished.title": "Documents you finished",
  "home.finished.detail": "Documents you have read to the last page.",
  "home.stale.title": "Documents you have not opened in a long time",
  "home.stale.detail": "Documents whose recorded reading position has not moved in a while.",

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
  "library.filterLabel": "Filter",
  "library.filterPlaceholder": "Filter by title or path",
  "library.sortLabel": "Sort by",
  "library.sort.title": "Title",
  "library.sort.pages": "Page count",
  "library.sort.progress": "Reading progress",
  "library.noMatches": "No document matches “{query}”.",
  "library.count": "{shown} of {total} documents",

  "card.cover": "No cover yet",
  "card.openFolder": "Open containing folder",
  "card.openedFolder": "Opened {folder} with {viewer}.",
  "card.openFolderError": "Could not open the folder: {message}",

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

  "settings.heading": "Settings",
  "settings.language.heading": "Language",
  "settings.folders.heading": "Watched folders",
  "settings.folders.loading": "Reading the folders the store has recorded…",
  "settings.folders.empty": "No folder has been recorded yet. A scan records one.",
  "settings.folders.error": "Could not read the watched folders: {message}",
  "settings.providers.error": "Could not read the progress providers: {message}",
  "settings.folders.recursive": "includes subfolders",
  "settings.folders.notRecursive": "top level only",
  "settings.folders.enabled": "watcher enabled",
  "settings.folders.disabled": "watcher disabled",

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

  "nav.ariaLabel": "Pantallas",
  "nav.home": "Inicio",
  "nav.library": "Biblioteca",
  "nav.settings": "Configuración",

  "home.heading": "Inicio",
  "home.placeholder":
    "Inicio resumirá tu lectura. Esos resúmenes necesitan consultas que todavía no existen, así que aquí no hay nada estimado.",
  "home.inProgress.title": "Documentos en curso",
  "home.inProgress.detail": "Documentos que empezaste y no terminaste.",
  "home.finished.title": "Documentos terminados",
  "home.finished.detail": "Documentos que leíste hasta la última página.",
  "home.stale.title": "Documentos sin abrir durante mucho tiempo",
  "home.stale.detail":
    "Documentos cuya posición de lectura registrada no se movió en un tiempo.",

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
  "library.filterLabel": "Filtrar",
  "library.filterPlaceholder": "Filtrar por título o ruta",
  "library.sortLabel": "Ordenar por",
  "library.sort.title": "Título",
  "library.sort.pages": "Número de páginas",
  "library.sort.progress": "Progreso de lectura",
  "library.noMatches": "Ningún documento coincide con «{query}».",
  "library.count": "{shown} de {total} documentos",

  "card.cover": "Sin portada todavía",
  "card.openFolder": "Abrir la carpeta contenedora",
  "card.openedFolder": "Se abrió {folder} con {viewer}.",
  "card.openFolderError": "No se pudo abrir la carpeta: {message}",

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

  "settings.heading": "Configuración",
  "settings.language.heading": "Idioma",
  "settings.folders.heading": "Carpetas vigiladas",
  "settings.folders.loading": "Leyendo las carpetas que el almacén tiene registradas…",
  "settings.folders.empty": "Todavía no hay ninguna carpeta registrada. Un escaneo registra una.",
  "settings.folders.error": "No se pudieron leer las carpetas vigiladas: {message}",
  "settings.providers.error": "No se pudieron leer los proveedores de progreso: {message}",
  "settings.folders.recursive": "incluye subcarpetas",
  "settings.folders.notRecursive": "solo el primer nivel",
  "settings.folders.enabled": "vigilancia activada",
  "settings.folders.disabled": "vigilancia desactivada",

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

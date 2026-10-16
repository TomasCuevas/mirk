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

/** The languages the first screen ships with. The list is the type's source, so a check written
 * against it cannot drift from `Locale`. */
export const LOCALES = ["en", "es"] as const;

/** A language the interface has words for. */
export type Locale = (typeof LOCALES)[number];

/**
 * Whether `value` is a language this build knows.
 *
 * It takes `unknown` on purpose: the value arrives from outside the interface — the stored setting,
 * which the desktop process also writes — so it has to be checked before a screen uses it. An
 * unknown locale would render keys with no words behind them and leave the tray with no labels,
 * which is why "unknown" and "stored" are different answers.
 */
export function isLocale(value: unknown): value is Locale {
  return typeof value === "string" && (LOCALES as readonly string[]).includes(value);
}

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

  "window.controls": "Window controls",
  "window.minimize": "Minimise",
  "window.hide": "Hide to the tray",
  "window.close": "Close oct",

  "nav.ariaLabel": "Screens",
  "nav.home": "Home",
  "nav.library": "Library",
  "nav.settings": "Settings",

  "home.heading": "Home",
  "home.inProgress.title": "Documents in progress",
  "home.inProgress.definition": "Documents whose recorded position is not at the end.",
  "home.finished.title": "Documents you finished",
  "home.finished.definition": "Documents whose recorded position is at the end.",
  "home.stale.title": "Documents you have not opened in a long time",
  "home.stale.definition":
    "Documents the viewer last recorded reading at least {days} days ago. A document no source ever \
recorded is not claimed here.",
  "home.openCategory": "Open full list",
  "home.openCategoryLabel": "Open the full list of {category}",
  "home.categoryEmpty": "Nothing here yet.",
  "home.firstRun.emptyLibrary.title": "Your library is empty",
  "home.firstRun.emptyLibrary.body":
    "No PDF has been found yet. A watched folder is what oct looks inside: it reads each document's pages and progress, and it never copies or moves anything.",
  "home.firstRun.emptyLibrary.action": "Add a folder",
  "home.firstRun.noRecords.title": "No reading recorded yet",
  "home.firstRun.noRecords.body":
    "A document appears in these lists once a source records where you are — your viewer, or this application while you read. Open a PDF in your viewer and it will show up here.",
  "home.firstRun.noRecords.action": "See the library",
  "category.back": "Back",

  "language.label": "Language",
  "language.en": "English",
  "language.es": "Español",

  "library.heading": "Library",
  "library.loading": "Loading the library…",
  "library.empty": "The library is empty. Scan a folder to add documents.",
  "library.error": "Could not load the library: {message}",
  "library.pages": "{count} pages",
  "library.author": "by {author}",
  "library.filterLabel": "Filter",
  "library.filterPlaceholder": "Filter by title or path",
  "library.sortLabel": "Sort by",
  "library.sort.title": "Title",
  "library.sort.pages": "Page count",
  "library.sort.progress": "Reading progress",
  "library.noMatches": "No document matches “{query}”.",
  "library.count": "{shown} of {total} documents",
  "library.pagePosition": "Page {page} of {pages}",
  "library.previousPage": "Previous page",
  "library.nextPage": "Next page",

  "card.cover": "No cover yet",
  "card.progressPercent": "{percent}%",
  "card.openFolder": "Open containing folder",
  "card.editTitle": "Edit the title of {title}",
  "card.titleEditor": "Edit the title",
  "card.titleLabel": "Title",
  "card.titleLabelYours": "Your title",
  "card.titleSave": "Save",
  "card.titleCancel": "Cancel",
  "card.titleRestore": "Restore the document's own title",
  "card.titleBlank": "A title cannot be empty.",

  "progress.position": "p. {page}",
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
  "settings.folders.remove": "Remove",
  "settings.folders.removeFolder": "Stop watching {path} and remove its documents",
  "settings.folders.open": "Open {path} in the file manager",
  "settings.folders.covered": "That folder is inside {folder}, which is already watched, so it was not added.",
  // R26: the covering folder can be the chosen path itself, and "a folder is inside itself" teaches the
  // reader nothing. Being already watched is its own sentence rather than a special case of the other.
  "settings.folders.coveredSelf": "That folder is already watched, so it was not added.",
  "settings.folders.removalTitle": "Stop watching this folder?",
  "settings.folders.removalNone": "No document of this folder is in your library, so only the watch is removed.",
  "settings.folders.removalAllKept": "Every document of this folder stays: another watched folder still covers it.",
  "settings.folders.removalOne": "1 document of this folder leaves your library.",
  "settings.folders.removalMany": "{count} documents of this folder leave your library.",
  "settings.folders.removalKept": "Documents that another watched folder still covers stay.",
  "settings.folders.removalRecover": "The reading position is kept by the viewer, so it comes back if you watch the folder again.",
  "settings.folders.removalCancel": "Cancel",
  "settings.folders.removalConfirm": "Remove folder and documents",
  "settings.providers.error": "Could not read the progress providers: {message}",
  "settings.folders.recursive": "includes subfolders",
  "settings.folders.notRecursive": "top level only",
  "settings.folders.enabled": "watcher enabled",
  "settings.folders.disabled": "watcher disabled",

  "capability.live": "live reading position",
  "capability.close-time": "progress recorded when the viewer closes",
  "capability.none": "no reading position",

  "scan.heading": "Scan a folder",
  "scan.choose": "Choose folder",
  "scan.manual": "Enter the path by hand",
  "scan.folderLabel": "Folder path",
  "scan.noFolder": "No folder chosen yet.",
  "scan.refusedTitle": "That folder was not added",
  "scan.refusedClose": "Got it",
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

  "window.controls": "Controles de la ventana",
  "window.minimize": "Minimizar",
  "window.hide": "Ocultar en la bandeja",
  "window.close": "Cerrar oct",

  "nav.ariaLabel": "Pantallas",
  "nav.home": "Inicio",
  "nav.library": "Biblioteca",
  "nav.settings": "Ajustes",

  "home.heading": "Inicio",
  "home.inProgress.title": "Documentos en curso",
  "home.inProgress.definition": "Documentos cuya posición registrada no está al final.",
  "home.finished.title": "Documentos terminados",
  "home.finished.definition": "Documentos cuya posición registrada está al final.",
  "home.stale.title": "Documentos sin abrir durante mucho tiempo",
  "home.stale.definition":
    "Documentos que el visor registró leyendo por última vez hace al menos {days} días. Los que ninguna \
fuente registró nunca no se listan acá.",
  "home.openCategory": "Abrir la lista completa",
  "home.openCategoryLabel": "Abrir la lista completa de {category}",
  "home.categoryEmpty": "Todavía no hay nada.",
  "home.firstRun.emptyLibrary.title": "Su biblioteca está vacía",
  "home.firstRun.emptyLibrary.body":
    "Todavía no se encontró ningún PDF. Una carpeta vigilada es lo que oct mira por dentro: lee las páginas y el progreso de cada documento, y nunca copia ni mueve nada.",
  "home.firstRun.emptyLibrary.action": "Agregar una carpeta",
  "home.firstRun.noRecords.title": "Todavía no hay ninguna lectura registrada",
  "home.firstRun.noRecords.body":
    "Un documento aparece en estas listas cuando una fuente registra por dónde va: su visor, o esta aplicación mientras lee. Abra un PDF en su visor y aparecerá acá.",
  "home.firstRun.noRecords.action": "Ver la biblioteca",
  "category.back": "Volver",

  "language.label": "Idioma",
  "language.en": "English",
  "language.es": "Español",

  "library.heading": "Biblioteca",
  "library.loading": "Cargando la biblioteca…",
  "library.empty": "La biblioteca está vacía. Escanea una carpeta para añadir documentos.",
  "library.error": "No se pudo cargar la biblioteca: {message}",
  "library.pages": "{count} páginas",
  "library.author": "de {author}",
  "library.filterLabel": "Filtrar",
  "library.filterPlaceholder": "Filtrar por título o ruta",
  "library.sortLabel": "Ordenar por",
  "library.sort.title": "Título",
  "library.sort.pages": "Número de páginas",
  "library.sort.progress": "Progreso de lectura",
  "library.noMatches": "Ningún documento coincide con «{query}».",
  "library.count": "{shown} de {total} documentos",
  "library.pagePosition": "Página {page} de {pages}",
  "library.previousPage": "Página anterior",
  "library.nextPage": "Página siguiente",

  "card.cover": "Sin portada todavía",
  "card.progressPercent": "{percent} %",
  "card.openFolder": "Abrir la carpeta contenedora",
  "card.editTitle": "Editar el título de {title}",
  "card.titleEditor": "Editar el título",
  "card.titleLabel": "Título",
  "card.titleLabelYours": "Su título",
  "card.titleSave": "Guardar",
  "card.titleCancel": "Cancelar",
  "card.titleRestore": "Restaurar el título del documento",
  "card.titleBlank": "El título no puede estar vacío.",

  "progress.position": "pág. {page}",
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

  "settings.heading": "Ajustes",
  "settings.language.heading": "Idioma",
  "settings.folders.heading": "Carpetas vigiladas",
  "settings.folders.loading": "Leyendo las carpetas que el almacén tiene registradas…",
  "settings.folders.empty": "Todavía no hay ninguna carpeta registrada. Un escaneo registra una.",
  "settings.folders.error": "No se pudieron leer las carpetas vigiladas: {message}",
  "settings.folders.remove": "Quitar",
  "settings.folders.removeFolder": "Dejar de vigilar {path} y quitar sus documentos",
  "settings.folders.open": "Abrir {path} en el explorador de archivos",
  "settings.folders.covered": "Esa carpeta está dentro de {folder}, que ya está vigilada, así que no se agregó.",
  "settings.folders.coveredSelf": "Esa carpeta ya está vigilada, así que no se agregó.",
  "settings.folders.removalTitle": "¿Dejar de vigilar esta carpeta?",
  "settings.folders.removalNone": "No hay ningún documento de esta carpeta en tu biblioteca, así que solo se quita la vigilancia.",
  "settings.folders.removalAllKept": "Todos los documentos de esta carpeta se quedan: otra carpeta vigilada todavía los cubre.",
  "settings.folders.removalOne": "1 documento de esta carpeta sale de tu biblioteca.",
  "settings.folders.removalMany": "{count} documentos de esta carpeta salen de tu biblioteca.",
  "settings.folders.removalKept": "Los documentos que otra carpeta vigilada todavía cubre se quedan.",
  "settings.folders.removalRecover": "La posición de lectura la guarda el visor, así que vuelve si volvés a vigilar la carpeta.",
  "settings.folders.removalCancel": "Cancelar",
  "settings.folders.removalConfirm": "Quitar la carpeta y sus documentos",
  "settings.providers.error": "No se pudieron leer los proveedores de progreso: {message}",
  "settings.folders.recursive": "incluye subcarpetas",
  "settings.folders.notRecursive": "solo el primer nivel",
  "settings.folders.enabled": "vigilancia activada",
  "settings.folders.disabled": "vigilancia desactivada",

  "capability.live": "posición de lectura en vivo",
  "capability.close-time": "progreso registrado al cerrar el visor",
  "capability.none": "ninguna posición de lectura",

  "scan.heading": "Escanear una carpeta",
  "scan.choose": "Elegir carpeta",
  "scan.manual": "Escribir la ruta a mano",
  "scan.folderLabel": "Ruta de la carpeta",
  "scan.noFolder": "Todavía no elegiste ninguna carpeta.",
  "scan.refusedTitle": "Esa carpeta no se agregó",
  "scan.refusedClose": "Entendido",
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

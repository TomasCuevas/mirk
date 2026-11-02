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
  "app.name": "mirk",
  "app.outsideTauri": "The interface is running outside the desktop shell; nothing is connected.",

  "window.controls": "Window controls",
  "window.minimize": "Minimise",
  "window.maximize": "Maximise",
  "window.restore": "Restore the window",
  "window.hide": "Hide to the tray",
  "window.close": "Close Mirk",

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
  "home.openCategory": "Full list",
  "home.carouselPrevious": "Scroll the carousel left",
  "home.carouselNext": "Scroll the carousel right",
  "home.openCategoryLabel": "See the full list of {category}",
  "home.categoryEmpty": "Nothing here yet.",
  "home.firstRun.emptyLibrary.title": "Your library is empty",
  "home.firstRun.emptyLibrary.body":
    "No PDF has been found yet. A watched folder is what Mirk looks inside: it reads each document's pages and progress, and it never copies or moves anything.",
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
  // R42/S4: the connector a list of authors is read with. It is here, in both languages, because the
  // sentence that states co-authors must not carry an English "and" into the Spanish interface.
  "library.authorsAnd": "and",
  "library.filterLabel": "Search",
  "library.filterPlaceholder": "Title, author or path",
  "library.authorLabel": "Author",
  "library.authorAll": "All authors",
  "library.sortLabel": "Order",
  "library.sort.title": "Title",
  "library.sort.pages": "Page count",
  "library.sort.progress": "Reading progress",
  "library.noMatches": "No document matches “{query}”.",
  "library.noMatchesAuthor": "No document by {author}.",
  "library.count": "{shown} of {total}",
  // Only when there is something hidden by a filter or a page: "42 of 42" is arithmetic, not information.
  "library.countAll": "{total} documents",
  "library.pagination": "Pages",
  "library.suggestions": "Suggestions",
  "library.suggestionsPending": "Suggestions: {count}",
  "library.suggestionsNone": "No suggestions to review",
  "library.page": "Page {page}",
  "library.pagePosition": "Page {page} of {pages}",
  "library.previousPage": "Previous page",
  "library.nextPage": "Next page",

  "card.cover": "No cover yet",
  "card.progressPercent": "{percent}%",
  "card.openFolder": "Open containing folder",
  "card.openDocument": "Open {title}",
  "card.editDocument": "Edit the document {title}",
  "card.editTip": "Edit the document's details",
  "card.documentEditor": "Edit the document",
  // R16: the editor has two groups, and each says which kind of change it holds.
  "card.editorText": "Text and authors",
  "card.titleLabel": "Title",
  "card.titleLabelYours": "Your title",
  "card.titleSave": "Save",
  "card.titleCancel": "Cancel",
  "card.titleRestore": "Restore the document's own title",
  "card.titleBlank": "A title cannot be empty.",
  // R18: restoring is destructive, so it asks, and the question says exactly what is lost.
  "card.restoreConfirmTitle": "Restore the document's title?",
  "card.restoreConfirmBody":
    "Your title is discarded and the document's own title is shown again. Unsaved changes in this form are discarded too.",
  "card.restoreConfirmAction": "Restore",
  "card.authorLabel": "Author",
  "card.authorLabelYours": "Your author",
  // R42/U30: the chips. The field beside the list is how a NEW name gets in, so its accessible name
  // says so and its placeholder states the gesture — Enter is not guessable from an empty box.
  "card.authorAdd": "Add an author",
  "card.authorAddPlaceholder": "Type a name and press Enter",
  // The remove control's accessible name NAMES its author: an icon alone would leave two identical
  // "remove" buttons and no way to know which name goes.
  "card.authorRemove": "Remove {author}",
  "card.authorHint": "Remove every name to use the document's own author.",
  "card.authorChoose": "Choose an author…",
  "card.coverEditor": "Change the cover",
  "card.coverPreview": "Current cover",
  // R18: the cover section states its source before offering the two ways to change it.
  "card.coverCurrentPage": "Now: page {page} of the document.",
  "card.coverCurrentImage": "Now: an image of yours.",
  "card.coverSourceHint": "A page and an image cannot both be the cover: using one replaces the other.",
  "card.coverPageSection": "Page of the document",
  "card.coverImageSection": "An image of your own",
  "card.coverUseImage": "Use the image you saved",
  "card.coverPageLabel": "Page",
  "card.coverPageHint": "The first page is the default. Leave it empty to use the first page.",
  "card.coverPageSave": "Use this page",
  "card.coverPageInvalid": "A page is a whole number from 1.",
  "card.coverChooseImage": "Choose an image…",
  "card.coverRemoveImage": "Remove the image",

  "progress.position": "p. {page}",
  "progress.by": "recorded by {viewer}",
  "progress.closed": "recorded when {viewer} closed",
  "progress.manual": "set by you",
  "progress.unknown": "provenance unknown",

  "source.native": "Mirk",
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
  // R43/S1–S4. Ajustes is an index of three sub-screens, so each one needs its name, the one sentence
  // the index says under it, and the one line the sub-screen says about itself. Nothing here is a new
  // setting: it is the same five blocks, named, grouped and explained.
  "settings.intro": "Four sections, one subject each.",
  "settings.back": "Settings",
  "settings.backLabel": "Back to Settings",
  "settings.library.title": "Library",
  "settings.library.summary": "The folders Mirk reads, and scanning.",
  "settings.library.purpose": "What Mirk watches, and how to add a folder.",
  "settings.reading.title": "Reading",
  "settings.reading.summary": "Where Mirk can read your position from.",
  "settings.reading.purpose":
    "Each viewer Mirk can take a position from, and whether this machine has it.",
  "settings.app.title": "Application",
  // S7 of `odd/tasks/ci-and-release.md` put the update check on this sub-screen, so the section is no
  // longer only the language: the summary and the purpose name both, or the index would promise less
  // than the screen holds.
  "settings.app.summary": "The language, and the version Mirk is running.",
  "settings.app.purpose":
    "The words Mirk uses, and the update check that keeps it current.",
  "settings.update.heading": "Updates",
  "settings.update.hint":
    "Mirk asks GitHub for a newer version when it opens. Nothing is downloaded until the update is accepted.",
  "settings.update.idle": "Not checked in this window yet.",
  "settings.update.checking": "Asking GitHub for the newest version…",
  "settings.update.current": "Mirk is up to date.",
  "settings.update.available": "Version {version} is available.",
  "settings.update.installing": "Downloading the update… {percent}%",
  "settings.update.installingUnknown": "Downloading the update…",
  "settings.update.failed": "Could not check for a newer version: {reason}",
  "settings.update.check": "Check for updates",
  "settings.update.install": "Update and restart",
  "settings.update.dot": "A newer version of Mirk is available",
  "settings.window.title": "Window",
  "settings.window.summary": "How many documents a row holds, and the size of the window.",
  "settings.window.purpose":
    "The window is shaped by the grid: how many documents a row holds, and how much room they take.",
  "settings.window.heading": "Documents per row",
  "settings.window.label": "Window layout",
  "settings.window.columns2": "2 per row",
  "settings.window.columns3": "3 per row",
  "settings.window.columns4": "4 per row",
  "settings.window.auto": "Full screen",
  "settings.window.hint":
    "The window widens to fit the row you pick, and remembers it. Full screen uses whatever fits the screen.",
  "settings.language.heading": "Language",
  // R1: the choice is not the screen's private state — the desktop process builds the tray's words from
  // the same stored value — so the selector says so instead of leaving the reader to discover it.
  "settings.language.hint": "The tray menu changes at once, and the interface with it.",
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

  "suggestion.heading": "Suggestions",
  "suggestions.back": "Library",
  "suggestions.backLabel": "Back to the library",
  "suggestion.loading": "Checking for documents opened outside Mirk…",
  "suggestion.openFolder": "Open the folder that holds {path}",
  "suggestion.noDetector":
    "This system cannot tell which documents are open, so this list cannot fill.",
  "suggestion.none": "No document is waiting for a decision.",
  "suggestion.acceptLabel": "Add {path} to the library",
  "suggestion.discardLabel": "Discard the suggestion for {path}",
  "suggestion.error": "The decision could not be recorded: {message}",
  // T4: the queue read failed is not a decision that was attempted, and saying so would be false. The two
  // failures are two facts, so they are two sentences — the panel and the screen used to share this one.
  "suggestion.readError": "The suggestions could not be read: {message}",

  // S1–S3, S5 of `odd/tasks/suggestions-screen.md`: the sentence the suggestions screen adds around the
  // queue it already had words for. The definitions above are reused verbatim — one queue, one way of
  // describing it — so what is new here is only what the screen itself needs: its search field, its
  // order, its count, its pager and the two failures that can keep it from filling.
  "suggestions.filterLabel": "Search",
  "suggestions.filterPlaceholder": "Title or path",
  "suggestions.sortLabel": "Order",
  "suggestions.sort.recent": "Most recently opened first",
  "suggestions.sort.oldest": "Least recently opened first",
  "suggestions.sort.title": "Title",
  "suggestions.countAll": "{total} suggestions",
  "suggestions.pagination": "Suggestion pages",
  "suggestions.noMatches": "No suggestion matches “{query}”.",
  "suggestions.historyError":
    "The documents the desktop remembered opening could not be read: {message}",

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
  "app.name": "mirk",
  "app.outsideTauri":
    "La interfaz se está ejecutando fuera del entorno de escritorio; no hay nada conectado.",

  "window.controls": "Controles de la ventana",
  "window.minimize": "Minimizar",
  "window.maximize": "Maximizar",
  "window.restore": "Restaurar la ventana",
  "window.hide": "Ocultar en la bandeja",
  "window.close": "Cerrar Mirk",

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
  "home.openCategory": "Lista completa",
  "home.carouselPrevious": "Desplazar el carrusel a la izquierda",
  "home.carouselNext": "Desplazar el carrusel a la derecha",
  "home.openCategoryLabel": "Ver la lista completa de {category}",
  "home.categoryEmpty": "Todavía no hay nada.",
  "home.firstRun.emptyLibrary.title": "Su biblioteca está vacía",
  "home.firstRun.emptyLibrary.body":
    "Todavía no se encontró ningún PDF. Una carpeta vigilada es lo que Mirk mira por dentro: lee las páginas y el progreso de cada documento, y nunca copia ni mueve nada.",
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
  "library.empty": "La biblioteca está vacía. Escanee una carpeta para añadir documentos.",
  "library.error": "No se pudo cargar la biblioteca: {message}",
  "library.pages": "{count} páginas",
  "library.author": "de {author}",
  "library.authorsAnd": "y",
  "library.filterLabel": "Buscar",
  "library.filterPlaceholder": "Título, autor o ruta",
  "library.authorLabel": "Autor",
  "library.authorAll": "Todos los autores",
  "library.sortLabel": "Orden",
  "library.sort.title": "Título",
  "library.sort.pages": "Número de páginas",
  "library.sort.progress": "Progreso de lectura",
  "library.noMatches": "Ningún documento coincide con «{query}».",
  "library.noMatchesAuthor": "Ningún documento de {author}.",
  "library.count": "{shown} de {total}",
  "library.countAll": "{total} documentos",
  "library.pagination": "Páginas",
  "library.suggestions": "Sugerencias",
  "library.suggestionsPending": "Sugerencias: {count}",
  "library.suggestionsNone": "No hay sugerencias para revisar",
  "library.page": "Página {page}",
  "library.pagePosition": "Página {page} de {pages}",
  "library.previousPage": "Página anterior",
  "library.nextPage": "Página siguiente",

  "card.cover": "Sin portada todavía",
  "card.progressPercent": "{percent} %",
  "card.openFolder": "Abrir la carpeta contenedora",
  "card.openDocument": "Abrir {title}",
  "card.editDocument": "Editar el documento {title}",
  "card.editTip": "Editar los datos del documento",
  "card.documentEditor": "Editar el documento",
  "card.editorText": "Texto y autores",
  "card.titleLabel": "Título",
  "card.titleLabelYours": "Su título",
  "card.titleSave": "Guardar",
  "card.titleCancel": "Cancelar",
  "card.titleRestore": "Restaurar el título del documento",
  "card.titleBlank": "El título no puede estar vacío.",
  "card.restoreConfirmTitle": "¿Restaurar el título del documento?",
  "card.restoreConfirmBody":
    "Su título se descarta y vuelve a mostrarse el del documento. Los cambios sin guardar de este formulario también se descartan.",
  "card.restoreConfirmAction": "Restaurar",
  "card.authorLabel": "Autor",
  "card.authorLabelYours": "Su autor",
  "card.authorAdd": "Añadir un autor",
  "card.authorAddPlaceholder": "Escriba un nombre y presione Intro",
  "card.authorRemove": "Quitar a {author}",
  "card.authorHint": "Deje la lista vacía para usar el autor del documento.",
  "card.authorChoose": "Elija un autor…",
  "card.coverEditor": "Cambiar la portada",
  "card.coverPreview": "Portada actual",
  "card.coverCurrentPage": "Ahora: página {page} del documento.",
  "card.coverCurrentImage": "Ahora: una imagen suya.",
  "card.coverSourceHint": "Una página y una imagen no pueden ser la portada a la vez: usar una reemplaza la otra.",
  "card.coverPageSection": "Página del documento",
  "card.coverImageSection": "Una imagen suya",
  "card.coverUseImage": "Usar la imagen guardada",
  "card.coverPageLabel": "Página",
  "card.coverPageHint": "La primera página es la opción predeterminada. Deje el campo vacío para usarla.",
  "card.coverPageSave": "Usar esta página",
  "card.coverPageInvalid": "La página es un número entero desde 1.",
  "card.coverChooseImage": "Elegir una imagen…",
  "card.coverRemoveImage": "Quitar la imagen",

  "progress.position": "pág. {page}",
  "progress.by": "registrado por {viewer}",
  "progress.closed": "registrado al cerrar {viewer}",
  "progress.manual": "fijado por usted",
  "progress.unknown": "procedencia desconocida",

  "source.native": "Mirk",
  "source.manual": "usted",
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
  "settings.intro": "Cuatro secciones, un tema cada una.",
  "settings.back": "Ajustes",
  "settings.backLabel": "Volver a Ajustes",
  "settings.library.title": "Biblioteca",
  "settings.library.summary": "Carpetas vigiladas y escaneo.",
  "settings.library.purpose": "Qué vigila Mirk y cómo agregar una carpeta.",
  "settings.reading.title": "Lectura",
  "settings.reading.summary": "De dónde puede leer Mirk su posición.",
  "settings.reading.purpose":
    "Cada visor del que Mirk puede tomar una posición, y si esta máquina lo tiene.",
  "settings.app.title": "Aplicación",
  "settings.app.summary": "El idioma y la versión de Mirk.",
  "settings.app.purpose":
    "Las palabras que usa Mirk y la comprobación que lo mantiene al día.",
  "settings.update.heading": "Actualizaciones",
  "settings.update.hint":
    "Mirk le pregunta a GitHub si hay una versión nueva cuando se abre. No se descarga nada hasta que la actualización se acepta.",
  "settings.update.idle": "Todavía no se comprobó en esta ventana.",
  "settings.update.checking": "Consultando la versión más nueva en GitHub…",
  "settings.update.current": "Mirk está al día.",
  "settings.update.available": "Hay una versión nueva: {version}.",
  "settings.update.installing": "Descargando la actualización… {percent}%",
  "settings.update.installingUnknown": "Descargando la actualización…",
  "settings.update.failed": "No se pudo comprobar si hay una versión nueva: {reason}",
  "settings.update.check": "Buscar actualizaciones",
  "settings.update.install": "Actualizar y reiniciar",
  "settings.update.dot": "Hay una versión nueva de Mirk disponible",
  "settings.window.title": "Ventana",
  "settings.window.summary": "Cuántos documentos entran en una fila y el tamaño de la ventana.",
  "settings.window.purpose":
    "La ventana se adapta a la grilla: cuántos documentos entran en una fila y cuánto lugar ocupan.",
  "settings.window.heading": "Documentos por fila",
  "settings.window.label": "Disposición de la ventana",
  "settings.window.columns2": "2 por fila",
  "settings.window.columns3": "3 por fila",
  "settings.window.columns4": "4 por fila",
  "settings.window.auto": "Pantalla completa",
  "settings.window.hint":
    "La ventana se ensancha para que entre la fila que se elija, y lo recuerda. Pantalla completa usa lo que entre en la pantalla.",
  "settings.language.heading": "Idioma",
  "settings.language.hint": "El menú de la bandeja cambia al instante, y la interfaz con él.",
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
  "settings.folders.removalNone": "No hay ningún documento de esta carpeta en su biblioteca, así que solo se quita la vigilancia.",
  "settings.folders.removalAllKept": "Todos los documentos de esta carpeta se quedan: otra carpeta vigilada todavía los cubre.",
  "settings.folders.removalOne": "1 documento de esta carpeta sale de su biblioteca.",
  "settings.folders.removalMany": "{count} documentos de esta carpeta salen de su biblioteca.",
  "settings.folders.removalKept": "Los documentos que otra carpeta vigilada todavía cubre se quedan.",
  "settings.folders.removalRecover": "La posición de lectura la guarda el visor, así que vuelve si vigila la carpeta de nuevo.",
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

  "suggestion.heading": "Sugerencias",
  "suggestions.back": "Biblioteca",
  "suggestions.backLabel": "Volver a la biblioteca",
  "suggestion.loading": "Buscando documentos abiertos fuera de Mirk…",
  "suggestion.openFolder": "Abrir la carpeta que contiene {path}",
  "suggestion.noDetector":
    "Este sistema no puede saber qué documentos están abiertos, así que esta lista no puede llenarse.",
  "suggestion.none": "Ningún documento espera una decisión.",
  "suggestion.acceptLabel": "Agregar {path} a la biblioteca",
  "suggestion.discardLabel": "Descartar la sugerencia de {path}",
  "suggestion.error": "No se pudo registrar la decisión: {message}",
  "suggestion.readError": "No se pudieron leer las sugerencias: {message}",

  "suggestions.filterLabel": "Buscar",
  "suggestions.filterPlaceholder": "Título o ruta",
  "suggestions.sortLabel": "Orden",
  "suggestions.sort.recent": "Abiertos más recientemente primero",
  "suggestions.sort.oldest": "Abiertos hace más tiempo primero",
  "suggestions.sort.title": "Título",
  "suggestions.countAll": "{total} sugerencias",
  "suggestions.pagination": "Páginas de sugerencias",
  "suggestions.noMatches": "Ninguna sugerencia coincide con «{query}».",
  "suggestions.historyError":
    "No se pudieron leer los documentos que el escritorio recordaba haber abierto: {message}",

  "scan.heading": "Escanear una carpeta",
  "scan.choose": "Elegir carpeta",
  "scan.manual": "Escribir la ruta a mano",
  "scan.folderLabel": "Ruta de la carpeta",
  "scan.noFolder": "Todavía no eligió ninguna carpeta.",
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

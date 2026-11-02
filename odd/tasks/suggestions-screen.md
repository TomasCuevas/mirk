# Feature: la pantalla de sugerencias, y el historial del escritorio como su fuente

Status: **done** — T1–T5 implementadas y verificadas; **sin commitear**, porque los commits los autoriza el lector (L69 de `pdf-progress-tracker.md`)
Created: 2026-11-02
Branch: `feat/phase-1-core`
Supersedes T12 of `odd/tasks/pdf-progress-tracker.md` (axis A), which was blocked on a verdict and now
has one: the reader wants the history read **and** a screen to review what it finds.

## Specs

**S1 — la pantalla, en palabras del lector (L1, verbatim).**
> "Si, tiene que aparecer una lista de sugerencias con los mas recientes. Esta pantalla debe ser similar
> a la Biblioteca, pero con sugerencias. Debe tener paginacion, filtros de busqueda, titulo y portada
> preestablecidas. Obviamente, no se podra editar los datos de los documentos sugeridos"

La pantalla muestra la cola de sugerencias pendientes como una grilla de tarjetas, con la misma
composición que Biblioteca: una tarjeta por documento, con portada, título, la ruta como dato de
contexto, y las dos decisiones (agregar / descartar) sobre la tarjeta. Es una vista propia
(`#/suggestions`), no un panel dentro de otra pantalla.

**S2 — paginación.** La grilla pagina con la misma función que Biblioteca (`paginate`), con el mismo
comportamiento observable: el paginador no se dibuja cuando hay una sola página (regla que
`ui-capture-loop.md` S3.2 ya fijó para Biblioteca y que vale acá por la misma razón), y cambiar de
página devuelve el scroll arriba (L73 de `pdf-progress-tracker.md`).

**S3 — filtros de búsqueda.** Un campo de búsqueda filtra por título preestablecido (S5) y, cuando el
título no alcanza, por la ruta; sin distinguir mayúsculas ni acentos. *(Corregido al implementar, L8: esta cláusula decía
"como el buscador de Biblioteca", y eso es falso — `matchesQuery` en `library.ts:947` sólo pasa a
minúsculas, no pliega acentos —. Hoy las dos búsquedas difieren justo en acentos, que en español no es un
detalle; unificarlas es trabajo aparte, no de esta feature.)*
Un control de orden ofrece el orden por defecto (S4) más el inverso y por nombre, igual que el
selector de orden de Biblioteca. No hay filtro por autor: una sugerencia no tiene autores, y ofrecer
un filtro que siempre vacía la grilla es exactamente lo que el proyecto evita.

**S4 — "los más recientes" significa cuándo lo abrió el lector (decisión del lector, L2).** El orden
por defecto es la fecha que el historial del escritorio registró para ese documento, descendente. Los
documentos sin fecha de historial van después de todos los que tienen una, porque ordenarlos como 1970
sería afirmar algo falso — la misma regla que `pdf-progress-tracker.md` S5 de T12. El desempate es por
ruta, para que dos corridas no discrepen.

**S5 — título y portada preestablecidos, y de sólo lectura.** El título sale del nombre del archivo
(`suggestionTitle`, ya existente) y la portada de la primera página del PDF (`suggestion_cover`, ya
existente). Los dos se muestran; ninguno se edita. Un archivo que ya no existe o no se puede renderizar
conserva el marcador de posición que esas dos piezas ya implementan, y no inventa un título ni una
imagen.

**S6 — nada de una sugerencia es editable (L1, verbatim: "no se podra editar los datos").** La tarjeta
no tiene editor de título, ni de autores, ni de portada, ni de páginas: no hay control de edición y el
comando que lo haría no se expone para sugerencias. Lo único que el lector puede hacer con una
sugerencia son las dos decisiones que ya existen.

**S7 — las decisiones no cambian.** Agregar la incorpora a la biblioteca por el camino que ya existe
(contar páginas y persistir) y descartarla la marca como decidida. Nada entra a la biblioteca sin la
decisión del lector: la pantalla nueva es una superficie distinta para decidir, no una regla nueva.

**S8 — se entra desde Biblioteca, con un contador (decisión del lector, L3).** Biblioteca muestra un
acceso con el número de sugerencias pendientes, y de ahí se entra a la pantalla. La barra de navegación
no gana una cuarta entrada: en ventana chica ya tiene tres y el ancho es el recurso escaso. Sin
sugerencias, el acceso dice que no hay nada que decidir en vez de desaparecer sin explicación.

**S9 — el panel de Ajustes se muda (decisión del lector, L4).** La sección de sugerencias dentro de
Ajustes → Biblioteca se retira: la pantalla nueva es el único lugar donde se decide. Ajustes → Biblioteca
conserva las carpetas vigiladas y el escaneo. Dos lugares que muestran la misma cola son dos lugares
que se desincronizan.

**S10 — la fuente: el historial de archivos recientes del escritorio.** Absorbe T12 de
`pdf-progress-tracker.md`. El historial (`~/.local/share/recently-used.xbel`) se lee con el parser que
ya existe (`crates/mirk-platform/src/recent.rs`, en uso desde L46) a través de
`RecentDocumentsSource::recent` (`crates/mirk-platform/src/linux.rs:268`, hoy `NotSupported`), y cada
PDF que el historial nombra y no está ni en la biblioteca ni dentro de una carpeta vigilada se registra
como sugerencia. Reglas heredadas de la spec de T12, que siguen valiendo: un PDF es su mime-type o su
extensión `.pdf`; un archivo que ya no existe se omite; un historial ausente es una lista vacía y uno
ilegible es un error; `NotSupported` queda sólo donde no hay backend. **Lo único nuevo en los datos** es
la fecha del bookmark, que se guarda como su propio hecho — nunca se reutiliza `first_seen_at`, porque
L46 ya demostró una vez qué pasa cuando "cuándo lo miré yo" y "cuándo lo abrió el lector" comparten
columna.

**S11 — textos en los dos idiomas.** Toda cadena nueva vive en el catálogo (`web/src/lib/i18n.ts`), en
inglés y español; nada incrustado en el marcado.

**S12 — puerta de verificación.** Cada regla de arriba queda pinneada por un test sobre comportamiento
observable, no sobre estado interno. La migración se prueba sobre una base con datos (copia, nunca la
del lector) y es idempotente. `cargo test`, `clippy --all-targets -D warnings`, `bun run typecheck` y
`bun run test` en verde, y la pantalla nueva entra en `scripts/capture-ui.mjs` con sus comandos
mockeados, porque un comando sin respuesta en el arnés deja la captura en la ruta de error (lección de
L9 de `ui-capture-loop.md`).

## Tasks

| ID | Specs | Task | Route | Commit |
| --- | --- | --- | --- | --- |
| T1 | S10, S4 | Historial → sugerencias, con la fecha del bookmark en su propia columna (migración V11) | worker + T1b | — |
| T2 | S1, S2, S3, S5, S6, S7, S11 | La pantalla: grilla, búsqueda, orden, paginación, tarjetas de sólo lectura | worker | — |
| T3 | S8 | Acceso desde Biblioteca con el contador de pendientes | inline | — |
| T4 | S9 | Retirar el panel de sugerencias de Ajustes → Biblioteca | worker + inline | — |
| T5 | S12 | Capturar la pantalla nueva en el arnés y verificar el conjunto | inline | — |

`Risk: item 1` (migración) para T1; el resto `Risk: none` salvo que el diff toque algo compartido.

## Log

**L1** — "Si, tiene que aparecer una lista de sugerencias con los mas recientes. Esta pantalla debe ser
similar a la Biblioteca, pero con sugerencias. Debe tener paginacion, filtros de busqueda, titulo y
portada preestablecidas. Obviamente, no se podra editar los datos de los documentos sugeridos"

**L2 (decisión)** — Preguntado "¿recientes según qué?", el lector eligió **"Por cuándo lo abriste (fecha
del historial)"** sobre "por cuándo la app lo detectó". Consecuencia: la fecha del bookmark es un dato
persistido, y la migración es el trabajo que esa elección obliga.

**L3 (decisión)** — Preguntado cómo se llega, eligió **"Se entra desde Biblioteca, con un contador"**, y
no una cuarta entrada en la barra.

**L4 (decisión)** — Preguntado qué pasa con el panel de Ajustes, eligió **"Se muda: la pantalla nueva es
el único lugar"**.

**L5 (dependencia resuelta)** — T12 estaba bloqueado por un veredicto abierto (L75 de
`pdf-progress-tracker.md`): L63 registraba *"que no descubra, puede abrumar al usuario con tantos
documentos"*. Preguntado en simple —si quería que mirk leyera el historial— el lector respondió que sí, y
pidió esta pantalla. El veredicto de L63 apuntaba a otra cosa; queda superado por L1.

**L6 — la verificación independiente dijo "no satisfecho", y tenía razón en dos cosas.** T1 volvió con 365
tests en verde y clippy limpio, pero un verificador que sólo leyó el diff marcó **dos blockers** y un
should-fix. Los dos primeros son reales y los confirmé leyendo el código antes de mandar el arreglo:

1. **La ventana del historial se aplicaba antes de las exclusiones** (`commands.rs:943`). El productor
   pedía `recent(1_000)` y recién después descartaba lo que ya estaba en la biblioteca o dentro de una
   carpeta vigilada. Una historia con más de mil entradas, o con las mil más nuevas ya conocidas, perdía
   sugerencias elegibles para siempre — contra S10, que dice *todos* los PDFs elegibles.
2. **Las omisiones por archivo muerto no se contaban** (`linux.rs:356`). La spec de T12 (S4) no pide sólo
   omitir: pide contar la omisión. El proyecto ya tiene esa regla escrita en otro lado —`RecentDocuments::len`
   existe porque "una historia vacía en silencio" y "una historia bien poblada" no pueden verse igual desde
   afuera— y acá se había perdido.
3. **Una fecha futura ordenaba para siempre.** El verificador lo marcó should-fix y no blocker, porque la
   spec no lo prohibía; yo lo traté como defecto igual, por la misma familia que L46 ya corrigió una vez.
   La decisión **no** fue recortar la fecha al presente: recortar inventa un momento, que es exactamente lo
   que el proyecto se niega a hacer. Un bookmark fechado en el futuro se trata como **sin fecha usable**: el
   documento sigue apareciendo (T12 S3: un camino sin hora es un documento) y se ordena después de todos los
   fechados, igual que uno que nunca tuvo `modified` ni `visited`.

El arreglo fue a un worker propio (T1b), con el borde de la regla nombrado: un tiempo **igual** a `now` es
usable, uno un segundo después no.

LO QUE EL VERIFICADOR NO PUDO HACER, dicho por él: no corrió ni un test, ni clippy, ni un build — no tenía
comando autorizado —, así que su veredicto es de lectura y su "los 14 tests pasan" quedó como *no
verificado* (los corrió el worker: 365 en verde). La puerta real sigue siendo la corrida del conjunto.

LA MEDICIÓN QUE FALTABA (T12 S7), hecha por el padre con un control en vez de a ojo: el binario del ejemplo
con `XDG_DATA_HOME` vacío —historial ausente, sin parseo— tarda **2 ms**; con el historial real de **706 KB,
1000 bookmarks y 991 entradas `file://`** tarda **18 ms**. Costo de una lectura completa: **~16 ms**, cinco
corridas, mínimo de cada una. Es una lectura por llamada, sin caché, y a ese precio no hace falta ninguna.

RESIDUAL ACEPTADO: el productor se dispara a pedido de la interfaz, no en el tick de ingesta. Un tick cada
pocos segundos inflaría `seen_count` de cada sugerencia en cada pasada, que es justo lo que ese contador no
significa. La condición que lo hace aceptable es que la interfaz lo pida **antes** de confiar en el contador
—eso pasó a ser requisito explícito de T2 y T3—.

**L7 — T1 cerrada: puerta del padre en verde, y tres residuales con nombre.** T1b devolvió los tres arreglos
con rojos de verdad, y no con la palabra "TDD":

- ventana: `left: 0, right: 1` — la entrada más nueva ya estaba en la biblioteca y se comía la ventana
  entera. El defecto, en una línea.
- fecha futura: `left: [2099.pdf, dated.pdf, undated.pdf]` contra
  `right: [dated.pdf, 2099.pdf, undated.pdf]`. El 2099 ganaba el orden, que es exactamente lo que el
  verificador predijo.
- el conteo de omisiones **no tuvo RED comportamental y el worker lo dijo así**: un campo que no existe no
  puede observarse mal. Lo que sí hizo fue el triangulado que importa —el fixture incluye un `notes.txt`
  muerto y el conteo sigue siendo 1, o sea "omitido" significa "era candidato y su archivo no está", no
  "cualquier bookmark cuyo archivo no está"—.

PUERTA DEL PADRE, corrida por mí y no tomada del reporte: `cargo test --workspace` **368 tests en 13 targets,
0 fallos**; `cargo clippy --workspace --all-targets -D warnings` **0 warnings, 0 errores**. Revisé además los
dos hunks que deciden: el guard de la ventana está arriba del loop y se gasta con `recorded.len()`, así que
los descartes no consumen presupuesto; y el filtro de fecha es
`last_used_unix.filter(|time| *time <= now)`, con el `<=` que la spec nombró como borde.

No pedí una segunda verificación independiente: T1b no tocó la migración, que era el único ítem de riesgo
alto, y los dos blockers quedaron pinneados por rojos observados.

TRES RESIDUALES, anotados en vez de escondidos. (1) Una fila **ya decidida** que el historial sigue nombrando
gasta un lugar de la ventana y suma al número que devuelve el comando, aunque el almacén no la toque; el test
de contrato que lo fija (`a_decided_history_document_is_not_offered_again`) sigue valiendo 1 y cambiarlo
habría cambiado su significado. Consecuencia operativa, ya mandada al worker de T2 como restricción: **el
contador que ve el lector sale de `list_suggestions`, nunca del valor que devuelve `observe_recent_documents`**,
que es diagnóstico de la pasada. (2) `unix_now()` devuelve `i64::MAX` si el reloj no se puede situar después
de 1970: es "no conozco cota superior", no un momento inventado, pero es un juicio que la spec no tomó. (3) La
línea de stderr con las omisiones es ruido nuevo en una máquina con bookmarks muertos —5 de 34 acá—: una línea
por llamada, sólo cuando el conteo es positivo.

**L8 — T2 y T3 cerradas, y una cláusula mía que era falsa.** La pantalla salió con 437 pruebas (396 + 41),
typecheck 0/0 y build ok, corridos por mí; el acceso desde Biblioteca con 441 (las 437 más cuatro: tres de las
tres oraciones del acceso y una del clic real que navega). La regla que más fácil se rompe la revisé a mano —en
el código, no en el reporte—: en `sortSuggestions` la ausencia de fecha **no se invierte con la dirección**
(`oldest` no puede convertir "no sé cuándo" en "lo más viejo"), y el desempate por ruta existe para que dos
corridas sobre una misma cola no discrepen. La carga también: `loadSuggestionQueue` corre una vez al arrancar
y el tick de 15 s sólo relee la cola.

EL WORKER ENCONTRÓ QUE MI SPEC MENTÍA, y lo dijo en vez de obedecer: S3 pedía "sin distinguir acentos, como
el buscador de Biblioteca", pero `matchesQuery` sólo hace `toLowerCase`. El plegado de acentos quedó en
`suggestions.ts` (`foldForSearch`: NFD y marcas combinantes fuera) y **las dos búsquedas hoy difieren**, justo
en acentos, que en español no es un detalle. Corregí S3 para que no siga afirmando algo falso, y queda como
trabajo aparte: unificar los dos buscadores, con la decisión —del lector— de si Biblioteca gana el plegado.

EL ACCESO (S8), tres oraciones y no dos: `null` es "la cola no contestó", `0` es "no hay nada", y un número es
una cola. Botón de verdad, alcanzable con teclado, que sigue ahí cuando la biblioteca está vacía o falló
—quien no tiene nada en la biblioteca es quien más necesita la cola— y habilitado también con cero, porque la
pantalla que explica qué es una sugerencia se abre. El contador sale de `list_suggestions` y de ningún otro
lado. Lo que costó una vuelta: `LibraryScreen.dom.test.ts` monta la pantalla en seis lugares con props
escritas a mano, cuatro con `onResetScroll: () => {}` y dos con la forma corta, y las dos formas hubo que
encontrarlas por el error de tipos, no por el archivo.

LA LECCIÓN DE L9, EVITADA ANTES DE QUE MORDIERA. Con S10 el arranque pide `observe_recent_documents`; el arnés
de captura no lo contestaba, así que **toda captura habría fallado con "unmocked command"** y el exit 1 habría
aparecido recién al final, después de escribir las imágenes. Se mockeó antes de correr nada, junto con
`suggestion_cover` y tres sugerencias sembradas —una sin `historyAt`, porque "ninguna fuente dijo cuándo" es
un hecho que la pantalla también tiene que dibujar—, y la pantalla nueva entra a la matriz de captura por su
dirección (`#/suggestions`), porque no tiene entrada en la barra.

**L9 — T4 y T5 cerradas, y dos bugs míos en el arnés que valen más que el feature.** El worker de T4 entregó verde
(437 pruebas) y **preguntó en vez de romper un test**: `suggestion.definition` quedó sin lector al irse el panel, y
su única referencia restante era una lista de claves en `suggestions.test.ts`, fuera de sus superficies. La opción
correcta era la segunda: extender la superficie y borrar también `suggestion.deciding`, que **ya estaba muerta antes
de esta feature** (ni el panel ni `SuggestionCover` la dibujaban nunca). Lo hice yo: dos claves fuera de los dos
catálogos y de la lista, y el total siguió en 437 —no se agrega ni se quita una prueba, se saca una clave de una
lista—. El worker además reportó, sin tocarla, otra deriva que sí llegaba al lector: las dos frases del índice de
Ajustes que decían que esa sección tiene "la cola de sugerencias". Las reescribí en los dos idiomas. Es la tercera
vez en esta sesión que el mismo tipo de línea miente (roadmap del README, checklist de T12, índice de Ajustes):
cuando una pieza se muda, se mudan también las frases que la describen.

T5 CERRÓ CON DOS DEFECTOS EN EL ARNÉS, LOS DOS MÍOS. La primera corrida salió exit 0, sin problemas, con la
pantalla y el orden correctos en la imagen — el más nuevo arriba, el sin fecha al final — pero las portadas
decían "No cover yet": el seed preguntaba `args.document` y la tarjeta manda `{ path }` (`SuggestionCover.svelte:32`).
Al corregirlo escribí un comentario **con backticks dentro del template literal del seed**, y eso cerró el string:
el script no compilaba siquiera. Es la misma familia que L67 del registro —un bloque de texto que cambia de alcance
en silencio— y por eso el bloque ahora lleva escrito que ahí no van backticks. Segunda corrida: portadas dibujadas,
las tres tarjetas con sus dos decisiones, cero controles de edición.

NÚMEROS FINALES. Interfaz: **437 pruebas en 25 archivos**, `typecheck` 0 errores y 0 warnings, `build` ok (153
módulos). Núcleo: **368 pruebas en 13 targets**, clippy `--all-targets -D warnings` en 0 (sin cambios desde T1b,
así que siguen vigentes). Arnés: exit 0, `problems: []`, cinco pantallas capturadas.

RESIDUALES. (1) Las dos búsquedas difieren en acentos (S3 corregida, L8): unificar es trabajo aparte y la decisión
de si Biblioteca gana el plegado es del lector. (2) La frase de error de lectura y la de decisión quedaron separadas
por el worker de T4; cualquier superficie nueva que lea la cola tiene que usar la de lectura. (3) Nada está
commiteado: cuando el lector apruebe, la propuesta es **tres commits** — arnés y documentación; el dato (plataforma
+ migración V11 + productor); y la interfaz (pantalla, acceso, mudanza del panel) — porque juntos son ~1.500 líneas
y el slice de revisión es de ~400.

**L10 — dos reportes del lector con la aplicación en la mano, los dos de composición.**

> "La posicion del boton de sugerencias es mala. No me gusta que ocupe toda una fila el solo."
> "La pantalla sugerencias, no tiene un boton para volver a la biblioteca directamente. Tengo que cliquear en el
> navbar inferior para volver a la biblioteca."

EL BOTÓN OCUPABA UNA FILA POR UN DEFECTO ESTRUCTURAL, no de estilo: `.screen` es una columna flex, así que un
botón suelto se estira al ancho completo y se lee como una banda. Ahora vive dentro de `.title-row`, al lado del
título y el contador, con `flex: none`, y el título tiene `min-width: 0` para que un título largo se envuelva en
vez de empujar la acción fuera de la fila. El test que lo fija es **estructural a propósito**
(`class="title-row" … suggestions-entry`): el defecto era estructural y ninguna aserción de texto lo habría visto.
Verificado en las dos ventanas que importan: 634×844 (tres por fila) y 430×844 (dos por fila, la más angosta),
las dos con el botón arriba a la derecha y el título sin desbordar.

LA VUELTA COPIÓ EL PATRÓN QUE YA EXISTÍA, que es el de las sub-pantallas de Ajustes: mismo ícono, mismo
`aria-label` que nombra el destino, mismo estilo. El destino es **Biblioteca** y no "la pantalla anterior": la
única que lleva acá es Biblioteca, y adivinar una historia sería inventar. Pasa por `selectScreen("library")`, así
que el hash se actualiza y Atrás y una recarga siguen coincidiendo con lo que se ve.

VERIFICACIÓN. Interfaz en **440 pruebas** (437 más tres: la fila del título, el botón de vuelta en la pantalla y
la vuelta real desde el shell), `typecheck` 0/0, `build` ok, y dos capturas corridas con exit 0 y `problems: []`.
La aplicación quedó viva durante todo el cambio (pid 243678, HMR de Vite), que es como el lector pudo reportar y
como puede ver el arreglo sin reiniciar.






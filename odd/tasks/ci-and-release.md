# Feature: CI, empaquetado y releases

Status: **in progress** — planificado y autorizado el 2026-11-02; Fase 0 y 1 en curso
Created: 2026-11-02
Branch: `feat/phase-1-core`
Decisiones del lector: sólo Linux publica artefactos; actualización automática desde la v0.1; repo
público; `v0.1.0` con SemVer y marcado pre-release.

## Specs

**S1 — CI en cada push a `main` y en cada PR (`ci.yml`).** Cuatro jobs, con `concurrency` que cancela
corridas viejas del mismo ref:
1. `rust` — `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
   `cargo test --workspace`. Es la puerta que hoy no existe: 368 pruebas que sólo corren si alguien se
   acuerda.
2. `web` — `bun run typecheck` (tsc + svelte-check), `bun run test`, `bun run build`. Otras 440.
3. `compile` — matriz `windows-latest` + `macos-latest`, **sólo `cargo check`**: atrapa lo que rompe por
   `cfg(target_os)` sin pagar runners de 10× minutos en tests que ya corren en Linux.
4. `capture` — `node scripts/capture-ui.mjs`. Los runners traen Chrome, y el arnés sale 1 ante cualquier
   `console.error`, así que es un smoke test real de la interfaz: es el job que habría atrapado el
   `unmocked command` de L9 sin que nadie mirara una imagen.

**S2 — El runner se fija, no se deja en `latest`.** `ubuntu-24.04` explícito: trae `libwebkit2gtk-4.1`
y `24.04+` ya no garantiza esa versión en adelante (25.04 pasó a 4.1→6.0), así que `ubuntu-latest`
convertiría cada release del runner en una rotura. Dependencias: `libwebkit2gtk-4.1-dev`,
`libgtk-3-dev`, `libsoup-3.0-dev`, `librsvg2-dev`, `libayatana-appindicator3-dev`, `patchelf`,
`build-essential`. Caché: `Swatinem/rust-cache` para Cargo y una caché de `vendor/pdfium` con clave
derivada del hash de `scripts/fetch-pdfium.sh`, porque la librería es de ~8 MB y su origen puede
cambiar.

**S3 — El release publica sólo Linux (decisión del lector, L2).** Disparado por tag `v*`:
`.deb`, `.rpm`, `.AppImage`, más el tarball de fuentes que GPL-3.0 exige y `THIRD-PARTY-NOTICES`, más
`SHA256SUMS`. Se crea como **draft**: publicar es una promoción humana, no un efecto de empujar un tag.
El release **no** se marca `prerelease` (L7, decisión del lector que revierte la parte de L3b que hablaba de
la marca): GitHub sirve `/releases/latest/download/…` desde el último release publicado y **no** marcado, así
que la marca escondería del updater el único release que existe. No se pierde nada: `0.1.0` ya dice pre-1.0
por SemVer y el cuerpo del release lo dice con palabras.

**S4 — La versión tiene una sola fuente.** Hoy `0.0.1` vive en `Cargo.toml`, `tauri.conf.json` y
`package.json`, y un release que actualiza dos de tres publica un binario que miente sobre sí mismo. Un
script sincroniza las tres desde `tauri.conf.json`, y un check en CI falla si discrepan.

**S5 — `pdfium` viaja dentro del paquete (bloqueante, L1).** En un paquete instalado no existe
`vendor/pdfium` ni `$MIRK_PDFIUM_LIB`, así que la resolución actual dejaría la app publicada sin poder
contar páginas. La librería se declara en `bundle.resources` y se resuelve, en una app empaquetada,
desde `app.path().resource_dir()`; en desarrollo sigue ganando `vendor/pdfium`, y `$MIRK_PDFIUM_LIB`
conserva su prioridad. Una librería ausente sigue siendo un error que nombra el remedio, nunca un
recuento de cero.

**S6 — Iconos y bundle reales.** `tauri.conf.json` pasa a `bundle.active: true`, con targets de Linux y
los metadatos que el `.desktop` necesita. El icono actual es un PNG de 362 bytes; el set completo
(`.icns`, `.ico`, 32/128/256) se genera con `bun tauri icon` desde una fuente y se commitea.

**S7 — Actualización automática desde la v0.1 (decisión del lector, L2b).** Plugin updater configurado,
clave **pública** en `tauri.conf.json`, clave **privada** sólo en los secretos de CI
(`TAURI_SIGNING_PRIVATE_KEY`). `tauri-action` firma los artefactos y publica el `latest.json` que la app
consulta. La clave privada nunca entra al repo y **debe respaldarse**: si se pierde, las copias ya
instaladas dejan de poder actualizarse, y eso no se arregla con un release nuevo.

**S8 — Avisos de terceros.** `THIRD-PARTY-NOTICES.md` (el `.md` lo decidió L5, porque el archivo se lee en
GitHub mucho más de lo que se descarga) cubre pdfium (BSD-3-Clause), pdf.js (Apache-2.0, cuando llegue el
lector), WebKitGTK (LGPL-2.1+ como librería de sistema), WebView2 (propietario de Microsoft, documentado
aunque todavía no se empaquete) y las dependencias Rust relevantes. Se publica como asset del release y el
release **falla** si el archivo no está: un paquete que sale sin sus avisos es el defecto que los avisos
existen para evitar.

**S9 — Puerta.** Los workflows se validan (sintaxis YAML y `actionlint` si está disponible), el CI queda
verde en el PR que estrena la feature, y un tag real produce un draft con todos los artefactos —eso se
verifica una vez, a mano, con el resultado copiado en el log—. Nada se publica sin promoción humana.

**S10 — Fuera de alcance, a propósito.** Publicar en repositorios de paquetes (PPA, AUR, Flathub),
firma de Windows, notarización de macOS y artefactos para plataformas cuyos backends devuelven
`NotSupported`. Cada uno es una decisión con costo propio.

## Tasks

| ID | Specs | Tarea | Ruta |
| --- | --- | --- | --- |
| T1 | S5, S6 | pdfium como recurso del bundle + resolución desde `resource_dir`; iconos y `bundle.active` | worker |
| T2 | S1, S2 | `.github/workflows/ci.yml` con los cuatro jobs | worker |
| T3 | S3, S4, S9 | `.github/workflows/release.yml` + script de versión única y su check | worker |
| T4 | S8 | `THIRD-PARTY-NOTICES` + `CONTRIBUTING.md` | inline |
| T5 | S7 | Plugin updater, claves y secreto de CI | inline |
| T6 | S9 | Verificación: CI verde y un draft de release real | inline |
| T7 | S1 | `cargo fmt` bloqueante: el árbol formateado y el `continue-on-error` quitado | inline |

## Commits

El árbol que cargaba T1–T8 se repartió el 2026-11-02 en siete commits más el de versión, partiendo de una
etiqueta `wip-before-t6-slices` que congela todo antes de tocar nada. Qué guarda cada uno:

| Commit | Contenido | Tareas |
| --- | --- | --- |
| `b4454e0` | `style: rustfmt formats the workspace` | T7 |
| `a4872f2` | `feat(scan): the pdfium library travels inside the package` | T1 |
| `ad15f15` | `ci: gate every push, and keep the version in one place` | T2, T4 |
| `6f76e47` | `feat(platform): the desktop's own history feeds the suggestion queue` | otra feature |
| `aab8ecb` | `feat(suggestions): the queue gets a screen of its own` | otra feature |
| `3e8e99c` | `feat(release): signed releases the app can update itself from` | T3, T5 |
| `c5b3c0e` | `feat(ui): Ajustes tells the reader when a newer version exists` | T5 |
| `209b871` | `chore(release): the first release is 0.1.0` | preparación de T6 |

`Risk: item 1` para T1 (si la resolución del recurso está mal, la app publicada cuenta cero páginas en
silencio) y `Risk: item 5` para T3/T5 (entrega: workflows, firma y secretos).

## Log

**L1 (hallazgos de la investigación, 2026-11-02)** — Cuatro hechos del repo cambian el orden de la
propuesta, y uno es bloqueante: `resolve_pdfium_library` sólo conoce `$MIRK_PDFIUM_LIB`,
`vendor/pdfium` y la ruta del sistema, y en un paquete instalado no existe ninguna de las tres. Los otros
tres: un único icono de 362 bytes con `bundle.active: false`; la versión en tres archivos; y que
Windows/macOS compilan pero devuelven `NotSupported` en todos sus backends. Del ecosistema:
`tauri-action` es la vía oficial (tag → draft) y `bundle.resources` es el mecanismo para empaquetar una
`.so`/`.dll` (no `externalBin`, que es para ejecutables).

**L2 (decisiones del lector)** — Preguntado qué plataformas publican, eligió **"Sólo Linux por ahora"**,
con el preview que incluía `.deb`, `.rpm`, `.AppImage`, tarball de fuentes y avisos. **L2b** — Preguntado
por la actualización, eligió **"Actualización automática desde la v0.1"**, no la descarga manual.

**L3 (decisiones del lector)** — **L3a** — Repo **público**, y **primero ordenar la entrega de la rama**
antes del primer tag. **L3b** — Formato **`v0.1.0` con SemVer y pre-release**.

**L4 — T4 escrita, y los números del aviso de terceros estaban mal por mirarlos en vez de calcularlos.**
`THIRD-PARTY-NOTICES.md` y `CONTRIBUTING.md` escritos. El inventario de licencias salió de
`cargo metadata` (479 paquetes) y no de memoria — lo que encontró algo que el plan no tenía anotado: el
único copyleft de terceros es **MPL-2.0** en cinco paquetes (`cssparser`, `cssparser-macros`, `dtoa-short`,
`option-ext`, `selectors`), que es copyleft de archivo y compatible con GPL-3.0; y **ISC** en tres
(`inotify`, `inotify-sys`, `libloading`), que es el que ya había obligado a corregir una nota del plan.
Ningún otro sale del conjunto permisivo.

EL ERROR QUE VALE ANOTAR: la primera versión de la tabla agrupaba familias **estimando a ojo sobre la salida**
(271, 8, 7, 19), y los grupos no sumaban 479. Se detectó restando el total, no leyendo. Reescrita con un
script que clasifica cada cadena y suma: 287 + 107 + 22 + 18 + 11 + 10 + 6 + 5 + 5 + 3 + 2 + 2 + 1 = 479.
Es exactamente el mismo defecto que el registro documenta con el conteo de pruebas tras un recorte (L67):
**un total que no se verifica es una afirmación, no un dato**. El archivo ahora lleva el comando para
reproducirlo y la frase de que los conteos se vuelven a correr, no se confían.

También se evitó inventar la versión de SQLite (la primera redacción decía "3.46" sin haberla mirado): el
aviso dice lo que sí consta, que la compila `libsqlite3-sys` 0.30.1 con la feature `bundled`, y que SQLite
es de dominio público.

**L5 — T2/T3 entregadas, revisadas por mí, y un hallazgo que no esperaba: el árbol nunca se formateó.**
`ci.yml` con los cuatro jobs (`rust`, `web`, `compile` en windows+macos sólo con `cargo check`, `capture`
como smoke **bloqueante**), `release.yml` con draft + prerelease y los assets extra, y
`scripts/version-sync.sh` con `--check`, `--print` y `--tag`.

LO QUE VERIFIQUÉ YO, no tomado del reporte: los dos YAML parsean con un loader que **falla ante claves
duplicadas** (el error clásico de `on:`/`jobs:` repetidos pasa desapercibido a un parser tolerante), con
los jobs y la cantidad de pasos que dice el reporte; y el script de versión lo corrí: `--check` coincide,
`--tag v0.0.1` pasa, `--tag v9.9.9` sale 1. Eso es todo lo verificable sin empujar el repo: **la corrida
real es T6**, y está por eso.

LA DECISIÓN QUE DEJÓ ABIERTA, TOMADA: el nombre de los avisos. El worker aceptaba las dos variantes
(`THIRD-PARTY-NOTICES` y `.md`), publicaba una y **avisaba con un warning en cada release**. Aceptar dos
nombres es publicar el que exista, y un warning que sale siempre es un recordatorio de que nadie decidió,
no una decisión. Queda **`THIRD-PARTY-NOTICES.md`**, el release falla si no está, y el paso dejó de tener
ramas. Actualicé S8 con él.

EL HALLAZGO: `cargo fmt --all --check` **sale 1** — 19 archivos, 3.609 líneas de diff (89 hunks en
`mirk-store/tests/store.rs`, 36 en `mirk-store/src/lib.rs`, 35 en `track.rs`). El árbol nunca se formateó,
así que un gate de formato no podía activarse: habría pintado de rojo todos los PR desde el primero. El
worker lo dejó como paso **no bloqueante con el número medido y el remedio escritos en el archivo**, que es
la decisión correcta para esta unidad. **No lo formateé ahora a propósito**: T1 está editando `crates/` en
este momento y `cargo fmt --all` reescribiría archivos bajo sus manos. Queda como **T7**: un commit de sólo
formato (nadie revisa 3.600 líneas mecánicas a mano, y la suite completa prueba que no cambió comportamiento)
y después se quita el `continue-on-error`.

RESIDUALES ANOTADOS DEL REPORTE. `dtolnay/rust-toolchain@stable` es una referencia móvil (el toolchain del CI
es el stable del día, y un cambio de rustfmt en upstream cambiaría el diff — hoy inocuo porque el paso no
bloquea); `oven-sh/setup-bun@v2` con `bun-version: 1.x` resuelve por rango; `tauri-apps/tauri-action` está
fijada a **un SHA** y no a un tag (es a la única acción que se le entrega la clave de firma y `contents:
write`, y un tag se puede re-apuntar mientras un SHA no), con la nota de que v1.0.0 renombró
`includeUpdaterJson` a `uploadUpdaterJson`. Y el `compile` de Windows/macOS está razonado contra el código
de `tauri-build` 2.7.2 (no necesita `web/dist` en `cargo check`), no ejecutado: si rompe, rompe en el primer
PR, que es exactamente para lo que el job existe.

**L6 — T1 y T7 cerradas: el paquete se construye, y pdfium viaja adentro.**

T1 LO HIZO CON LA ÚNICA EVIDENCIA QUE VALE para un problema de empaquetado: **construyó los tres paquetes**
(`.deb` 8,9 MB, `.rpm` 8,8 MB, `.AppImage` 89 MB) y verificó la librería **dentro** de cada uno. Lo confirmé
yo, no lo tomé del reporte: `dpkg-deb -c` muestra `usr/lib/Mirk/pdfium/libpdfium.so` con sus 8.049.792
bytes, y el `.desktop` generado tiene `Categories=Office;`, `Exec=mirk-desktop` y el comment que sale de
`shortDescription`. El `AppImage` lleva la misma ruta, y el rpm la declara en su tabla de archivos.

LA RESOLUCIÓN QUEDÓ BIEN, y la revisé en el código: `explicit` → `packaged` → `vendor` → sistema, con el
candidato empaquetado **sólo si es un directorio que existe**. Ese `is_dir()` es lo que mantiene el
desarrollo intacto: en dev la ruta de recursos resuelve a `target/debug`, que no tiene `pdfium/`, y el
candidato se saltea solo. La inyección pasa por un `OnceLock` en `bind_pdfium_specifying`, así que
`commands.rs` y `watch.rs` no cambiaron y sus tests tampoco.

SOBRE LA SUPERFICIE QUE EXTENDÍ: el worker **no la necesitó**. Prefirió lo aditivo —una función nueva con el
tercer candidato, y la de dos argumentos delegando— así que `crates/mirk-scan/tests/scanner.rs` quedó sin
tocar, que es exactamente lo que la instrucción pedía preferir. Vale anotarlo: extendí la superficie por las
dudas y el resultado correcto fue no usarla.

T7, EL FORMATO. `cargo fmt --all` sobre el árbol: **20 archivos, 2.701 inserciones y 544 borrados**, todos
mecánicos. `cargo fmt --all --check` ahora sale **0**, los **13 targets** de `cargo test` siguen en verde y
clippy sigue limpio — o sea que formatear no cambió comportamiento, que es lo que un commit de sólo formato
tiene que demostrar. Recién entonces el paso de CI pasó de `continue-on-error` a **gate bloqueante**, y el
comentario del workflow cuenta el antes y el después en vez de prometer un futuro.

RESIDUALES. (1) `bun tauri icon` generó también los sets de Android e iOS (~40 archivos) y quedaron
commiteados: inofensivos, pero no son parte del producto y el día que molesten se borran en una línea. (2)
La prueba de que un paquete instalado **cuenta páginas** es estructural (la ruta del recurso coincide con la
que `tauri-utils` resuelve para un build instalado en Linux), no de ejecución: no hay `sudo` ni display acá
para instalar y abrir el `.deb`. Es la única cosa de esta fase que el lector puede cerrar instalándolo.


**L7 — T5 en curso: el updater está cableado hasta la clave, y una decisión del lector mudó S3.**

LAS DOS DECISIONES. Preguntado por el endpoint, el lector eligió **"Publicar sin marca pre-release desde
v0.1.0"**: `releases/latest/download/latest.json` sólo resuelve al último release publicado y **sin** la marca,
así que con v0.1.0 marcado el updater habría quedado mudo justo en la única versión que hoy existe. Eso
revierte la parte de L3b que pedía la marca de GitHub; el "pre-release" sigue dicho por el `0.x` de SemVer y
por el cuerpo del release. S3 quedó reescrito y `release.yml` pasó de `prerelease: true` a `false` en sus dos
pasos de build, con el porqué en el encabezado del archivo.

LA SEGUNDA: el chequeo se ve **discreto**, sin modal — un punto en la fila de Aplicación del índice de Ajustes,
y el estado completo adentro. La tercera no fue una elección sino una espera: la clave pública la pega el
lector, así que T5 queda abierta exactamente en el valor que falta.

LO QUE YA ESTÁ, y con qué evidencia. `tauri-plugin-updater` 2.14.1 y `tauri-plugin-process` 2.4.0 en los dos
lados (Cargo y bun), registrados en el builder; la capability del main gana `updater:default` y
`process:allow-restart` **y no** `process:allow-exit`, porque entrar a una actualización verificada y cerrar la
aplicación son poderes distintos y sólo el primero tiene quien lo llame. La regla vive en
`web/src/lib/update.ts` (seis estados, las palabras de cada uno, el porcentaje de la descarga con su clamp y su
`null` cuando el manifest no anuncia total) y el plugin en `web/src/lib/updater.ts` (imports dinámicos, la
oferta recordada y liberada, el rechazo explícito de instalar sin oferta). El chequeo corre una vez al abrir la
ventana, desde el shell y no desde una pantalla, porque el punto que puede encender pertenece al índice.

LOS NÚMEROS: `bun run typecheck` 0/0 y `svelte-check` 0 warnings; `bun run test` **473 en 27 archivos**;
`cargo test -p mirk-desktop` **129**; `cargo fmt --all --check` **0** y clippy del workspace **limpio** con
`-D warnings`. El arnés de captura sale **0** con "no exceptions or console errors" y estrena la pantalla
`settings-app`: el fixture contesta `null` a `plugin:updater|check` y la captura muestra "Mirk is up to date"
con su control. **El mock del arnés no es cosmético**: sin él el chequeo de arranque cae en "unmocked command" y
el job `capture` se pone rojo por una razón falsa.

EL COSTO, dicho: el updater arrastra `reqwest` + `rustls` + `zip`/`tar` (~40 crates), y el job `compile` de
Windows y macOS compila eso en cada PR. No está medido; si T6 muestra que se va de tiempo, la salida es
`native-tls` por plataforma, no apagar el job.

RESIDUALES. (1) Falta la clave pública: `plugins.updater` (pubkey + endpoint), `bundle.createUpdaterArtifacts:
true` y `uploadUpdaterJson: true` en el paso firmado se escriben cuando exista, junto con el secreto
`TAURI_SIGNING_PRIVATE_KEY` y su respaldo —el lector la genera con
`bun tauri signer generate -w ~/.tauri/mirk.key`—. Hasta entonces el panel dice "could not check", que es la
verdad, y no un "está al día" inventado. (2) Queda por resolver qué hace el paso **sin** clave cuando
`createUpdaterArtifacts` sea `true`: el CLI de Tauri se niega a construir artefactos de updater sin clave, así
que esa rama —que existe para forks y para un secreto borrado— va a necesitar
`--config '{"bundle":{"createUpdaterArtifacts":false}}'` o desaparecer. (3) **T8 no figura en este archivo**: el
lector lo dio por cerrado y no hay fila en la tabla ni entrada en el log; agregué T7, que sí está documentado en
L6, y no invento el contenido de T8.

**L8 — T5 cerrada: la clave llegó, y lo que hay que contestar sobre la privada.**

LA CLAVE PÚBLICA, VALIDADA ANTES DE ESCRIBIRLA. El lector la pasó en base64 y se decodificó antes de tocar el
archivo: dos líneas, comentario `minisign public key: B20AAF6DF81CC50F` y una segunda línea de 56 caracteres que
decodifica a **42 bytes** — dos de algoritmo (`Ed`, Ed25519), ocho de id y treinta y dos de clave. El id, leído
en little-endian (`0FC51CF86DAF0AB2`), coincide con el hexadecimal del comentario al revés, así que las dos
codificaciones de la misma clave se dan la razón. Ese es el valor que quedó en `plugins.updater.pubkey`.

QUÉ SE ESCRIBIÓ. `tauri.conf.json` gana `plugins.updater` (pubkey + endpoint
`https://github.com/TomasCuevas/mirk/releases/latest/download/latest.json`) y `bundle.createUpdaterArtifacts:
true`. `release.yml`: `uploadUpdaterJson: true` en el paso firmado, y el paso sin clave ahora escribe
`{"bundle":{"createUpdaterArtifacts":false}}` a un archivo y lo pasa con `--config` — **archivo y no JSON
inline**, porque `args` se concatena a la línea de comandos del CLI y un blob con comillas tendría que sobrevivir
al quoting de un shell que este workflow no controla. `CONTRIBUTING.md` gana la sección "Releases and the update
key", que es donde vive lo que un colaborador necesita y donde la pregunta sobre la clave privada queda
contestarada de una vez para todos.

EL RESIDUAL DE L7(2) SE MIDIÓ, y no como se suponía. `bun run tauri build --bundles deb` sobre esta máquina:
empaqueta el `.deb` (10,35 MiB) y **después** sale 1 con "A public key has been found, but no private key. Make
sure to set `TAURI_SIGNING_PRIVATE_KEY`". O sea que el CLI **no** advierte y sigue: falla, y falla al final, con el
paquete ya escrito. El `--config` del paso sin clave no era una precaución sino un requisito, y se verificó:
con `--config` apuntando a `{"bundle":{"createUpdaterArtifacts":false}}` el mismo build sale **0** con el mismo
`.deb`. Las dos ramas del workflow quedan medidas, cada una con su código de salida.

EL DEFECTO QUE INTRODUJE Y EL COMPILADOR ATRAPÓ, que es la razón por la que este párrafo existe. El primer intento
de ese mismo build falló **antes** de empacar, en `tauri::generate_context!()`: *"cannot find `serde_json` in the
crate root"*. La causa no era el updater sino el bloque `plugins` que acabo de agregar al config: el macro expande
código que deserializa la configuración de plugins con `serde_json`, así que en cuanto la app declara uno, la crate
lo necesita en **su** raíz. `serde_json` estaba sólo en `[dev-dependencies]`, y eso explica por qué nada lo había
visto: `cargo test` y `cargo check --all-targets` ven las dev-dependencies, y el `cargo check` de CI —que la matriz
`compile` corre sin `--all-targets` en Windows y macOS— habría sido el primero en ponerse rojo, no esta máquina.
Promovido a dependencia normal (misma versión del workspace, ningún crate nuevo en el grafo) y `cargo check -p
mirk-desktop` en verde. Lección para el registro: **un chequeo con `--all-targets` no prueba que el binario
compile**, y el camino de release es el único que ejercita esa diferencia.

LO ÚNICO QUE NO SE PUEDE VERIFICAR SIN EMPUJAR: que GitHub Actions resuelva el secreto y que `latest.json` quede
en el draft con sus `.sig`. Eso es T6, junto con la corrida verde del CI y el primer tag.

**L8b — la contraseña de la clave, y el chequeo que el CLI ya trae.**

El lector generó la clave **con** password, así que `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` dejó de ser opcional y
los tres textos que decían lo contrario se corrigieron: el comentario del paso firmado en `release.yml`, la
sección de `CONTRIBUTING.md` (que ahora explica que `gh secret set NOMBRE` sin valor es lo que mantiene la
contraseña fuera del historial, y que el build local necesita las dos variables) y el snippet de build local.

Y MEDIR EN VEZ DE AVERIGUAR DEJÓ UN CHEQUEO GRATIS. Con una clave descartable generada en `/tmp` —borrada después—
un `bun run tauri build --bundles deb` firmado mostró dos cosas: el artefacto esperado, `Mirk_0.0.1_amd64.deb.sig`
(428 bytes) junto al `.deb`; y que **el CLI compara por su cuenta la clave privada con `plugins.updater.pubkey`**
y avisa `The updater secret key … does not match the public key … won't be accepted at runtime`. O sea que el
proyecto tiene un gate de par-de-claves antes de empujar ningún tag: build local con las dos variables, y si el
aviso aparece, el par no es el de este repositorio. Vale más que cualquier instrucción en un documento, porque
falla solo.

Contexto del error real: el lector cargó la clave privada como valor del secreto de la contraseña. Un secreto de
GitHub no se puede leer de vuelta, sólo sobrescribir; el valor equivocado no expone nada nuevo (la clave ya vive
en el otro secreto, cifrada) pero hay que reemplazarlo. Sin la contraseña correcta el release falla al descifrar,
que es el modo de fallo que este workflow quiere.

**L8c — la clave real firma, y los secretos están cargados.**

Ejecutado sobre la máquina con la clave de mirk (no una descartable): `bun run tauri build --bundles deb` con las
dos variables tomadas de los archivos → **exit 0**, `Mirk_0.0.1_amd64.deb.sig` de 428 bytes junto al `.deb`, y
**cero** apariciones del aviso de par-de-claves. O sea que la contraseña descifra la clave y que el par corresponde
al `plugins.updater.pubkey` del repo, que es exactamente lo que el primer release va a exigir.

Secretos cargados con `gh secret set` leyendo desde los archivos, así el valor nunca pasó por la conversación ni
por el historial del shell: `TAURI_SIGNING_PRIVATE_KEY` y `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, confirmados con
`gh secret list`.

LO QUE ESTO DEJA EN LA MÁQUINA, y que el respaldo tiene que incluir: `~/.tauri/mirk.key` (cifrada),
`~/.tauri/mirk.key.pub` y `~/.tauri/mirk.key.password` (modo 600). La contraseña **no** está dentro de la clave, así
que una copia de la clave sin su contraseña no sirve para firmar nada — guardar las dos juntas o no guardar nada.

Nota de método, porque costó tres intentos y ninguno fue por la criptografía: el shell del lector es **fish**, no
bash. `read -rsp` no existe ahí (la variable quedaba vacía y el CLI respondía "Wrong password for that key", un
mensaje que culpa a la contraseña cuando el problema era la sintaxis), `export`/`unset`/`$(…)` tampoco. La forma
que funcionó: `read -s -P 'prompt: ' VAR` y `printf '%s'` para escribir sin salto de línea final — un `\n` en el
archivo de la contraseña habría invalidado el secreto.

**L9 — T6, primera mitad: el árbol repartido en commits, y dos números míos que estaban mal.**

EL MÉTODO. Antes de tocar nada: `git add -A`, un commit temporal con todo —incluidos los archivos nuevos— y la
etiqueta **`wip-before-t6-slices`**; después `git reset --soft HEAD~1 && git reset`, que devuelve el árbol a su
estado sin commitear con la etiqueta intacta como red. Es la misma técnica que el registro ya tenía para
rebanadas anteriores. La comprobación de que nada se perdió no es mirar el diff contra la etiqueta (los archivos
nuevos aparecen como "borrados", porque en el árbol no están versionados): es contar que siguen en disco y que al
final `git status` queda vacío con el árbol igual al de la etiqueta.

DOS CORRECCIONES DE MIS PROPIOS NÚMEROS, porque el registro sirve justamente para esto:

1. **Dije "9 commits" y son 8** (siete de trabajo más el de versión). Partir los dos grandes en dos cada uno da
   siete, no nueve: conté de más en la vista previa que le mostré al lector. Se lo dije antes de empezar.
2. **El formato no medía 2.701 líneas, medía 1.071.** El número de L6 era el diff del árbol *entero* en ese
   momento, no el componente de formato: la diferencia era código nuevo sin formatear viajando con él. Medido
   ahora aplicando rustfmt a los archivos tal como están en `HEAD`: **19 archivos, 1.071 inserciones, 472
   borrados**. Corolario que no esperaba: de los 51 archivos modificados, **20 eran sólo formato** y desaparecieron
   del diff una vez commiteado el estilo.

UNA TRAMPA DE GIT QUE ME HIZO CREER QUE HABÍA PERDIDO TRABAJO. `git checkout <etiqueta> -- <archivos>` escribe el
contenido en el árbol **y en el índice**; `git diff` no muestra lo preparado, así que durante un momento pareció
que los cambios funcionales de Rust se habían esfumado. No se había perdido nada: estaba preparado. La forma sin
efectos laterales es `git restore --source=<etiqueta> --worktree`, que sólo toca el árbol.

LA VERIFICACIÓN DE CADA COMMIT, que es lo que hace que valga la pena. Compilar el árbol no prueba nada sobre un
commit: cargo y vitest leen el árbol de trabajo, no lo preparado. Lo que sí prueba es
`git stash push -u --keep-index`: deja en el árbol exactamente el contenido preparado y esconde el resto (con `-u`
también los archivos nuevos, que si no sobreviven y rompen las pruebas por razones falsas — pasó con
`update.test.ts` y sus claves todavía sin commitear). Resultado por commit: `a4872f2` y `6f76e47` y `3e8e99c`
compilan (`cargo check` 0); `aab8ecb` da **440 pruebas en 25 archivos** y typecheck 0/0; `c5b3c0e` da **473 en 27**
y typecheck 0/0. Cada uno verificado con el contenido exacto que quedó commiteado, no con el árbol final.

TRES ARCHIVOS TUVIERON QUE PARTIRSE A MANO porque dos features tocaban las mismas líneas: `App.svelte`,
`i18n.ts`/`i18n.test.ts` y `scripts/capture-ui.mjs` (más `main.rs` y `tauri.conf.json`, que tienen tres dueños
distintos: pdfium, historial y updater). El criterio fue siempre el mismo: el commit anterior se queda con la
versión sin la otra feature, y la otra entra en su propio commit.

TAMAÑO DE LA RAMA, dicho sin adorno: **53.583 líneas autoradas en 146 commits** desde `main`. El lector eligió
merge directo a `main` y etiqueta, así que la entrega no pasa por un PR; queda anotado porque es el número que un
revisor vería de una vez.

**L10 — el primer CI de la rama encontró el defecto que el job existía para encontrar.**

La corrida real sobre `main` (run **41507917349**, 3m 13s): `rust`, `web` y `capture` en verde; `compile
(macos-latest)` y `compile (windows-latest)` en rojo, los dos con la misma línea:

```
glob pattern ../../vendor/pdfium/lib/* path not found or didn't match any files.
```

La causa no era el updater ni el código por plataforma: `tauri-build` valida `bundle.resources` en el **build
script**, antes de compilar nada, y un glob sin coincidencias es un error. El razonamiento con el que se armó el
job —"`cargo check` nunca necesita pdfium"— es verdad sobre la librería, que se abre en runtime, y falso sobre el
manifiesto que la empaqueta. Este era exactamente el residual que L5 dejó anotado como *razonado y no ejecutado*:
rompió en la primera corrida, que es para lo que el job existe.

El arreglo es un archivo marcador que el job crea en el runner (`vendor/pdfium/lib/placeholder`, gitignored): hace
que el glob coincida sin pagar 8 MB por runner. Medido localmente en las dos direcciones, moviendo `libpdfium.so`
fuera del camino y forzando el build script con un `touch`: sin nada en el directorio aparece el error; con el
marcador, `cargo check -p mirk-desktop` sale **0**. Y un detalle que casi se pasa: el nombre **no** puede empezar
con punto — el crate `glob` no matchea dotfiles con `*`, así que un `.keep` no habría servido.

#!/usr/bin/env bun
// Refuses to produce a package that would count zero pages in silence.
//
// `mirk-scan` counts pages with the pdfium native library, and an installed application has neither
// `vendor/pdfium` nor `$MIRK_PDFIUM_LIB`: its only copy is the one `bundle.resources` copies into the
// package (S5 of `odd/tasks/ci-and-release.md`). The library is deliberately not committed, so a
// clone that never ran `scripts/fetch-pdfium.sh` still builds and bundles happily — and the result
// installs, launches, and cannot count a single page.
//
// This script runs from `bundle.beforeBuildCommand`, before the frontend or Rust build starts, and
// turns that silent outcome into a named failure. It checks two things, because either alone can be
// wrong: that the library is on disk, and that `tauri.conf.json` still ships it as a resource.
//
// It is plain Node APIs run by bun, so it needs no shell, no bash and no platform-specific quoting:
// `tauri build` invokes it on Linux, macOS and Windows alike.

import { existsSync, readdirSync, statSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
/** The repository root: this file lives at `<root>/crates/mirk-desktop/scripts/`. */
const root = resolve(here, "../../..");
const vendorLibrary = join(root, "vendor/pdfium/lib");
const configPath = join(root, "crates/mirk-desktop/tauri.conf.json");

/** The subdirectory of the bundle's resources that carries the library, mirroring `main.rs`. */
const RESOURCE_DIRECTORY = "pdfium";

/** The library file name `pdfium-render` will look for inside that directory. */
function libraryName() {
  switch (process.platform) {
    case "linux":
      return "libpdfium.so";
    case "darwin":
      return "libpdfium.dylib";
    case "win32":
      return "pdfium.dll";
    default:
      return null;
  }
}

function fail(problem, remedy) {
  console.error("");
  console.error("require-pdfium-resource: refusing to build a package without pdfium.");
  console.error("");
  console.error(`  ${problem}`);
  console.error("");
  console.error("  Why this is fatal: the packaged application has no `vendor/pdfium` and no");
  console.error("  `$MIRK_PDFIUM_LIB`, so the bundled copy is the only one it can load. Without it,");
  console.error("  the installed app cannot count the pages of a single document.");
  console.error("");
  console.error(`  Remedy: ${remedy}`);
  console.error("");
  process.exit(1);
}

const name = libraryName();
if (name === null) {
  fail(
    `no prebuilt pdfium library name is known for platform "${process.platform}".`,
    "see https://github.com/bblanchon/pdfium-binaries/releases and extend this script.",
  );
}

const library = join(vendorLibrary, name);
if (!existsSync(library) || statSync(library).size === 0) {
  const present = existsSync(vendorLibrary)
    ? readdirSync(vendorLibrary).join(", ") || "nothing"
    : "the directory does not exist";
  fail(
    `${library} is missing or empty (vendor/pdfium/lib contains: ${present}).`,
    "run scripts/fetch-pdfium.sh from the repository root, then build again.",
  );
}

// The library exists, but a config edit that dropped the resource would still produce a package
// without it. Both facts are checked here so neither can drift alone.
const config = JSON.parse(await readFile(configPath, "utf8"));
const bundle = config.bundle ?? {};
if (bundle.active !== true) {
  fail(
    "crates/mirk-desktop/tauri.conf.json has bundle.active off, so nothing is packaged at all.",
    "set bundle.active to true.",
  );
}

const resources = bundle.resources ?? {};
const entry = Object.entries(resources).find(([source]) =>
  source.replace(/\\/g, "/").includes("vendor/pdfium/lib"),
);
if (entry === undefined) {
  fail(
    "crates/mirk-desktop/tauri.conf.json declares no bundle.resources entry for vendor/pdfium/lib.",
    `add "resources": { "../../vendor/pdfium/lib/*": "${RESOURCE_DIRECTORY}/" } to bundle.`,
  );
}

const [source, target] = entry;
const expectedTarget = `${RESOURCE_DIRECTORY}/`;
if (target !== expectedTarget && target !== RESOURCE_DIRECTORY) {
  fail(
    `bundle.resources copies ${source} to "${target}", but the application looks in "${expectedTarget}".`,
    `change the resource target to "${expectedTarget}" (or change PDFIUM_RESOURCE_DIRECTORY in crates/mirk-desktop/src/main.rs to match).`,
  );
}

console.log(`require-pdfium-resource: ${name} will ship as the bundle resource ${target}`);

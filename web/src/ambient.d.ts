// TypeScript needs to know what a `.svelte` import is. Svelte's own tooling type-checks the
// components; `tsc --noEmit` only sees this entry file and the plain `.ts` modules, so a component
// arrives here as an opaque value.
declare module "*.svelte" {
  const component: unknown;
  export default component;
}

// A side-effect CSS import (`import "./styles/tokens.css"`) is a module to Vite but an opaque
// specifier to `tsc`, which refuses it without a declaration. The file has no exports to describe:
// importing it is the whole effect.
declare module "*.css";

// Vite's `?url` suffix. The debug-only fixture page (`web/reader-fixture.html`) imports
// `pdfjs-dist/build/pdf.worker.min.mjs?url` to get the URL of the worker asset Vite emits, so pdf.js
// starts a real module worker instead of a fake one on the interface thread. To `tsc` and
// `svelte-check` that specifier is a module whose default export is the asset's URL.
declare module "*?url" {
  const url: string;
  export default url;
}

// The one `import.meta.env` flag the web code reads, and the only one it may read. Vite replaces it with
// a literal at build time. `web/src/reader-fixture-entry.ts` uses it to refuse to mount the debug-only
// fixture proof in anything but a dev build — the page is not a build input either, so this is the
// second lock, not the only one. Declaring just `DEV` keeps the web code from reading configuration the
// repository never defined.
interface ImportMetaEnv {
  readonly DEV: boolean;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}

/** The frontend's build stamp, injected by Vite (`define` in `web/vite.config.ts`). */
declare const __BUILD_STAMP__: string;


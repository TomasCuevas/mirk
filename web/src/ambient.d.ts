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

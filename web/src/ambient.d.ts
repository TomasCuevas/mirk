// TypeScript needs to know what a `.svelte` import is. Svelte's own tooling type-checks the
// components; `tsc --noEmit` only sees this entry file and the plain `.ts` modules, so a component
// arrives here as an opaque value.
declare module "*.svelte" {
  const component: unknown;
  export default component;
}

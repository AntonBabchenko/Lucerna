// Vite's `?raw` suffix inlines a file's text as a default string export.
// Declared explicitly because this project does not pull in `vite/client`
// ambient types in `src/`. This wildcard covers any `*.md?raw` import in the
// project, not just CHANGELOG.md — add no second declaration elsewhere.
declare module '*.md?raw' {
  const content: string;
  export default content;
}

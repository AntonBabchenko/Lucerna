// Vite's `?raw` suffix inlines a file's text as a default string export.
// Declared explicitly because this project does not pull in `vite/client`
// ambient types in `src/`.
declare module '*.md?raw' {
  const content: string;
  export default content;
}

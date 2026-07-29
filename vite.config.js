import { defineConfig } from "vite";
import { sveltekit } from "@sveltejs/kit/vite";

const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [sveltekit()],

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`, and `.claude/` — agent
      // worktrees live at `.claude/worktrees/<name>/`, i.e. INSIDE this
      // project root, and each is a full checkout carrying its own generated
      // `.svelte-kit/tsconfig.json`. Without this, a `svelte-kit sync` in any
      // worktree makes THIS dev server log "changed tsconfig file detected",
      // clear its cache and force a full reload — mid-request — which leaves
      // the SSR module graph half-initialised and every page then fails with
      // `(0 , __vite_ssr_import_N__.respond) is not a function`.
      ignored: ["**/src-tauri/**", "**/.claude/**"],
    },
  },
}));

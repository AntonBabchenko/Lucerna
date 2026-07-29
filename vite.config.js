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
      // 3. Ignore `src-tauri`, plus two paths that would otherwise make this
      //    dev server invalidate its whole module graph mid-request:
      //    - `.claude/` — agent worktrees live at `.claude/worktrees/<name>/`,
      //      i.e. INSIDE this project root, each a full checkout with its own
      //      generated `.svelte-kit/tsconfig.json`.
      //    - our own `.svelte-kit/tsconfig.json`, rewritten by every
      //      `svelte-kit sync` (i.e. by `pnpm typecheck` / `pnpm check`).
      //    Either write makes Vite log "changed tsconfig file detected", call
      //    `moduleGraph.invalidateAll()` and force a full reload. Landing that
      //    mid-request exposes an upstream Vite bug: its module-runner cycle
      //    check reads `mod.importers`, which `invalidateModule` never clears,
      //    and SvelteKit's runtime has a genuine `respond.js` <-> `fetch.js`
      //    cycle — so `server/index.js` re-evaluates against respond.js's
      //    empty placeholder namespace, Vite's export getter swallows the TDZ
      //    error, and every subsequent request dies with
      //    `(0 , __vite_ssr_import_N__.respond) is not a function` until the
      //    server restarts. Upstream: vitejs/vite#22293, fixed by #22369 in
      //    vite 8.0.12 — NOT backported to 6.x/7.x, so this stays a
      //    trigger-side mitigation until we move to vite 8 (needs
      //    @sveltejs/vite-plugin-svelte 7, whose peer range allows it).
      ignored: [
        "**/src-tauri/**",
        "**/.claude/**",
        "**/.svelte-kit/tsconfig.json",
      ],
    },
  },
}));

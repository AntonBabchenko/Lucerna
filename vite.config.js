import { defineConfig } from "vite";
import { sveltekit } from "@sveltejs/kit/vite";
import { createRootDirIgnore } from "./tools/dev-watch-ignore.mjs";

const host = process.env.TAURI_DEV_HOST;

// The directory this file lives in — the root the dev server watches. It is
// whichever checkout the server was started from: the main one, or an agent
// worktree nested inside it. `import.meta.dirname` is the spelling that
// survives all three loaders of this file: Vite's config bundler injects the
// original file's value, Vitest's module runner sets it on the `import.meta`
// it hands to transformed modules, and Node has it natively.
// `new URL(".", import.meta.url)` does not — Vite rewrites that pattern into
// an asset URL in anything Vitest transforms.
const projectRoot = import.meta.dirname;

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
      // 3. Ignore `src-tauri`, plus the paths that would otherwise make this
      //    dev server invalidate its whole module graph mid-request:
      //    - `.claude/` and `.claude-worktrees/` — agent worktrees live at
      //      `.claude/worktrees/<name>/` and `.claude-worktrees/<name>/`, i.e.
      //      INSIDE this project root, each a full checkout with its own
      //      generated `.svelte-kit/tsconfig.json`.
      //    - `.cargo-target*/` — per-worktree cargo target dirs at the root;
      //      hundreds of thousands of build artifacts that nothing in the
      //      frontend graph depends on, but which the watcher would crawl.
      //    - our own `.svelte-kit/tsconfig.json`, rewritten by every
      //      `svelte-kit sync` (i.e. by `pnpm typecheck` / `pnpm check`).
      //
      //    The first two are ROOT-RELATIVE ON PURPOSE: `createRootDirIgnore`
      //    matches them only as direct children of `projectRoot`. Do not turn
      //    them back into `**/` globs. The watcher tests ABSOLUTE paths, and
      //    this config also runs from inside those very worktrees, where every
      //    path contains `/.claude/`: a `**/.claude/**` glob then matches the
      //    watched root itself, the crawl is pruned at the top, and hot reload
      //    is off without a word — no `hmr update` lines, stale transforms
      //    served until a restart. Anchored to the root, one rule does both
      //    jobs: the main checkout still skips the worktrees nested in it, and
      //    a worktree watches its own `src/` while skipping whatever is nested
      //    in IT. Rationale and tests: tools/dev-watch-ignore.mjs,
      //    tests/dev-watch-ignore.test.ts.
      //
      //    The two globs that remain are fine unanchored: no checkout lives
      //    under a directory named `src-tauri` or `.svelte-kit`. The tsconfig
      //    one must keep matching a worktree's OWN `.svelte-kit/tsconfig.json`
      //    when the server runs there — unanchored, it does.
      //
      //    Why it matters: a tsconfig write the watcher sees (a nested
      //    worktree's or our own) makes Vite log "changed tsconfig file
      //    detected", call `moduleGraph.invalidateAll()` and force a full
      //    reload. Landing that mid-request exposes an upstream Vite bug: its
      //    module-runner cycle check reads `mod.importers`, which
      //    `invalidateModule` never clears, and SvelteKit's runtime has a
      //    genuine `respond.js` <-> `fetch.js` cycle — so `server/index.js`
      //    re-evaluates against respond.js's empty placeholder namespace,
      //    Vite's export getter swallows the TDZ error, and every subsequent
      //    request dies with
      //    `(0 , __vite_ssr_import_N__.respond) is not a function` until the
      //    server restarts. Upstream: vitejs/vite#22293, fixed by #22369 in
      //    vite 8.0.12 — NOT backported to 6.x/7.x, so this stays a
      //    trigger-side mitigation until we move to vite 8 (needs
      //    @sveltejs/vite-plugin-svelte 7, whose peer range allows it).
      ignored: [
        "**/src-tauri/**",
        "**/.svelte-kit/tsconfig.json",
        createRootDirIgnore(projectRoot),
      ],
    },
  },
}));

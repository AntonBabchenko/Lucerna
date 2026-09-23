#!/usr/bin/env node
// Fails CI if the OS-opener plugin is referenced outside the enumerated
// call sites below. `@tauri-apps/plugin-opener` spawns an OS process — the
// user's default browser (openUrl) or file manager (revealItemInDir) —
// outside the Rust process:: chokepoint, so the set of places that can
// trigger it must stay enumerable (see docs/PRINCIPLES.md hard rule 3 and
// the opener rows in Appendix A). This is the frontend counterpart of the
// backend allowlist in src-tauri/tests/structural_no_raw_spawn.rs.
//
// Scope: the SvelteKit source (`src/`). The plugin is an npm module, so
// nothing under `static/` or the HTML shell can reach it — there is no
// import machinery there to resolve the specifier.
//
// Threat model: this is a guardrail against unreviewed growth of the opener
// surface, not a sandbox. A determined developer can bypass it with a
// dynamic specifier like `import('@tauri-apps/' + 'plugin-opener')`. The
// point is to make the principle hard to violate by mistake — anything that
// looks suspicious enough to write that way will not survive review.

import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { extname, join, resolve } from 'node:path';

const ROOTS = [resolve('src')].filter((p) => existsSync(p));

// Every file that may reference the opener plugin. Adding a NEW call site
// means: confirm the spawn is covered by the opener rows in
// docs/PRINCIPLES.md Appendix A (default browser / file manager), then add
// the file here. For a URL there is no new call site: every open goes
// through `openExternalHttps` in src/lib/ui/safe-open.ts.
const ALLOWED = new Set(
  [
    // Reveals the just-created desktop shortcut (revealItemInDir).
    'src/lib/instances/CreateShortcutDialog.svelte',
    // The ONE frontend chokepoint for openUrl: https-only, and it reports a
    // refused scheme or a failed open.
    'src/lib/ui/safe-open.ts',
  ].map((p) => resolve(p)),
);

// The import specifier is what the scan keys on: the dynamic
// `import('@tauri-apps/plugin-opener')` form both allowed files use and a
// static `import ... from '@tauri-apps/plugin-opener'` both contain it
// verbatim.
const PLUGIN_RE = /@tauri-apps\/plugin-opener/;

// `openPath` is forbidden outright, allowlisted file or not: folder opening
// goes through a Rust command (which validates the path and is itself
// allowlisted in structural_no_raw_spawn.rs), never through the frontend
// opener.
const OPEN_PATH_RE = /\bopenPath\s*\(/;

const EXTS = new Set(['.ts', '.tsx', '.js', '.svelte', '.mjs', '.cjs', '.html']);

function walk(dir) {
  const out = [];
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    const s = statSync(full);
    if (s.isDirectory()) {
      out.push(...walk(full));
    } else if (EXTS.has(extname(entry))) {
      out.push(full);
    }
  }
  return out;
}

let violations = 0;
const files = ROOTS.flatMap((root) => walk(root));
for (const file of files) {
  const content = readFileSync(file, 'utf8');
  const lines = content.split('\n');
  const allowed = ALLOWED.has(file);
  for (let i = 0; i < lines.length; i++) {
    if (!allowed && PLUGIN_RE.test(lines[i])) {
      console.error(`${file}:${i + 1}: opener plugin referenced outside the allowlist`);
      violations++;
    }
    if (OPEN_PATH_RE.test(lines[i])) {
      console.error(
        `${file}:${i + 1}: forbidden frontend "openPath" (use a Rust folder-open command)`,
      );
      violations++;
    }
  }
}

if (violations > 0) {
  console.error(
    `\n${violations} violation(s). Opener spawns must stay enumerable (see docs/PRINCIPLES.md Appendix A); a deliberate new call site is added to ALLOWED in tools/check-opener-calls.mjs.`,
  );
  process.exit(1);
}
console.log('All opener plugin references in src/ are on the allowlist.');

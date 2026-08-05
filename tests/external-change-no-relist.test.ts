/**
 * The always-mounted `ModsReconciled` handler must never re-request the mod list.
 *
 * `mods_list_installed` is the ONLY thing that emits this event, so a handler
 * that lists again hands itself its own trigger. The marker is cleared when it
 * is taken, so a single extra listing terminates — but the scenario this whole
 * feature exists for is a pack's downloader mod writing dozens of jars over
 * minutes, and there each re-listing finds a FURTHER change, sets the marker
 * again, and emits again. That is a self-feeding cycle for as long as the
 * external writer keeps going, re-walking a 356-entry `mods/` every 150ms.
 *
 * It shipped that way in #356: the handler called `stats.refreshInstalledStats`,
 * which calls `commands.modsListInstalled` one level down — while the comment
 * directly above it claimed the opposite.
 *
 * `InstalledModsView`'s handler is pinned by a rendering test
 * (`installed-external-mod-change.test.ts`). `+page.svelte` is the whole app
 * shell and is not renderable under vitest, so this reads the source instead —
 * the same guard-by-source-scan shape as `tools/check-no-network-calls.mjs` and
 * the Rust `structural_*` tests.
 */
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

const PAGE = resolve('src/routes/+page.svelte');

/** Every command that returns the installed-mod list, i.e. that can emit. */
const LISTING_CALLS = [
  'modsListInstalled',
  // Composable wrappers that call it one level down. `refreshInstalledStats`
  // is the one that actually shipped the defect.
  'refreshInstalledStats',
];

/**
 * The body of the debounced handler registered on `events.modsReconciled`,
 * from its `const` line to the `}, <ms>);` that closes `debounceTrailing`.
 */
function externalChangeHandlerBody(src: string): string {
  const start = src.indexOf('const debouncedExternalChangeStats');
  expect(start, 'the ModsReconciled handler must still be named this').toBeGreaterThan(-1);
  const end = src.indexOf('}, 150);', start);
  expect(end, 'handler must still be a debounceTrailing(…, 150)').toBeGreaterThan(start);
  return src.slice(start, end);
}

describe('the always-mounted external-change handler', () => {
  it('does not re-request the mod list, directly or through a composable', () => {
    const body = externalChangeHandlerBody(readFileSync(PAGE, 'utf8'));
    for (const call of LISTING_CALLS) {
      expect(body, `${call} lists the mods, which is what emits this event`).not.toContain(call);
    }
  });

  it('still refreshes the compatibility scan — the point of the handler', () => {
    const body = externalChangeHandlerBody(readFileSync(PAGE, 'utf8'));
    expect(body).toContain('refreshIncompatible');
    // Unforced, the shared store dedupes on (instance, mc, loader) — none of
    // which an external write changes — and the rescan silently does nothing.
    expect(body).toContain('force: true');
  });
});

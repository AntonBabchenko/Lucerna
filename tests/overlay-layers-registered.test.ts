import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

// Every surface that floats over the app must sit in the one layer stack
// (src/lib/ui/layer-stack.svelte.ts): an overlay outside it is painted under a
// running contextual tour's dim and shares its Escape with whatever is below.
// Markers of an overlay surface: the popover / modal tier tokens and the
// modal backdrop.
const MARKERS = [/z-\[var\(--z-popover\)\]/, /z-\[var\(--z-modal\)\]/, /fixed inset-0 z-50/];
const REGISTERS = /from ['"](?:\$lib\/ui|\.)\/layer-stack\.svelte['"]/;

// Exact paths, each with the reason it needs no registration of its own.
const EXEMPT = new Map<string, string>([
  [
    join('src', 'lib', 'tasks', 'OperationsBar.svelte'),
    'names --z-popover only in a comment; the surface is OperationsPanel.svelte',
  ],
]);

function svelteFiles(dir: string, acc: string[] = []): string[] {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) svelteFiles(full, acc);
    else if (entry.name.endsWith('.svelte')) acc.push(full);
  }
  return acc;
}

describe('overlay surfaces register in the layer stack', () => {
  it('every file that renders a popover / modal surface imports layer-stack', () => {
    const offenders = svelteFiles('src')
      .filter((f) => !EXEMPT.has(f))
      .filter((f) => {
        const src = readFileSync(f, 'utf8');
        return MARKERS.some((m) => m.test(src)) && !REGISTERS.test(src);
      });
    expect(offenders).toEqual([]);
  });

  it('every exemption still names a file that has a marker', () => {
    for (const file of EXEMPT.keys()) {
      const src = readFileSync(file, 'utf8');
      expect(
        MARKERS.some((m) => m.test(src)),
        file,
      ).toBe(true);
    }
  });
});

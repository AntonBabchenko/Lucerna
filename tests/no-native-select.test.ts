import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

function svelteFiles(dir: string, acc: string[] = []): string[] {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) svelteFiles(full, acc);
    else if (entry.name.endsWith('.svelte')) acc.push(full);
  }
  return acc;
}

describe('no native <select> in src', () => {
  it('every dropdown uses the Select component, not a native <select>', () => {
    const offenders = svelteFiles('src')
      // src/lib/ui/Select.svelte is the custom replacement; its doc-comment
      // legitimately mentions the native `<select>` element it supersedes,
      // which would otherwise trip the element-tag regex below.
      .filter((f) => !f.endsWith('Select.svelte'))
      .filter((f) => /<select[\s>]/.test(readFileSync(f, 'utf8')));
    expect(offenders).toEqual([]);
  });
});

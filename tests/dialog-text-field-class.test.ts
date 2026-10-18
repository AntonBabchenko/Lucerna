/**
 * No `.input` rule exists in the app's CSS, so a field given `class="input …"` keeps Tailwind
 * preflight's zero border and no padding: it reads as a line of text, not as something to edit
 * (four dialogs did, found by the v0.28 regression pass). Text fields spell the modal input recipe
 * (`border rounded px-2 py-1`, DESIGN.md) inline. This guard keeps the undefined class from coming
 * back unless someone defines it.
 */
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

function svelteFiles(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return svelteFiles(path);
    return path.endsWith('.svelte') ? [path] : [];
  });
}

const css = readFileSync(resolve('src/app.css'), 'utf8');
const inputRuleDefined = /(^|[\s,}])\.input(?![-\w])/m.test(css);

describe('text field classes', () => {
  it('no component uses the undefined `input` class', () => {
    const offenders: string[] = [];
    for (const file of svelteFiles(resolve('src'))) {
      const src = readFileSync(file, 'utf8');
      for (const m of src.matchAll(/class="([^"]*)"/g)) {
        if (m[1].split(/\s+/).includes('input')) offenders.push(file.replace(resolve('.'), '').replaceAll('\\', '/'));
      }
    }
    expect(inputRuleDefined || offenders.length === 0, `undefined .input class in: ${offenders.join(', ')}`).toBe(
      true,
    );
  });
});

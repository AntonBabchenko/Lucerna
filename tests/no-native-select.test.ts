import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

// The one file allowed to mention a native <select>: the custom replacement
// itself, whose doc-comment legitimately names the element it supersedes.
// Exact relative path, built with join() so it compares equal to the
// join()-built paths svelteFiles returns on every OS (same construction as
// the bindings path in tests/ai-translation-setting.test.ts). The previous
// endsWith('Select.svelte') would have silently exempted ANY future
// *Select.svelte wrapper from this scan entirely.
const SELECT_COMPONENT = join('src', 'lib', 'ui', 'Select.svelte');

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
      .filter((f) => f !== SELECT_COMPONENT)
      .filter((f) => /<select[\s>]/.test(readFileSync(f, 'utf8')));
    expect(offenders).toEqual([]);
  });
});

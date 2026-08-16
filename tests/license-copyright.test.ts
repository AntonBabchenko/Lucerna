import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, test } from 'vitest';

describe('copyright and license notices', () => {
  test('README License section carries the GPL notice with the copyright line', () => {
    const readme = readFileSync(resolve('README.md'), 'utf8');

    expect(readme).toContain('Copyright (C) 2026 Anton Babchenko');
    expect(readme).toContain('either version 3 of the License, or');
  });

  test('bundle metadata carries the copyright line', () => {
    const conf = JSON.parse(readFileSync(resolve('src-tauri/tauri.conf.json'), 'utf8'));

    expect(conf.bundle.copyright).toBe('Copyright (C) 2026 Anton Babchenko');
  });
});

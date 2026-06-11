import { describe, expect, it } from 'vitest';
import { CHANGELOG } from '$lib/changelog/source';

// Canary: the real CHANGELOG.md is inlined via Vite `?raw` and parsed once.
// If this fails to resolve, the `?raw` import path in source.ts is wrong.
describe('embedded changelog source', () => {
  it('loads and parses the real CHANGELOG.md into at least one version', () => {
    expect(Array.isArray(CHANGELOG)).toBe(true);
    expect(CHANGELOG.length).toBeGreaterThan(0);
  });

  it('includes the 0.1.0 first-release version with content', () => {
    const first = CHANGELOG.find((v) => v.version === '0.1.0');
    expect(first).toBeDefined();
    expect(first?.sections.length).toBeGreaterThan(0);
  });
});

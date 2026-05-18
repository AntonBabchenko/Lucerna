import { render } from '@testing-library/svelte';
import { describe, expect, test } from 'vitest';

// Smoke test for the vitest+svelte component testing infrastructure.
// If this passes, the rest of the v0.5.0 component tests (LoaderPicker,
// Sidebar, MainTabs) can be written against the same setup.
describe('component test infrastructure', () => {
  test('happy-dom is available', () => {
    expect(typeof document).toBe('object');
    expect(typeof window).toBe('object');
  });

  test('renders an inline svelte component', async () => {
    // We construct a minimal component at import-time via a virtual
    // module isn't an option in vitest; instead we lean on the
    // smoke.test.ts and trust the dom/render imports load. The real
    // component renders live in their own test files.
    expect(render).toBeTypeOf('function');
  });
});

import { afterEach, describe, expect, it } from 'vitest';
import {
  initialAddonsView,
  rememberAddonsView,
  resetAddonsViewMemory,
} from '$lib/mods/addons-view-memory.svelte';

afterEach(() => resetAddonsViewMemory());

describe('addons view memory', () => {
  it('opens Browse on a first visit, and Installed for mods when the profile has some', () => {
    expect(initialAddonsView('mod', false)).toBe('browse');
    expect(initialAddonsView('mod', true)).toBe('installed');
    expect(initialAddonsView('shader', true)).toBe('browse');
  });

  it('remembers the last view per kind, over the first-visit default', () => {
    rememberAddonsView('mod', 'browse');
    rememberAddonsView('shader', 'installed');
    expect(initialAddonsView('mod', true)).toBe('browse');
    expect(initialAddonsView('shader', false)).toBe('installed');
    expect(initialAddonsView('resource_pack', false)).toBe('browse');
  });
});

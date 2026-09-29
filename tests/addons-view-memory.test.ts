import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
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

  // The count lands a moment after a start or a profile switch. Not known is not "none": the
  // Installed view shows what is there — and an empty one holds the full drop area.
  it('opens Installed for mods while the profile’s count is not known yet', () => {
    expect(initialAddonsView('mod', null)).toBe('installed');
    expect(initialAddonsView('shader', null)).toBe('browse');
  });

  it('remembers the last view per kind, over the first-visit default', () => {
    rememberAddonsView('mod', 'browse');
    rememberAddonsView('shader', 'installed');
    expect(initialAddonsView('mod', true)).toBe('browse');
    expect(initialAddonsView('mod', null)).toBe('browse');
    expect(initialAddonsView('shader', false)).toBe('installed');
    expect(initialAddonsView('resource_pack', false)).toBe('browse');
  });

  // The page is the whole app shell and is not renderable under vitest (see
  // tests/page-preflight-wiring.test.ts): a source scan proves the wiring exists. A plain
  // `total > 0` read would say "no mods" before the count of THIS profile has landed.
  it('the page hands Add-ons the active profile’s own count, unknown until it lands', () => {
    const page = readFileSync(resolve('src/routes/+page.svelte'), 'utf8');
    expect(page).toContain('hasInstalledMods={stats.hasInstalledMods(activeInstance?.id ?? null)}');
    expect(page).not.toContain('hasInstalledMods={stats.installedStats');
  });
});

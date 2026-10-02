import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

// The scroll containers a browser lives in reserve the scrollbar's gutter (DESIGN.md §14). A
// browser reloads its list on every query; a list that turns shorter than the window drops the
// scrollbar, and without the reserved gutter the whole page moved 15 px sideways and back — and an
// open popover anchored in the filter bar, such as the Minecraft-version list, was left 15 px off
// its field (2026-10-02 regression F04). happy-dom lays nothing out, so the class is what is pinned.
const SCROLLERS: ReadonlyArray<[file: string, marker: RegExp]> = [
  // The instance's tab panel: Add-ons (mods, resource packs, shaders, data packs) browse here.
  [join('src', 'lib', 'layout', 'MainTabs.svelte'), /id="maintabpanel"/],
  // The Modpacks window's list, Browse and Imported.
  [join('src', 'lib', 'modpacks', 'ModpacksTab.svelte'), /<ModpackBrowseView/],
];

describe('browse scroll containers reserve the scrollbar gutter', () => {
  it.each(SCROLLERS)('%s', (file, marker) => {
    const src = readFileSync(file, 'utf8');
    expect(src).toMatch(marker);
    expect(src).toMatch(/overflow-y-auto \[scrollbar-gutter:stable\]/);
  });
});

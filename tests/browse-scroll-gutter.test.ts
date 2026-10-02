import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

// The scroll containers a browser lives in reserve the scrollbar's gutter (DESIGN.md §14). A
// browser reloads its list on every query; a list that turns shorter than the window drops the
// scrollbar, and without the reserved gutter the whole view moved 15 px sideways and back — and an
// open popover anchored in the filter bar, such as the Minecraft-version list, was left 15 px off
// its field (2026-10-02 regression F04). happy-dom lays nothing out, so the class on the container
// that holds the browser — the one that actually scrolls, not an outer one — is what is pinned.
const SCROLLERS: ReadonlyArray<[file: string, scroller: RegExp]> = [
  // The Add-ons tab's own list container (Browse and Installed, every content kind). The
  // instance tab panel around it does not overflow there, so a gutter on it would only take room.
  [
    join('src', 'lib', 'mods', 'AddonsTab.svelte'),
    /<div class="flex-1 overflow-y-auto \[scrollbar-gutter:stable\] relative">\s*<div class:hidden=\{view !== 'browse'\}>/,
  ],
  // The Modpacks window's list, Browse and Imported.
  [
    join('src', 'lib', 'modpacks', 'ModpacksTab.svelte'),
    /<div class="flex-1 overflow-y-auto \[scrollbar-gutter:stable\]">[\s\S]*?<ModpackBrowseView/,
  ],
];

describe('browse scroll containers reserve the scrollbar gutter', () => {
  it.each(SCROLLERS)('%s', (file, scroller) => {
    expect(readFileSync(file, 'utf8')).toMatch(scroller);
  });

  it('the instance tab panel around Add-ons reserves none: it does not scroll there', () => {
    const src = readFileSync(join('src', 'lib', 'layout', 'MainTabs.svelte'), 'utf8');
    expect(src).toMatch(/id="maintabpanel"/);
    expect(src).not.toMatch(/scrollbar-gutter/);
  });
});

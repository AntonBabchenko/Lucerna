// The reinstall warning names the pack and gives no count. A count was wrong
// twice over: the entry for a `saves/` folder that could not be listed read as
// "1 world", and a multi-pack install (a Vanilla Tweaks build, a drag-drop
// batch) pushed one identical warning per pack with nothing to tell them
// apart. The lines below the title say which worlds, or that no world was
// refreshed at all.
import { afterEach, describe, expect, it } from 'vitest';
import { warnFailedRefresh } from '$lib/mods/datapack-refresh-warning';
import { dismiss, toastList } from '$lib/toasts/toasts.svelte';

afterEach(() => {
  for (const x of toastList()) dismiss(x.id);
});

const warnings = () => toastList().filter((x) => x.kind === 'warning');

describe('warnFailedRefresh', () => {
  it('names the pack and counts no worlds', () => {
    warnFailedRefresh('VeinMiner', [
      { kind: 'refreshed', world: 'Beta' },
      { kind: 'failed', world: 'Alpha', details: 'locked' },
      { kind: 'failed', world: 'Gamma', details: 'denied' },
    ]);
    const [w] = warnings();
    expect(w.title).toBe(
      "The new version of VeinMiner didn't reach every world — these may still be using the old one:",
    );
    expect(w.lines).toEqual(['Alpha: locked', 'Gamma: denied']);
  });

  it('does not call a saves folder it could not list a world', () => {
    warnFailedRefresh('VeinMiner', [
      {
        kind: 'failed',
        world: '/inst/.minecraft/saves',
        details: 'no world was refreshed: access denied',
      },
    ]);
    const [w] = warnings();
    expect(w.title).not.toMatch(/\d/);
    expect(w.title).not.toMatch(/\bworld could\b|\b1 world\b/i);
    expect(w.lines).toEqual(['/inst/.minecraft/saves: no world was refreshed: access denied']);
  });

  it('tells two packs apart', () => {
    warnFailedRefresh('Graves', [{ kind: 'failed', world: 'Alpha', details: 'locked' }]);
    warnFailedRefresh('Coords HUD', [{ kind: 'failed', world: 'Alpha', details: 'locked' }]);
    const titles = warnings().map((w) => w.title);
    expect(titles).toHaveLength(2);
    expect(titles[0]).toMatch(/Graves/);
    expect(titles[1]).toMatch(/Coords HUD/);
  });

  it('warns about nothing when every world was refreshed', () => {
    warnFailedRefresh('VeinMiner', [{ kind: 'refreshed', world: 'Beta' }]);
    expect(warnings()).toHaveLength(0);
  });
});

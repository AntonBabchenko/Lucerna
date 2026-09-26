// The reinstall warning names the pack and gives no count. A count was wrong
// twice over: the entry for a `saves/` folder that could not be listed read as
// "1 world", and a multi-pack install (a Vanilla Tweaks build, a drag-drop
// batch) pushed one identical warning per pack with nothing to tell them
// apart. The lines below the title say which worlds, or that no world was
// refreshed at all.
import { afterEach, describe, expect, it } from 'vitest';
import { locale } from '$lib/i18n';
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
      {
        kind: 'failed',
        world: 'Alpha',
        error: { kind: 'io', path: 'saves/Alpha', details: 'locked' },
      },
      {
        kind: 'failed',
        world: 'Gamma',
        error: { kind: 'io', path: 'saves/Gamma', details: 'denied' },
      },
    ]);
    const [w] = warnings();
    expect(w.title).toBe(
      "The new version of VeinMiner didn't reach every world — these may still be using the old one:",
    );
    expect(w.lines).toEqual([
      'Alpha: IO error at saves/Alpha: locked',
      'Gamma: IO error at saves/Gamma: denied',
    ]);
  });

  it('does not count a saves folder it could not list as a world', () => {
    warnFailedRefresh('VeinMiner', [
      {
        kind: 'worlds_unchecked',
        error: { kind: 'io', path: '/inst/.minecraft/saves', details: 'access denied' },
      },
    ]);
    const [w] = warnings();
    expect(w.title).not.toMatch(/\d/);
    expect(w.title).not.toMatch(/\bworld could\b|\b1 world\b/i);
  });

  it('tells two packs apart', () => {
    const locked = { kind: 'io' as const, path: 'saves/Alpha', details: 'locked' };
    warnFailedRefresh('Graves', [{ kind: 'failed', world: 'Alpha', error: locked }]);
    warnFailedRefresh('Coords HUD', [{ kind: 'failed', world: 'Alpha', error: locked }]);
    const titles = warnings().map((w) => w.title);
    expect(titles).toHaveLength(2);
    expect(titles[0]).toMatch(/Graves/);
    expect(titles[1]).toMatch(/Coords HUD/);
  });

  // A Rust-made English sentence once sat under a Russian title. A failed
  // world carries the typed error; the line is worded in the UI language by
  // `formatError`, like every other error the launcher shows.
  it('words a failed world in the UI language, from its typed error', () => {
    locale.set('ru');
    try {
      warnFailedRefresh('VeinMiner', [
        {
          kind: 'failed',
          world: 'OnlyOld',
          error: { kind: 'world_level_dat_only_old', folder_name: 'OnlyOld' },
        },
      ]);
      const [w] = warnings();
      expect(w.lines).toHaveLength(1);
      expect(w.lines[0]).toMatch(/^OnlyOld: У мира «OnlyOld» пропал level\.dat/);
      expect(w.lines[0]).not.toMatch(/has only level\.dat_old/);
    } finally {
      locale.set('en');
    }
  });

  it('says the worlds could not be checked, instead of naming the saves folder as a world', () => {
    warnFailedRefresh('VeinMiner', [
      {
        kind: 'worlds_unchecked',
        error: { kind: 'io', path: '/inst/.minecraft/saves', details: 'access denied' },
      },
    ]);
    const [w] = warnings();
    expect(w.lines).toEqual([
      "Couldn't check the worlds: IO error at /inst/.minecraft/saves: access denied",
    ]);
  });

  it('warns about nothing when every world was refreshed', () => {
    warnFailedRefresh('VeinMiner', [{ kind: 'refreshed', world: 'Beta' }]);
    expect(warnings()).toHaveLength(0);
  });
});

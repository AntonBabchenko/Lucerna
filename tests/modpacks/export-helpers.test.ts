import { describe, expect, it } from 'vitest';
import type { ExportModInfo } from '$lib/ipc/bindings';
import { defaultExportFilename, unresolvableMods } from '$lib/modpacks/export';

const mk = (
  sha1: string,
  source: 'modrinth' | 'curseforge' | null,
  has_ids: boolean,
): ExportModInfo => ({ sha1, name: sha1, filename: `${sha1}.jar`, source, has_ids });

describe('defaultExportFilename', () => {
  it('slugifies the instance name and appends the format extension', () => {
    expect(defaultExportFilename('My Cool Pack!', '1.0.0', 'modrinth')).toBe(
      'my-cool-pack-1.0.0.mrpack',
    );
    expect(defaultExportFilename('My Cool Pack!', '1.0.0', 'curseforge')).toBe(
      'my-cool-pack-1.0.0.zip',
    );
  });
});

describe('unresolvableMods', () => {
  const mods = [mk('a', 'modrinth', true), mk('b', 'curseforge', true), mk('c', null, false)];

  it('full mode marks every mod unresolvable', () => {
    expect(unresolvableMods(mods, 'modrinth', 'full').map((m) => m.sha1)).toEqual(['a', 'b', 'c']);
  });

  it('mrpack lightweight only the source-less local jar', () => {
    expect(unresolvableMods(mods, 'modrinth', 'lightweight').map((m) => m.sha1)).toEqual(['c']);
  });

  it('cf lightweight marks modrinth + local mods unresolvable', () => {
    expect(unresolvableMods(mods, 'curseforge', 'lightweight').map((m) => m.sha1)).toEqual([
      'a',
      'c',
    ]);
  });
});

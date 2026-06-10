import { describe, expect, it } from 'vitest';
import type { DepProjectRef, LoaderKind, ModVersion } from '$lib/ipc/bindings';
import { enrichUnresolvable } from '$lib/mods/unresolvable-detail';

const MANIFEST = ['1.21', '1.20.6', '1.20.4', '1.19.2', '1.7.10'];
const NAMES: Record<LoaderKind, string> = {
  fabric: 'Fabric',
  forge: 'Forge',
  neoforge: 'NeoForge',
  quilt: 'Quilt',
  vanilla: 'Vanilla',
};
const displayLoader = (k: LoaderKind): string => NAMES[k];
const ref: DepProjectRef = { source: 'modrinth', project_id: 'jade', version_id: null };

function ver(loaders: LoaderKind[], mc: string[]): ModVersion {
  return {
    source: 'modrinth',
    project_id: 'jade',
    version_id: 'v',
    name: 'n',
    version_number: '1',
    primary_file: {
      url: 'u',
      filename: 'f.jar',
      sha1: 's',
      size: null,
      distribution_allowed: true,
    },
    deps: [],
    published_at: null,
    loaders,
    mc_versions: mc,
  } as ModVersion;
}

function deps(overrides: {
  fetchVersions: (r: DepProjectRef) => Promise<ModVersion[] | null>;
  currentLoader: LoaderKind;
  mcVersion: string;
  fetchProjectName?: (r: DepProjectRef) => Promise<string>;
}) {
  return {
    fetchProjectName: overrides.fetchProjectName ?? (async () => 'Jade'),
    fetchVersions: overrides.fetchVersions,
    manifestOrder: MANIFEST,
    displayLoader,
    currentLoader: overrides.currentLoader,
    mcVersion: overrides.mcVersion,
  };
}

describe('enrichUnresolvable', () => {
  it('classifies noMc when the dep supports the loader but not this MC', async () => {
    const res = await enrichUnresolvable(
      [ref],
      deps({
        fetchVersions: async () => [ver(['forge'], ['1.19.2'])],
        currentLoader: 'forge',
        mcVersion: '1.20.4',
      }),
    );
    expect(res[0]).toEqual({
      kind: 'noMc',
      name: 'Jade',
      mcVersion: '1.20.4',
      list: 'Forge 1.19.2',
    });
  });

  it('classifies wrongLoader when the dep ships only other loaders', async () => {
    const res = await enrichUnresolvable(
      [ref],
      deps({
        fetchVersions: async () => [ver(['fabric'], ['1.21'])],
        currentLoader: 'forge',
        mcVersion: '1.20.4',
      }),
    );
    expect(res[0]).toEqual({
      kind: 'wrongLoader',
      name: 'Jade',
      loader: 'Forge',
      list: 'Fabric 1.21',
    });
  });

  it('falls back to noVersions when the version lookup fails', async () => {
    const res = await enrichUnresolvable(
      [ref],
      deps({ fetchVersions: async () => null, currentLoader: 'forge', mcVersion: '1.20.4' }),
    );
    expect(res[0]).toEqual({ kind: 'noVersions', name: 'Jade' });
  });

  it('falls back to noVersions when the dep has no usable loader versions', async () => {
    const res = await enrichUnresolvable(
      [ref],
      deps({
        fetchVersions: async () => [ver(['vanilla'], ['1.21'])],
        currentLoader: 'forge',
        mcVersion: '1.20.4',
      }),
    );
    expect(res[0]).toEqual({ kind: 'noVersions', name: 'Jade' });
  });

  it('uses the resolved project name for the row', async () => {
    const res = await enrichUnresolvable(
      [ref],
      deps({
        fetchVersions: async () => [ver(['fabric'], ['1.21'])],
        currentLoader: 'forge',
        mcVersion: '1.20.4',
        fetchProjectName: async () => 'Balm',
      }),
    );
    expect(res[0]).toMatchObject({ name: 'Balm' });
  });

  it('classifies multiple refs independently and preserves order', async () => {
    const refA: DepProjectRef = { source: 'modrinth', project_id: 'a', version_id: null };
    const refB: DepProjectRef = { source: 'curseforge', mod_id: 42, file_id: null };
    const res = await enrichUnresolvable([refA, refB], {
      fetchProjectName: async (r) => (r.source === 'modrinth' ? 'Alpha' : 'Beta'),
      fetchVersions: async (r) =>
        r.source === 'modrinth' ? [ver(['forge'], ['1.19.2'])] : [ver(['fabric'], ['1.21'])],
      manifestOrder: MANIFEST,
      displayLoader,
      currentLoader: 'forge',
      mcVersion: '1.20.4',
    });
    expect(res).toEqual([
      { kind: 'noMc', name: 'Alpha', mcVersion: '1.20.4', list: 'Forge 1.19.2' },
      { kind: 'wrongLoader', name: 'Beta', loader: 'Forge', list: 'Fabric 1.21' },
    ]);
  });
});

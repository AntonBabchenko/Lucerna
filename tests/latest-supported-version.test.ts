import { describe, expect, it } from 'vitest';
import type { ModVersion } from '../src/lib/ipc/bindings';
import { latestSupportedPerLoader } from '../src/lib/mods/latest-supported-version';

// Newest-first manifest order (as listVersions returns it).
const MANIFEST = ['1.21', '1.20.6', '1.20.4', '1.19.2', '1.7.10'];

function v(partial: Partial<ModVersion> & Pick<ModVersion, 'loaders' | 'mc_versions'>): ModVersion {
  return {
    source: 'modrinth',
    project_id: 'p',
    version_id: 'v',
    name: 'n',
    version_number: '1',
    primary_file: { url: 'u', filename: 'f.jar', sha1: 's', size: null },
    deps: [],
    published_at: null,
    ...partial,
  } as ModVersion;
}

describe('latestSupportedPerLoader', () => {
  it('returns the latest MC version per loader using manifest order', () => {
    const versions = [
      v({ loaders: ['forge'], mc_versions: ['1.19.2'] }),
      v({ loaders: ['forge'], mc_versions: ['1.7.10'] }),
      v({ loaders: ['fabric'], mc_versions: ['1.21'] }),
      v({ loaders: ['fabric'], mc_versions: ['1.20.4'] }),
    ];
    expect(latestSupportedPerLoader(versions, MANIFEST)).toEqual([
      { loader: 'fabric', mcVersion: '1.21' },
      { loader: 'forge', mcVersion: '1.19.2' },
    ]);
  });

  it('keeps a fixed loader order (fabric, forge, neoforge, quilt)', () => {
    const versions = [
      v({ loaders: ['quilt'], mc_versions: ['1.21'] }),
      v({ loaders: ['neoforge'], mc_versions: ['1.20.6'] }),
      v({ loaders: ['forge'], mc_versions: ['1.20.4'] }),
      v({ loaders: ['fabric'], mc_versions: ['1.19.2'] }),
    ];
    expect(latestSupportedPerLoader(versions, MANIFEST).map((x) => x.loader)).toEqual([
      'fabric',
      'forge',
      'neoforge',
      'quilt',
    ]);
  });

  it('handles a version that lists multiple loaders and multiple mc_versions', () => {
    const versions = [v({ loaders: ['fabric', 'quilt'], mc_versions: ['1.20.4', '1.21'] })];
    expect(latestSupportedPerLoader(versions, MANIFEST)).toEqual([
      { loader: 'fabric', mcVersion: '1.21' },
      { loader: 'quilt', mcVersion: '1.21' },
    ]);
  });

  it('omits loaders the mod has no version for', () => {
    const versions = [v({ loaders: ['fabric'], mc_versions: ['1.21'] })];
    const result = latestSupportedPerLoader(versions, MANIFEST);
    expect(result).toEqual([{ loader: 'fabric', mcVersion: '1.21' }]);
  });

  it('skips a vanilla loader entry', () => {
    const versions = [v({ loaders: ['vanilla', 'forge'], mc_versions: ['1.20.4'] })];
    expect(latestSupportedPerLoader(versions, MANIFEST)).toEqual([
      { loader: 'forge', mcVersion: '1.20.4' },
    ]);
  });

  it('returns an empty array when there are no versions', () => {
    expect(latestSupportedPerLoader([], MANIFEST)).toEqual([]);
  });

  it('falls back to the first-seen mc version for versions not in the manifest', () => {
    // A snapshot / unknown MC not in the manifest still produces an entry.
    const versions = [v({ loaders: ['fabric'], mc_versions: ['24w14a'] })];
    expect(latestSupportedPerLoader(versions, MANIFEST)).toEqual([
      { loader: 'fabric', mcVersion: '24w14a' },
    ]);
  });
});

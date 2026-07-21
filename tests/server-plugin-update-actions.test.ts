import { describe, expect, it } from 'vitest';
import type { ModSummary, ModUpdateState, ModVersion } from '$lib/ipc/bindings';
import {
  autoUpdateTargets,
  countAutoUpdatable,
  externalUpdateUrl,
  hasUpdate,
  isAutoUpdatable,
} from '$lib/servers/addons/plugin-update-actions';

function version(overrides: Partial<ModVersion> = {}): ModVersion {
  return {
    source: 'hangar',
    project_id: 'LuckPerms',
    version_id: '5.4',
    name: '5.4',
    version_number: '5.4',
    mc_versions: ['1.21.4'],
    loaders: [],
    primary_file: {
      filename: 'LuckPerms-5.4.jar',
      url: 'https://hangar.papermc.io/download/LuckPerms/5.4',
      sha1: null,
      sha256: 'ab',
      size: 1,
      distribution_allowed: true,
    },
    deps: [],
    published_at: null,
    ...overrides,
  };
}

function available(target: ModVersion): ModUpdateState {
  return { kind: 'update_available', target };
}

const summary: ModSummary = {
  source: 'hangar',
  project_id: 'LuckPerms',
  slug: 'LuckPerms',
  name: 'LuckPerms',
  summary: '',
  icon_url: null,
  downloads: 1,
  author: 'Luck',
  updated_at: null,
};

describe('plugin-update-actions', () => {
  it('hasUpdate is true only for update_available', () => {
    expect(hasUpdate(available(version()))).toBe(true);
    expect(hasUpdate({ kind: 'up_to_date' })).toBe(false);
    expect(hasUpdate({ kind: 'check_failed', reason: 'x' })).toBe(false);
    expect(hasUpdate(null)).toBe(false);
    expect(hasUpdate(undefined)).toBe(false);
  });

  it('isAutoUpdatable excludes externally-hosted targets', () => {
    expect(isAutoUpdatable(available(version()))).toBe(true);
    const external = version({
      primary_file: { ...version().primary_file, distribution_allowed: false },
    });
    expect(isAutoUpdatable(available(external))).toBe(false);
    expect(isAutoUpdatable({ kind: 'unknown' })).toBe(false);
  });

  it('externalUpdateUrl prefers the file url, falls back to the project page', () => {
    const ext = version({
      primary_file: {
        ...version().primary_file,
        distribution_allowed: false,
        url: 'https://www.spigotmc.org/resources/luckperms.28140/',
      },
    });
    expect(externalUpdateUrl(ext, summary)).toBe(
      'https://www.spigotmc.org/resources/luckperms.28140/',
    );

    const noUrl = version({
      primary_file: { ...version().primary_file, distribution_allowed: false, url: '' },
    });
    expect(externalUpdateUrl(noUrl, summary)).toContain('LuckPerms');
    expect(externalUpdateUrl(noUrl, null)).toBeNull();
  });

  it('autoUpdateTargets / countAutoUpdatable skip external + non-updatable rows', () => {
    const external = version({
      version_id: '9.9',
      primary_file: { ...version().primary_file, distribution_allowed: false },
    });
    const rows = [{ sha1: 'a' }, { sha1: 'b' }, { sha1: 'c' }, { sha1: 'd' }];
    const checks = new Map<string, ModUpdateState>([
      ['a', available(version())], // auto-updatable
      ['b', available(external)], // external → skipped
      ['c', { kind: 'up_to_date' }], // no update
      // 'd' absent → no check
    ]);
    expect(countAutoUpdatable(rows, checks)).toBe(1);
    expect(autoUpdateTargets(rows, checks)).toEqual([{ sha: 'a', target: version() }]);
  });
});

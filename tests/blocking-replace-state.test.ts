import { describe, expect, it } from 'vitest';
import type { ModVersion } from '$lib/ipc/bindings';
import {
  getChosenVersion,
  getLoadedVersions,
  setChosenVersion,
  setLoadedVersions,
} from '$lib/logs/blocking-replace-state.svelte';

const v = (id: string): ModVersion => ({
  source: 'modrinth',
  project_id: 'p',
  version_id: id,
  name: id,
  version_number: id,
  mc_versions: ['1.20.1'],
  loaders: ['forge'],
  primary_file: { filename: 'x.jar', url: 'u', sha1: 's', size: 1, distribution_allowed: true },
  deps: [],
  published_at: null,
});

describe('blocking-replace-state', () => {
  it('round-trips chosen version + loaded versions by instance+sha1', () => {
    expect(getChosenVersion('i1', 'a')).toBeNull();
    expect(getLoadedVersions('i1', 'a')).toBeNull();

    setLoadedVersions('i1', 'a', [v('1.0'), v('1.1')]);
    setChosenVersion('i1', 'a', '1.1');

    expect(getLoadedVersions('i1', 'a')?.map((x) => x.version_id)).toEqual(['1.0', '1.1']);
    expect(getChosenVersion('i1', 'a')).toBe('1.1');
    // distinct key
    expect(getChosenVersion('i1', 'b')).toBeNull();
  });
});

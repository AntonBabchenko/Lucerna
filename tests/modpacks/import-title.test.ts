import { describe, expect, it } from 'vitest';
import { importTitle } from '$lib/modpacks/import-request';
import type { ModpackImportRequest } from '$lib/modpacks/import-request';

const base: ModpackImportRequest = {
  path: 'C:\\packs\\Haste-1.2.3.mrpack',
  selectedShas: [],
  displayName: '',
  projectId: null,
  source: null,
  versionId: null,
};

describe('importTitle', () => {
  it('uses the pack name the picker read from the manifest', () => {
    expect(importTitle({ ...base, displayName: 'Haste' })).toBe('Haste');
  });

  it('never falls back to the platform project id', () => {
    // The bug this pins: the project id is an opaque code (`1KVo5zza`) the
    // user has never seen. It must not reach the task title even when it is
    // the only identifier present besides the path.
    const title = importTitle({ ...base, projectId: '1KVo5zza' });
    expect(title).not.toBe('1KVo5zza');
    expect(title).toBe('Haste-1.2.3.mrpack');
  });

  it('falls back to the archive filename when no name was read', () => {
    expect(importTitle(base)).toBe('Haste-1.2.3.mrpack');
  });

  it('treats a whitespace-only name as absent', () => {
    expect(importTitle({ ...base, displayName: '   ' })).toBe('Haste-1.2.3.mrpack');
  });

  it('splits a POSIX path as well as a Windows one', () => {
    expect(importTitle({ ...base, path: '/home/a/Cobble.zip' })).toBe('Cobble.zip');
  });

  it('falls back to a generic label when neither name nor filename exists', () => {
    expect(importTitle({ ...base, path: '' })).toBe('modpack');
  });
});

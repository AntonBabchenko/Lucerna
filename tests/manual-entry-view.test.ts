import { describe, expect, it } from 'vitest';
import type { MissingModStatus, ModpackUnresolvable } from '$lib/ipc/bindings';
import { entryLink, needsAttention } from '$lib/modpacks/manual/entry-view';
import { isUnresolvedMissingState } from '$lib/modpacks/missing-mod';

const base: ModpackUnresolvable = {
  reason: 'distribution_disabled',
  mod_name: 'SRP',
  manual_action_url: 'https://www.curseforge.com/projects/1',
  filename: 'srp.jar',
  size: 1,
  sha1: 'aa',
  project_id: '1',
  meta: { file_id: '5', md5: null, install_path: null, required: true, download_url: null },
};

describe('entryLink', () => {
  it('prefers the file download page', () => {
    const e = {
      ...base,
      meta: {
        ...base.meta,
        download_url: 'https://www.curseforge.com/minecraft/mc-mods/srp/download/5',
      },
    };
    expect(entryLink(e)).toEqual({ url: e.meta.download_url, kind: 'download' });
  });
  it('calls a curseforge.com project link a page', () => {
    expect(entryLink(base)).toEqual({ url: base.manual_action_url, kind: 'page' });
  });
  it('calls any other link a link', () => {
    const e = {
      ...base,
      reason: 'host_not_allowed' as const,
      manual_action_url: 'https://example.com/x.zip',
    };
    expect(entryLink(e)).toEqual({ url: 'https://example.com/x.zip', kind: 'link' });
  });
  it('has nothing to open without a url', () => {
    expect(entryLink({ ...base, manual_action_url: '' })).toBeNull();
  });
});

describe('needsAttention', () => {
  it('never counts a manual step Lucerna cannot do', () => {
    const m: MissingModStatus = {
      entry: { ...base, reason: 'unsupported_type' },
      state: 'missing',
    };
    expect(needsAttention(m)).toBe(false);
  });
  it('treats a user-chosen file as resolved', () => {
    expect(isUnresolvedMissingState('chosen_unverified')).toBe(false);
    expect(needsAttention({ entry: base, state: 'missing' })).toBe(true);
  });
});

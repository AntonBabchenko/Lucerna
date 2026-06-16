import { describe, it, expect } from 'vitest';
import { cfKeyErrorStatus } from '$lib/settings/cf-key-status';
import type { Error as IpcError } from '$lib/ipc/bindings';

describe('cfKeyErrorStatus', () => {
  it('maps a genuine bad key to invalid', () => {
    const e: IpcError = { kind: 'mods_platform_auth', kind_detail: 'invalid' };
    expect(cfKeyErrorStatus(e)).toBe('invalid');
  });

  it('maps a region/Cloudflare block to unverified', () => {
    const e: IpcError = { kind: 'mods_platform_unreachable', url: 'https://api.curseforge.com' };
    expect(cfKeyErrorStatus(e)).toBe('unverified');
  });

  it('maps a network failure to unverified', () => {
    const e: IpcError = { kind: 'mods_network', url: 'https://x', details: 'timeout' };
    expect(cfKeyErrorStatus(e)).toBe('unverified');
  });

  it('maps a host-not-allowed failure to unverified', () => {
    const e: IpcError = { kind: 'host_not_allowed', url: 'https://x' };
    expect(cfKeyErrorStatus(e)).toBe('unverified');
  });

  it('falls back to invalid for any other error kind', () => {
    const e: IpcError = { kind: 'mods_decode', source: 'curseforge', details: 'boom' };
    expect(cfKeyErrorStatus(e)).toBe('invalid');
  });
});

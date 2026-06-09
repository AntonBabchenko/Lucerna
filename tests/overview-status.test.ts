import { describe, expect, it } from 'vitest';
import { deriveStatus } from '$lib/overview/status';

const ready = { mc_version: '1.20.1', ready: true };

describe('deriveStatus', () => {
  it('reports running with priority over everything else', () => {
    expect(deriveStatus({ mc_version: '', ready: false }, true, true)).toEqual({
      kind: 'running',
      tone: 'ok',
    });
  });

  it('reports pick_version when the mc version is empty (and not running)', () => {
    expect(deriveStatus({ mc_version: '', ready: false }, false, false)).toEqual({
      kind: 'pick_version',
      tone: 'warn',
    });
  });

  it('reports installing when an install is in flight', () => {
    expect(deriveStatus(ready, false, true)).toEqual({ kind: 'installing', tone: 'accent' });
  });

  it('reports needs_install when not ready', () => {
    expect(deriveStatus({ mc_version: '1.20.1', ready: false }, false, false)).toEqual({
      kind: 'needs_install',
      tone: 'warn',
    });
  });

  it('reports ready when installed and idle', () => {
    expect(deriveStatus(ready, false, false)).toEqual({ kind: 'ready', tone: 'ok' });
  });
});

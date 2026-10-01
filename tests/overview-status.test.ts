import { describe, expect, it } from 'vitest';
import { deriveStatus } from '$lib/overview/status';

const ready = { mc_version: '1.20.1', ready: true };

describe('deriveStatus', () => {
  it('reports running with priority over everything else', () => {
    expect(deriveStatus({ mc_version: '', ready: false }, true, true, 0)).toEqual({
      kind: 'running',
      tone: 'ok',
    });
  });

  it('reports pick_version when the mc version is empty (and not running)', () => {
    expect(deriveStatus({ mc_version: '', ready: false }, false, false, 0)).toEqual({
      kind: 'pick_version',
      tone: 'warn',
    });
  });

  it('reports installing when an install is in flight', () => {
    expect(deriveStatus(ready, false, true, 0)).toEqual({ kind: 'installing', tone: 'accent' });
  });

  it('reports needs_install when not ready', () => {
    expect(deriveStatus({ mc_version: '1.20.1', ready: false }, false, false, 0)).toEqual({
      kind: 'needs_install',
      tone: 'warn',
    });
  });

  it('reports ready when installed, idle and no mod stops the game', () => {
    expect(deriveStatus(ready, false, false, 0)).toEqual({ kind: 'ready', tone: 'ok' });
  });

  // Plan §5b V1 (screenshot 11): «Ready to play» sat beside «5 mods will stop the game from
  // starting». The pill follows the page pre-flight — the Play gate's own verdict.
  it('is not ready while mods stop the game', () => {
    expect(deriveStatus(ready, false, false, 5)).toEqual({ kind: 'mods_blocking', tone: 'danger' });
  });

  it('claims nothing about the mods until the pre-flight has answered', () => {
    expect(deriveStatus(ready, false, false, null)).toEqual({
      kind: 'mods_unknown',
      tone: 'neutral',
    });
  });

  it('keeps the launch and set-up states first — they are what the Play button shows', () => {
    expect(deriveStatus(ready, true, false, 5).kind).toBe('running');
    expect(deriveStatus({ mc_version: '1.20.1', ready: false }, false, false, 5).kind).toBe(
      'needs_install',
    );
    expect(deriveStatus(ready, false, true, null).kind).toBe('installing');
  });
});

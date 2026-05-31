import { describe, expect, test } from 'vitest';
import { resolveLoaderVersion } from '../src/lib/instances/loader-version';
import type { LoaderVersion } from '../src/lib/ipc/bindings';

const v = (version: string, stable = false): LoaderVersion => ({ version, stable, build: 0 });

describe('resolveLoaderVersion', () => {
  const list = [v('64.0.8', true), v('64.0.9'), v('63.0.1')];

  test('resetToStable picks the recommended build', () => {
    expect(resolveLoaderVersion('64.0.9', list, true)).toBe('64.0.8');
  });

  test('resetToStable falls back to first when none is stable', () => {
    expect(resolveLoaderVersion('x', [v('64.0.9'), v('63.0.1')], true)).toBe('64.0.9');
  });

  test('keeps a stored version that is still a real build', () => {
    expect(resolveLoaderVersion('64.0.9', list, false)).toBe('64.0.9');
  });

  test('replaces a stale stored version with the recommended one', () => {
    // The Forge-404 case: "58.1.0" is not in the list for this MC.
    expect(resolveLoaderVersion('58.1.0', list, false)).toBe('64.0.8');
  });

  test('replaces a null stored version with the recommended one', () => {
    expect(resolveLoaderVersion(null, list, false)).toBe('64.0.8');
  });

  test('returns null when the platform offers no builds', () => {
    expect(resolveLoaderVersion('64.0.8', [], false)).toBe(null);
    expect(resolveLoaderVersion('64.0.8', [], true)).toBe(null);
  });
});

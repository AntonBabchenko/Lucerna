import { describe, expect, it } from 'vitest';
import type { LogFileMeta } from '$lib/ipc/bindings';
import { clearOldPreview, isProtectedLog } from '$lib/logs/manage';

function meta(name: string, size: number): LogFileMeta {
  return { path: `/l/${name}`, name, source: 'game', size_bytes: size, modified_unix_ms: 0 };
}

describe('isProtectedLog', () => {
  it('protects latest.log and debug.log only', () => {
    expect(isProtectedLog('latest.log')).toBe(true);
    expect(isProtectedLog('debug.log')).toBe(true);
    expect(isProtectedLog('2024-01-01-1.log.gz')).toBe(false);
    expect(isProtectedLog('crash-1.txt')).toBe(false);
  });
});

describe('clearOldPreview', () => {
  it('counts non-protected files and sums their bytes', () => {
    const files = [
      meta('latest.log', 100),
      meta('debug.log', 100),
      meta('old-a.log', 30),
      meta('crash-1.txt', 20),
    ];
    expect(clearOldPreview(files)).toEqual({ count: 2, bytes: 50 });
  });

  it('returns zero when only protected files exist', () => {
    expect(clearOldPreview([meta('latest.log', 5), meta('debug.log', 5)])).toEqual({
      count: 0,
      bytes: 0,
    });
  });
});

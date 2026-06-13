import { describe, expect, it } from 'vitest';
import type { LogFileMeta } from '$lib/ipc/bindings';
import { chooseOpenLog } from '$lib/logs/select-log';

function meta(path: string, modifiedUnixMs: number): LogFileMeta {
  return { path, name: path, source: 'game', size_bytes: 0, modified_unix_ms: modifiedUnixMs };
}

describe('chooseOpenLog', () => {
  it('returns null when there are no logs', () => {
    expect(chooseOpenLog([], null)).toBeNull();
    expect(chooseOpenLog([], '/i/latest.log')).toBeNull();
  });

  it('selects the newest log (files[0]) when no deep-link is given', () => {
    // Backend sorts newest-first by mtime; the helper trusts that order, so a
    // fresh run's log (index 0) wins over a stale prior selection.
    const files = [meta('/i/latest.log', 200), meta('/i/2026-06-13-1.log.gz', 100)];
    expect(chooseOpenLog(files, null)).toBe('/i/latest.log');
  });

  it('honours an initialPath deep-link present in the list over the newest', () => {
    const files = [meta('/i/latest.log', 200), meta('/i/crash-2026.txt', 50)];
    expect(chooseOpenLog(files, '/i/crash-2026.txt')).toBe('/i/crash-2026.txt');
  });

  it('falls back to the newest when the deep-link file has rotated away', () => {
    const files = [meta('/i/latest.log', 200)];
    expect(chooseOpenLog(files, '/i/gone-2026-06-01.log.gz')).toBe('/i/latest.log');
  });
});

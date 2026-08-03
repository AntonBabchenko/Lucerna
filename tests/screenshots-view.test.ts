import { describe, expect, it } from 'vitest';
import type { Screenshot } from '$lib/ipc/bindings';
import { shotTime, sortShots } from '$lib/screenshots/screenshots-view';

function makeShot(over: Partial<Screenshot> = {}): Screenshot {
  return {
    instance_id: 'inst-1',
    instance_name: 'Instance One',
    file_name: 'shot.png',
    size_bytes: 1024,
    modified_unix_ms: 0,
    ...over,
  };
}

describe('shotTime', () => {
  it('normalises a null timestamp to 0', () => {
    expect(shotTime(makeShot({ modified_unix_ms: null }))).toBe(0);
  });

  it('returns the timestamp when present', () => {
    expect(shotTime(makeShot({ modified_unix_ms: 1500 }))).toBe(1500);
  });
});

describe('sortShots', () => {
  const older = makeShot({ file_name: 'older.png', modified_unix_ms: 1000 });
  const newer = makeShot({ file_name: 'newer.png', modified_unix_ms: 2000 });

  it('orders newest first', () => {
    const got = sortShots([older, newer], 'newest');
    expect(got.map((s) => s.file_name)).toEqual(['newer.png', 'older.png']);
  });

  it('orders oldest first', () => {
    const got = sortShots([older, newer], 'oldest');
    expect(got.map((s) => s.file_name)).toEqual(['older.png', 'newer.png']);
  });

  it('does not mutate its input', () => {
    const input = [older, newer];
    sortShots(input, 'oldest');
    expect(input.map((s) => s.file_name)).toEqual(['older.png', 'newer.png']);
  });

  it('sorts a shot with no timestamp to the tail when newest-first', () => {
    const undated = makeShot({ file_name: 'undated.png', modified_unix_ms: null });
    const got = sortShots([undated, older, newer], 'newest');
    expect(got[got.length - 1].file_name).toBe('undated.png');
  });
});

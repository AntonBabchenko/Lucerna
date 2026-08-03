import { describe, expect, it } from 'vitest';
import type { Screenshot } from '$lib/ipc/bindings';
import type { Translate } from '$lib/i18n';
import { groupLabel, groupShots, shotTime, sortShots } from '$lib/screenshots/screenshots-view';

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

describe('groupShots', () => {
  it('returns no groups for an empty list', () => {
    expect(groupShots([], 'day')).toEqual([]);
  });

  it('splits two shots either side of local midnight into two day groups', () => {
    const before = makeShot({
      file_name: 'before.png',
      modified_unix_ms: new Date(2026, 0, 15, 23, 59).getTime(),
    });
    const after = makeShot({
      file_name: 'after.png',
      modified_unix_ms: new Date(2026, 0, 16, 0, 1).getTime(),
    });
    const got = groupShots(sortShots([before, after], 'oldest'), 'day');
    expect(got).toHaveLength(2);
    expect(got[0].shots.map((s) => s.file_name)).toEqual(['before.png']);
    expect(got[1].shots.map((s) => s.file_name)).toEqual(['after.png']);
  });

  it('keeps the same two shots in one group at month granularity', () => {
    const before = makeShot({ modified_unix_ms: new Date(2026, 0, 15, 23, 59).getTime() });
    const after = makeShot({ modified_unix_ms: new Date(2026, 0, 16, 0, 1).getTime() });
    const got = groupShots(sortShots([before, after], 'oldest'), 'month');
    expect(got).toHaveLength(1);
    expect(got[0].shots).toHaveLength(2);
  });

  it('splits across a month boundary at month granularity', () => {
    const jan = makeShot({ modified_unix_ms: new Date(2026, 0, 31, 12, 0).getTime() });
    const feb = makeShot({ modified_unix_ms: new Date(2026, 1, 1, 12, 0).getTime() });
    const got = groupShots(sortShots([jan, feb], 'oldest'), 'month');
    expect(got).toHaveLength(2);
  });

  it('follows the order of the list it is given', () => {
    const jan = makeShot({ modified_unix_ms: new Date(2026, 0, 31, 12, 0).getTime() });
    const feb = makeShot({ modified_unix_ms: new Date(2026, 1, 1, 12, 0).getTime() });
    const newestFirst = groupShots(sortShots([jan, feb], 'newest'), 'day');
    expect(newestFirst[0].startMs).toBeGreaterThan(newestFirst[1].startMs);
  });

  it('gives each group a key that is unique per granularity', () => {
    const shot = makeShot({ modified_unix_ms: new Date(2026, 0, 15, 12, 0).getTime() });
    const [dayGroup] = groupShots([shot], 'day');
    const [monthGroup] = groupShots([shot], 'month');
    expect(dayGroup.key).not.toBe(monthGroup.key);
  });
});

// Stand-in translator: returns the key, so assertions do not depend on copy.
const tk = ((key: string) => key) as unknown as Translate;

function dayStart(offsetDays: number): number {
  const now = new Date();
  return new Date(now.getFullYear(), now.getMonth(), now.getDate() - offsetDays).getTime();
}

describe('groupLabel', () => {
  it("labels today's calendar day", () => {
    expect(groupLabel(tk, 'en', dayStart(0), 'day')).toBe('screenshots.groupToday');
  });

  it('labels yesterday', () => {
    expect(groupLabel(tk, 'en', dayStart(1), 'day')).toBe('screenshots.groupYesterday');
  });

  it('falls back to a formatted date for older days', () => {
    const label = groupLabel(tk, 'en', dayStart(10), 'day');
    expect(label).not.toContain('screenshots.');
    expect(label.length).toBeGreaterThan(0);
  });

  it('formats a month with its name and year in the given locale', () => {
    const label = groupLabel(tk, 'en', new Date(2026, 0, 1).getTime(), 'month');
    expect(label).toMatch(/january/i);
    expect(label).toContain('2026');
  });

  it('honours the locale it is passed rather than the OS locale', () => {
    const label = groupLabel(tk, 'ru', new Date(2026, 0, 1).getTime(), 'month');
    expect(label).toMatch(/январ/i);
  });
});

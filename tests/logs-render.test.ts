import { describe, expect, it } from 'vitest';
import { groupStackFolds, maybeParseCrashReport, tagWithSeverity } from '$lib/logs/render';

describe('tagWithSeverity', () => {
  it('parses MC level prefix INFO/WARN/ERROR', () => {
    const tagged = tagWithSeverity([
      '[12:00:00] [main/INFO] [foo]: hello',
      '[12:00:01] [Worker/WARN]: oops',
      '[12:00:02] [Render/ERROR]: boom',
    ]);
    expect(tagged.map((l) => l.level)).toEqual(['info', 'warn', 'error']);
  });

  it('inherits previous level for continuation lines', () => {
    const tagged = tagWithSeverity([
      '[12:00:00] [main/ERROR]: stack trace follows',
      '\tat com.foo.Bar.baz(Bar.java:10)',
      '\tat com.foo.Bar.qux(Bar.java:20)',
      '[12:00:01] [main/INFO]: back to normal',
    ]);
    expect(tagged.map((l) => l.level)).toEqual(['error', 'error', 'error', 'info']);
  });

  it("levels lines with no prefix at all as 'other' at start", () => {
    const tagged = tagWithSeverity(['random plain line', '[12:00:00] [m/INFO]: now we know']);
    expect(tagged[0].level).toBe('other');
    expect(tagged[1].level).toBe('info');
  });

  it('carries original line indices with offset', () => {
    const tagged = tagWithSeverity(['a', 'b'], 10);
    expect(tagged.map((l) => l.index)).toEqual([10, 11]);
  });
});

describe('groupStackFolds', () => {
  function line(text: string, level: 'info' | 'warn' | 'error' = 'error', index = 0) {
    return { text, level, index };
  }

  it('folds 5 consecutive at-lines into one unit', () => {
    const units = groupStackFolds([
      line('Caused by: java.lang.RuntimeException', 'error', 0),
      line('\tat com.foo.A.a(A.java:1)', 'error', 1),
      line('\tat com.foo.B.b(B.java:2)', 'error', 2),
      line('\tat com.foo.C.c(C.java:3)', 'error', 3),
      line('\tat com.foo.D.d(D.java:4)', 'error', 4),
      line('\tat com.foo.E.e(E.java:5)', 'error', 5),
      line('[12:00:00] [m/INFO]: done', 'info', 6),
    ]);
    // 1 caused-by line + 1 fold + 1 info line = 3 units
    expect(units.length).toBe(3);
    expect(units[1].kind).toBe('fold');
    if (units[1].kind === 'fold') {
      expect(units[1].hiddenFrames.length).toBe(4); // first frame stays visible
    }
  });

  it('leaves 2-3 frame traces unfolded', () => {
    const units = groupStackFolds([
      line('Caused by: x', 'error', 0),
      line('\tat a.b(a.java:1)', 'error', 1),
      line('\tat a.c(a.java:2)', 'error', 2),
      line('\tat a.d(a.java:3)', 'error', 3),
    ]);
    // No fold (under threshold), each frame stays as a line unit.
    expect(units.every((u) => u.kind === 'line')).toBe(true);
  });

  it('resets at Caused by:', () => {
    const units = groupStackFolds([
      line('exception 1', 'error', 0),
      line('\tat a.a(a.java:1)', 'error', 1),
      line('\tat a.b(a.java:2)', 'error', 2),
      line('\tat a.c(a.java:3)', 'error', 3),
      line('\tat a.d(a.java:4)', 'error', 4),
      line('\tat a.e(a.java:5)', 'error', 5),
      line('Caused by: exception 2', 'error', 6),
      line('\tat z.a(z.java:1)', 'error', 7),
      line('\tat z.b(z.java:2)', 'error', 8),
      line('\tat z.c(z.java:3)', 'error', 9),
      line('\tat z.d(z.java:4)', 'error', 10),
      line('\tat z.e(z.java:5)', 'error', 11),
    ]);
    const folds = units.filter((u) => u.kind === 'fold');
    expect(folds.length).toBe(2); // one per exception, not merged
  });

  it('preserves original indices on line units', () => {
    const tagged = tagWithSeverity(['head', '  at a.b(c.java:1)', 'tail']);
    const units = groupStackFolds(tagged);
    const lineUnits = units.filter((u) => u.kind === 'line');
    expect(lineUnits.map((u) => (u.kind === 'line' ? u.index : -1))).toEqual([0, 1, 2]);
  });
});

describe('maybeParseCrashReport', () => {
  it('returns null for non-crash text', () => {
    expect(maybeParseCrashReport('hello world')).toBeNull();
  });

  it('parses standard crash report into sections', () => {
    const body = [
      '---- Minecraft Crash Report ----',
      '// witty comment',
      'Time: 2026-05-26 12:00:00',
      'Description: Watching Subject',
      '',
      'java.lang.NullPointerException',
      '',
      '-- Affected level --',
      'Details:',
      '\tFoo: bar',
      '',
      '-- System Details --',
      'Details:',
      '\tMinecraft Version: 1.21.8',
    ].join('\n');
    const parsed = maybeParseCrashReport(body);
    expect(parsed).not.toBeNull();
    expect(parsed?.length).toBeGreaterThanOrEqual(3);
    expect(parsed?.[0].title).toBe('Head');
    expect(parsed?.some((s) => s.title === 'Affected level')).toBe(true);
    expect(parsed?.some((s) => s.title === 'System Details')).toBe(true);
  });

  it('records section start lines', () => {
    const body = [
      '---- Minecraft Crash Report ----',
      'head line',
      '-- System Details --',
      'detail',
    ].join('\n');
    const sections = maybeParseCrashReport(body);
    expect(sections?.[0].startLine).toBe(0);
    expect(sections?.[1].startLine).toBe(3);
  });
});

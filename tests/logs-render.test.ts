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
});

describe('groupStackFolds', () => {
  function line(text: string, level: 'info' | 'warn' | 'error' = 'error') {
    return { text, level };
  }

  it('folds 5 consecutive at-lines into one unit', () => {
    const units = groupStackFolds([
      line('Caused by: java.lang.RuntimeException'),
      line('\tat com.foo.A.a(A.java:1)'),
      line('\tat com.foo.B.b(B.java:2)'),
      line('\tat com.foo.C.c(C.java:3)'),
      line('\tat com.foo.D.d(D.java:4)'),
      line('\tat com.foo.E.e(E.java:5)'),
      line('[12:00:00] [m/INFO]: done', 'info'),
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
      line('Caused by: x'),
      line('\tat a.b(a.java:1)'),
      line('\tat a.c(a.java:2)'),
      line('\tat a.d(a.java:3)'),
    ]);
    // No fold (under threshold), each frame stays as a line unit.
    expect(units.every((u) => u.kind === 'line')).toBe(true);
  });

  it('resets at Caused by:', () => {
    const units = groupStackFolds([
      line('exception 1'),
      line('\tat a.a(a.java:1)'),
      line('\tat a.b(a.java:2)'),
      line('\tat a.c(a.java:3)'),
      line('\tat a.d(a.java:4)'),
      line('\tat a.e(a.java:5)'),
      line('Caused by: exception 2'),
      line('\tat z.a(z.java:1)'),
      line('\tat z.b(z.java:2)'),
      line('\tat z.c(z.java:3)'),
      line('\tat z.d(z.java:4)'),
      line('\tat z.e(z.java:5)'),
    ]);
    const folds = units.filter((u) => u.kind === 'fold');
    expect(folds.length).toBe(2); // one per exception, not merged
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
});

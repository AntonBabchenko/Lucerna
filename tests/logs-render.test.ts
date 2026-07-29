import { describe, expect, it } from 'vitest';
import {
  collapseRepeats,
  groupStackFolds,
  maybeParseCrashReport,
  type RenderUnit,
  repeatKey,
  tagWithSeverity,
} from '$lib/logs/render';

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

describe('repeatKey', () => {
  it('strips a leading MC timestamp so spam lines compare equal', () => {
    expect(repeatKey('[12:04:01] [Server thread/WARN]: Can not keep up!')).toBe(
      repeatKey('[12:04:59] [Server thread/WARN]: Can not keep up!'),
    );
  });

  it('strips a millisecond timestamp too', () => {
    expect(repeatKey('[12:04:01.123] [main/INFO]: tick')).toBe(
      repeatKey('[12:04:02.999] [main/INFO]: tick'),
    );
  });

  it('leaves a line with no timestamp untouched', () => {
    expect(repeatKey('java.lang.OutOfMemoryError: Java heap space')).toBe(
      'java.lang.OutOfMemoryError: Java heap space',
    );
  });

  it('does not treat different messages as equal', () => {
    expect(repeatKey('[12:04:01] [main/INFO]: a')).not.toBe(repeatKey('[12:04:01] [main/INFO]: b'));
  });
});

describe('collapseRepeats', () => {
  function unit(text: string, level: 'info' | 'warn' | 'error' = 'warn', index = 0): RenderUnit {
    return { kind: 'line', text, level, index };
  }
  function spam(second: number): RenderUnit {
    return unit(
      `[12:04:${String(second).padStart(2, '0')}] [Server thread/WARN]: Can not keep up!`,
    );
  }

  it('collapses consecutive lines that differ only by timestamp', () => {
    const out = collapseRepeats([spam(1), spam(2), spam(3), spam(4)]);
    expect(out.length).toBe(1);
    expect(out[0].kind).toBe('repeat');
    if (out[0].kind === 'repeat') {
      expect(out[0].count).toBe(4);
      // The first line stays visible; the rest are behind the expander.
      expect(out[0].hiddenLines.length).toBe(3);
      expect(out[0].text).toContain('[12:04:01]');
      expect(out[0].hiddenLines[0]).toContain('[12:04:02]');
    }
  });

  it('leaves a run below the threshold alone', () => {
    const out = collapseRepeats([spam(1), spam(2)]);
    expect(out.length).toBe(2);
    expect(out.every((u) => u.kind === 'line')).toBe(true);
  });

  it('does not collapse lines from different threads', () => {
    // Thread names are real information: Worker-1 and Worker-2 are different
    // producers, and merging them would hide that.
    const out = collapseRepeats([
      unit('[12:04:01] [Worker-Main-1/WARN]: slow'),
      unit('[12:04:02] [Worker-Main-2/WARN]: slow'),
      unit('[12:04:03] [Worker-Main-3/WARN]: slow'),
    ]);
    expect(out.length).toBe(3);
  });

  it('collapses identical lines that carry no timestamp', () => {
    const out = collapseRepeats([
      unit('OpenGL error 1281'),
      unit('OpenGL error 1281'),
      unit('OpenGL error 1281'),
    ]);
    expect(out.length).toBe(1);
    expect(out[0].kind === 'repeat' && out[0].count).toBe(3);
  });

  it('a stack fold breaks a run instead of merging across it', () => {
    const fold: RenderUnit = {
      kind: 'fold',
      level: 'error',
      firstFrame: '\tat com.foo.A.a(A.java:1)',
      hiddenFrames: ['\tat com.foo.B.b(B.java:2)'],
    };
    const out = collapseRepeats([spam(1), spam(2), spam(3), fold, spam(4), spam(5), spam(6)]);
    expect(out.map((u) => u.kind)).toEqual(['repeat', 'fold', 'repeat']);
  });

  it('keeps the first member level and original line index', () => {
    const out = collapseRepeats([
      unit('[12:04:01] [main/ERROR]: boom', 'error', 41),
      unit('[12:04:02] [main/ERROR]: boom', 'error', 42),
      unit('[12:04:03] [main/ERROR]: boom', 'error', 43),
    ]);
    expect(out[0].kind).toBe('repeat');
    if (out[0].kind === 'repeat') {
      expect(out[0].level).toBe('error');
      // Index preserved so an inline hint badge still attaches to the row.
      expect(out[0].index).toBe(41);
    }
  });

  it('preserves surrounding lines and their order', () => {
    const out = collapseRepeats([
      unit('[12:04:00] [main/INFO]: before', 'info', 0),
      spam(1),
      spam(2),
      spam(3),
      unit('[12:04:10] [main/INFO]: after', 'info', 4),
    ]);
    expect(out.map((u) => u.kind)).toEqual(['line', 'repeat', 'line']);
    expect(out[0].kind === 'line' && out[0].text).toContain('before');
    expect(out[2].kind === 'line' && out[2].text).toContain('after');
  });

  it('is identity for an empty list', () => {
    expect(collapseRepeats([])).toEqual([]);
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

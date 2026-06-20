import { describe, expect, it } from 'vitest';
import {
  countLevels,
  filterLevels,
  findMatches,
  highlightConsoleLine,
  presentLevels,
  tagConsoleLines,
} from '$lib/servers/console-filter';

const SAMPLE = [
  '[12:00:00] [Server thread/INFO]: Starting minecraft server',
  '[12:00:01] [Server thread/WARN]: Could not find world',
  '[12:00:02] [Server thread/ERROR]: Failed to load mod',
  '    at net.minecraft.Foo (Foo.java:1)', // inherits ERROR
  '[12:00:03] [Server thread/INFO]: Done!',
];

describe('console-filter: tagging + levels', () => {
  it('carries the severity forward onto stack/continuation lines', () => {
    const tagged = tagConsoleLines(SAMPLE);
    expect(tagged[2].level).toBe('error');
    // The unlabeled "at ..." line inherits the previous ERROR level.
    expect(tagged[3].level).toBe('error');
    expect(tagged[4].level).toBe('info');
  });

  it('counts lines per level', () => {
    const counts = countLevels(tagConsoleLines(SAMPLE));
    expect(counts.get('info')).toBe(2);
    expect(counts.get('warn')).toBe(1);
    expect(counts.get('error')).toBe(2); // ERROR line + inherited "at" line
  });

  it('lists present levels in severity order', () => {
    expect(presentLevels(tagConsoleLines(SAMPLE))).toEqual(['error', 'warn', 'info']);
  });
});

describe('console-filter: level filtering', () => {
  it('hides a level entirely', () => {
    const tagged = tagConsoleLines(SAMPLE);
    const visible = filterLevels(tagged, new Set(['info']));
    expect(visible.some((l) => l.level === 'info')).toBe(false);
    // The non-info lines survive.
    expect(visible.length).toBe(3);
  });

  it('is identity when nothing is hidden', () => {
    const tagged = tagConsoleLines(SAMPLE);
    expect(filterLevels(tagged, new Set())).toBe(tagged);
  });
});

describe('console-filter: search', () => {
  it('finds case-insensitive matches across lines', () => {
    const lines = ['Hello world', 'WORLD peace', 'nothing'];
    const m = findMatches(lines, 'world');
    expect(m).toHaveLength(2);
    expect(m[0]).toEqual({ line: 0, start: 6, end: 11 });
    expect(m[1]).toEqual({ line: 1, start: 0, end: 5 });
  });

  it('finds repeated matches on one line', () => {
    expect(findMatches(['aaa'], 'a')).toHaveLength(3);
  });

  it('returns nothing for a blank needle', () => {
    expect(findMatches(['anything'], '')).toHaveLength(0);
  });
});

describe('console-filter: highlight', () => {
  it('wraps matches in <mark> and tags the active one', () => {
    const matches = findMatches(['error: boom error'], 'error');
    const html = highlightConsoleLine('error: boom error', 0, matches, 1);
    expect(html).toContain('<mark>error</mark>'); // first (inactive)
    expect(html).toContain('data-match-active="true"'); // second (active)
  });

  it('escapes HTML so log content cannot inject markup', () => {
    const html = highlightConsoleLine('<script>alert(1)</script>', 0, [], 0);
    expect(html).not.toContain('<script>');
    expect(html).toContain('&lt;script&gt;');
  });
});

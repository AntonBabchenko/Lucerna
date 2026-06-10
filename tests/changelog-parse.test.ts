import { describe, expect, it } from 'vitest';
import { parseChangelog } from '$lib/changelog/parse';

describe('parseChangelog', () => {
  it('parses a version heading with an em-dash date', () => {
    const log = parseChangelog('## [0.11.0] — 2026-06-05\n\n### Added\n- A thing\n');
    expect(log).toHaveLength(1);
    expect(log[0].version).toBe('0.11.0');
    expect(log[0].date).toBe('2026-06-05');
    expect(log[0].sections[0]).toEqual({ kind: 'added', heading: 'Added', items: ['A thing'] });
  });

  it('parses a version heading with a plain-hyphen date', () => {
    const log = parseChangelog('## [1.2.3] - 2026-01-01\n\n### Fixed\n- Bug\n');
    expect(log[0].version).toBe('1.2.3');
    expect(log[0].date).toBe('2026-01-01');
    expect(log[0].sections[0].kind).toBe('fixed');
  });

  it('parses a version heading with an en-dash date', () => {
    const log = parseChangelog('## [2.0.0] – 2026-03-03\n\n### Changed\n- C\n');
    expect(log[0].version).toBe('2.0.0');
    expect(log[0].date).toBe('2026-03-03');
    expect(log[0].sections[0].kind).toBe('changed');
  });

  it('silently drops bullets that appear before any section heading', () => {
    const log = parseChangelog('## [0.1.0] — 2026-01-01\n- orphan bullet\n\n### Added\n- real\n');
    expect(log[0].sections).toHaveLength(1);
    expect(log[0].sections[0].items).toEqual(['real']);
  });

  it('gives a dateless version (Unreleased) a null date and no sections', () => {
    const log = parseChangelog(
      '## [Unreleased]\n\n## [0.1.0] — 2026-05-13\n\n### Added\n- First\n',
    );
    const unreleased = log.find((v) => v.version === 'Unreleased');
    expect(unreleased).toBeDefined();
    expect(unreleased?.date).toBeNull();
    expect(unreleased?.sections).toEqual([]);
  });

  it('maps known kinds case-insensitively and falls back to other', () => {
    const md = '## [0.1.0] — 2026-01-01\n\n### Security\n- s\n\n### Notes\n- n\n';
    const log = parseChangelog(md);
    expect(log[0].sections[0].kind).toBe('security');
    expect(log[0].sections[1].kind).toBe('other');
    expect(log[0].sections[1].heading).toBe('Notes');
  });

  it('collapses a wrapped multi-line bullet into one entry', () => {
    const md = '## [0.1.0] — 2026-01-01\n\n### Added\n- First line\n  second line\n  third line\n';
    const log = parseChangelog(md);
    expect(log[0].sections[0].items).toEqual(['First line second line third line']);
  });

  it('attaches a link-reference URL to the matching version', () => {
    const md =
      '## [0.2.0] — 2026-02-02\n\n### Added\n- x\n\n## [0.1.0] — 2026-01-01\n\n### Added\n- y\n\n' +
      '[0.2.0]: https://example.test/v0.2.0\n';
    const log = parseChangelog(md);
    expect(log.find((v) => v.version === '0.2.0')?.url).toBe('https://example.test/v0.2.0');
    expect(log.find((v) => v.version === '0.1.0')?.url).toBeNull();
  });

  it('returns an empty array for non-changelog garbage without throwing', () => {
    expect(parseChangelog('just some prose\nwith no headings')).toEqual([]);
    expect(parseChangelog('')).toEqual([]);
  });

  it('ignores intro content before the first version heading', () => {
    const md =
      '# Changelog\n\nSome intro paragraph.\n\n## [0.1.0] — 2026-01-01\n\n### Added\n- x\n';
    const log = parseChangelog(md);
    expect(log).toHaveLength(1);
    expect(log[0].version).toBe('0.1.0');
  });
});

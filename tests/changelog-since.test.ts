import { describe, expect, it } from 'vitest';
import { changelogSince, hasRenderableEntry } from '$lib/changelog/since';
import type { Changelog } from '$lib/changelog/types';

function v(version: string, withSection = true): Changelog[number] {
  return {
    version,
    date: '2026-01-01',
    url: null,
    sections: withSection ? [{ kind: 'added', heading: 'Added', items: [`item ${version}`] }] : [],
  };
}

// Newest-first, like the parser's output.
const LOG: Changelog = [v('0.23.0'), v('0.22.0'), v('0.21.0')];

describe('changelogSince', () => {
  it('returns [] on first-ever launch (seen == null)', () => {
    expect(changelogSince(LOG, '0.23.0', null)).toEqual([]);
  });

  it('returns [] when the current version is absent from the changelog', () => {
    expect(changelogSince(LOG, '9.9.9', '0.22.0')).toEqual([]);
  });

  it('returns just the current version when seen is absent from the changelog', () => {
    const r = changelogSince(LOG, '0.23.0', '0.1.0');
    expect(r.map((x) => x.version)).toEqual(['0.23.0']);
  });

  it('returns every version newer than seen, up to current (newest-first)', () => {
    const r = changelogSince(LOG, '0.23.0', '0.21.0');
    expect(r.map((x) => x.version)).toEqual(['0.23.0', '0.22.0']);
  });

  it('returns [] when already on the seen version', () => {
    expect(changelogSince(LOG, '0.22.0', '0.22.0')).toEqual([]);
  });

  it('returns [] on a downgrade (current older than seen)', () => {
    expect(changelogSince(LOG, '0.21.0', '0.23.0')).toEqual([]);
  });
});

describe('hasRenderableEntry', () => {
  it('is false for an empty list', () => {
    expect(hasRenderableEntry([])).toBe(false);
  });

  it('is false when the only new version has no sections (empty Unreleased)', () => {
    expect(hasRenderableEntry([v('Unreleased', false)])).toBe(false);
  });

  it('is true when at least one version has a section', () => {
    expect(hasRenderableEntry([v('0.23.0')])).toBe(true);
  });
});

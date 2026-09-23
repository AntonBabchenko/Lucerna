// UPD-09: in Settings the installed version stays open and every other version
// sits in one disclosure; the What's-new dialog (no collapseOlder) is unchanged.
import { render, screen } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import ChangelogPanel from '$lib/changelog/ChangelogPanel.svelte';
import type { Changelog } from '$lib/changelog/types';

const v = (version: string): Changelog[number] => ({
  version,
  date: version === 'Unreleased' ? null : '2026-01-01',
  url: null,
  sections: [{ kind: 'added', heading: 'Added', items: [`Thing in ${version}`] }],
});
const ENTRIES: Changelog = [v('Unreleased'), v('0.3.0'), v('0.2.0'), v('0.1.0')];

const inDetails = (text: string) => screen.getByText(text).closest('details') !== null;

describe('the changelog in Settings', () => {
  it('keeps the installed version open and folds every other one away', () => {
    render(ChangelogPanel, { entries: ENTRIES, collapseOlder: '0.2.0' });
    expect(inDetails('Thing in 0.2.0')).toBe(false);
    for (const other of ['Unreleased', '0.3.0', '0.1.0']) {
      expect(inDetails(`Thing in ${other}`)).toBe(true);
    }
    expect(document.querySelectorAll('details')).toHaveLength(1);
    expect(screen.getByText('Show 3 more versions')).toBeTruthy();
  });

  it('opens the newest release when the installed version is not listed', () => {
    render(ChangelogPanel, { entries: ENTRIES, collapseOlder: '9.9.9' });
    expect(inDetails('Thing in 0.3.0')).toBe(false);
    expect(inDetails('Thing in Unreleased')).toBe(true);
  });

  it('shows everything open when not asked to collapse (the What’s-new dialog)', () => {
    render(ChangelogPanel, { entries: ENTRIES });
    expect(document.querySelectorAll('details')).toHaveLength(0);
  });
});

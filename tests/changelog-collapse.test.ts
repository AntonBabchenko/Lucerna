// Settings → Updates: every version opens and closes on its own and the installed one starts
// open; the What's-new dialog (no installedVersion) shows its versions open, as before.
import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { Changelog } from '$lib/changelog/types';

const openUrlMock = vi.fn().mockResolvedValue(undefined);
vi.mock('@tauri-apps/plugin-opener', () => ({
  openUrl: (url: string) => openUrlMock(url),
}));

import ChangelogPanel from '$lib/changelog/ChangelogPanel.svelte';

const v = (version: string): Changelog[number] => ({
  version,
  date: version === 'Unreleased' ? null : '2026-01-01',
  url: version === 'Unreleased' ? null : `https://example.test/v${version}`,
  sections: [{ kind: 'added', heading: 'Added', items: [`Thing in ${version}`] }],
});
const ENTRIES: Changelog = [v('Unreleased'), v('0.3.0'), v('0.2.0'), v('0.1.0')];

const toggle = (version: string) => screen.getByRole('button', { name: `v${version}` });
const isOpen = (version: string) => toggle(version).getAttribute('aria-expanded') === 'true';
const shows = (version: string) => screen.queryByText(`Thing in ${version}`) !== null;

describe('the changelog in Settings', () => {
  it('folds every version on its own and opens the installed one', () => {
    render(ChangelogPanel, { entries: ENTRIES, installedVersion: '0.2.0' });
    expect(isOpen('0.2.0')).toBe(true);
    expect(shows('0.2.0')).toBe(true);
    for (const other of ['Unreleased', '0.3.0', '0.1.0']) {
      expect(isOpen(other)).toBe(false);
      expect(shows(other)).toBe(false);
    }
    expect(document.querySelectorAll('details')).toHaveLength(0);
    expect(screen.queryByText(/more version/i)).toBeNull();
  });

  it('opens and closes each version independently of the others', async () => {
    render(ChangelogPanel, { entries: ENTRIES, installedVersion: '0.2.0' });
    await fireEvent.click(toggle('0.1.0'));
    expect(isOpen('0.1.0')).toBe(true);
    expect(shows('0.1.0')).toBe(true);
    expect(isOpen('0.2.0')).toBe(true);

    await fireEvent.click(toggle('0.2.0'));
    expect(isOpen('0.2.0')).toBe(false);
    expect(shows('0.2.0')).toBe(false);
    expect(isOpen('0.1.0')).toBe(true);

    await fireEvent.click(toggle('0.2.0'));
    expect(isOpen('0.2.0')).toBe(true);
  });

  it('names the open body in aria-controls only while it is rendered', () => {
    render(ChangelogPanel, { entries: ENTRIES, installedVersion: '0.2.0' });
    const id = toggle('0.2.0').getAttribute('aria-controls');
    expect(id).toBeTruthy();
    expect(document.getElementById(id as string)?.textContent).toContain('Thing in 0.2.0');
    expect(toggle('0.1.0').hasAttribute('aria-controls')).toBe(false);
  });

  it('opens the newest release when the installed version is not listed', () => {
    render(ChangelogPanel, { entries: ENTRIES, installedVersion: '9.9.9' });
    expect(isOpen('0.3.0')).toBe(true);
    expect(isOpen('Unreleased')).toBe(false);
  });

  it('keeps each version an h4 named by the version, with its toggle inside and the date beside it', () => {
    render(ChangelogPanel, { entries: ENTRIES, installedVersion: '0.2.0' });
    const heading = screen.getByRole('heading', { level: 4, name: 'v0.1.0' });
    expect(heading.querySelector('button[aria-expanded]')).not.toBeNull();
    expect(heading.textContent).not.toContain('2026-01-01');
    // Only the open version renders its sections.
    expect(screen.getAllByRole('heading', { level: 5, name: 'Added' })).toHaveLength(1);
  });

  it('ends an open version with its compare link, opened through the https-only opener', async () => {
    openUrlMock.mockClear();
    render(ChangelogPanel, { entries: ENTRIES, installedVersion: '0.2.0' });
    const links = screen.getAllByRole('button', { name: 'Changes on GitHub' });
    expect(links).toHaveLength(1);
    await fireEvent.click(links[0]);
    await vi.waitFor(() => {
      expect(openUrlMock).toHaveBeenCalledWith('https://example.test/v0.2.0');
    });
  });

  it('leaves out the compare line for a version without a URL', () => {
    render(ChangelogPanel, { entries: ENTRIES, installedVersion: 'Unreleased' });
    expect(isOpen('Unreleased')).toBe(true);
    expect(screen.queryByRole('button', { name: 'Changes on GitHub' })).toBeNull();
  });
});

describe('the What’s-new dialog', () => {
  it('shows every version open, with no toggle and no compare line', () => {
    render(ChangelogPanel, { entries: ENTRIES });
    for (const ver of ['Unreleased', '0.3.0', '0.2.0', '0.1.0']) expect(shows(ver)).toBe(true);
    expect(document.querySelectorAll('[aria-expanded]')).toHaveLength(0);
    expect(screen.queryByRole('button', { name: 'Changes on GitHub' })).toBeNull();
  });
});

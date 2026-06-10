import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { Changelog } from '$lib/changelog/types';

const openUrlMock = vi.fn().mockResolvedValue(undefined);
vi.mock('@tauri-apps/plugin-opener', () => ({
  openUrl: (url: string) => openUrlMock(url),
}));

import ChangelogPanel from '$lib/changelog/ChangelogPanel.svelte';

const SAMPLE: Changelog = [
  {
    version: 'Unreleased',
    date: null,
    url: null,
    sections: [], // must be hidden
  },
  {
    version: '0.2.0',
    date: '2026-02-02',
    url: 'https://example.test/v0.2.0',
    sections: [
      { kind: 'added', heading: 'Added', items: ['New thing'] },
      { kind: 'other', heading: 'Notes', items: ['A note'] },
    ],
  },
];

describe('ChangelogPanel', () => {
  it('renders a version with its date and localized section heading', () => {
    render(ChangelogPanel, { props: { entries: SAMPLE } });
    expect(screen.getByText('v0.2.0')).toBeTruthy();
    expect(screen.getByText('2026-02-02')).toBeTruthy();
    expect(screen.getByText('Added')).toBeTruthy();
    expect(screen.getByText('New thing')).toBeTruthy();
  });

  it('renders the verbatim heading for an unknown (other) section kind', () => {
    render(ChangelogPanel, { props: { entries: SAMPLE } });
    expect(screen.getByText('Notes')).toBeTruthy();
    expect(screen.getByText('A note')).toBeTruthy();
  });

  it('hides versions that have no sections (empty Unreleased)', () => {
    render(ChangelogPanel, { props: { entries: SAMPLE } });
    expect(screen.queryByText('vUnreleased')).toBeNull();
  });

  it('opens the version URL via tauri-plugin-opener when the version is clicked', async () => {
    render(ChangelogPanel, { props: { entries: SAMPLE } });
    const link = screen.getByRole('button', { name: /v0\.2\.0/ });
    await fireEvent.click(link);
    await vi.waitFor(() => {
      expect(openUrlMock).toHaveBeenCalledWith('https://example.test/v0.2.0');
    });
  });

  it('shows the empty-state message when there are no visible versions', () => {
    render(ChangelogPanel, { props: { entries: [] } });
    expect(screen.getByText('No changelog available.')).toBeTruthy();
  });
});

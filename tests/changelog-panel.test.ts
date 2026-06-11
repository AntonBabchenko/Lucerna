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
      { kind: 'added', heading: 'WRONG_HEADING', items: ['New thing'] },
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
    // The localized label (from i18n), not the raw heading, must be shown.
    expect(screen.queryByText('WRONG_HEADING')).toBeNull();
  });

  it('renders the verbatim heading for an unknown (other) section kind', () => {
    render(ChangelogPanel, { props: { entries: SAMPLE } });
    expect(screen.getByText('Notes')).toBeTruthy();
    expect(screen.getByText('A note')).toBeTruthy();
  });

  it('renders inline bold/code markup instead of literal markers', () => {
    const log: Changelog = [
      {
        version: '0.3.0',
        date: '2026-03-03',
        url: null,
        sections: [{ kind: 'added', heading: 'Added', items: ['**Bold lead.** and `code` here'] }],
      },
    ];
    render(ChangelogPanel, { props: { entries: log } });
    const strong = screen.getByText('Bold lead.');
    expect(strong.tagName).toBe('STRONG');
    const code = screen.getByText('code');
    expect(code.tagName).toBe('CODE');
    // No literal Markdown markers leak through.
    expect(screen.queryByText(/\*\*/)).toBeNull();
  });

  it('hides versions that have no sections (empty Unreleased)', () => {
    render(ChangelogPanel, { props: { entries: SAMPLE } });
    expect(screen.queryByText('vUnreleased')).toBeNull();
  });

  it('opens the version URL via tauri-plugin-opener when the version is clicked', async () => {
    render(ChangelogPanel, { props: { entries: SAMPLE } });
    const link = screen.getByRole('button', { name: /v0\.2\.0/ });
    expect(screen.getByRole('button', { name: /release notes on GitHub/i })).toBeTruthy();
    await fireEvent.click(link);
    await vi.waitFor(() => {
      expect(openUrlMock).toHaveBeenCalledWith('https://example.test/v0.2.0');
    });
  });

  it('refuses to open a non-https version URL (defense-in-depth)', async () => {
    const log: Changelog = [
      {
        version: '0.4.0',
        date: '2026-04-04',
        url: 'file:///etc/passwd',
        sections: [{ kind: 'added', heading: 'Added', items: ['x'] }],
      },
    ];
    openUrlMock.mockClear(); // module-level mock accumulates across tests
    render(ChangelogPanel, { props: { entries: log } });
    await fireEvent.click(screen.getByRole('button', { name: /v0\.4\.0/ }));
    // The opener is never handed a non-https scheme.
    await new Promise((r) => setTimeout(r, 20));
    expect(openUrlMock).not.toHaveBeenCalled();
  });

  it('shows the empty-state message when there are no visible versions', () => {
    render(ChangelogPanel, { props: { entries: [] } });
    expect(screen.getByText('No changelog available.')).toBeTruthy();
  });
});

import { fireEvent, render, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { Changelog } from '$lib/changelog/types';

vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn() }));

import WhatsNewModal from '$lib/changelog/WhatsNewModal.svelte';
import { whatsNewState } from '$lib/changelog/whats-new.svelte';

const SINCE: Changelog = [
  {
    version: '0.23.0',
    date: '2026-08-12',
    url: null,
    sections: [{ kind: 'added', heading: 'Added', items: ['Post-update changelog prompt'] }],
  },
];

describe('WhatsNewModal', () => {
  beforeEach(() => {
    whatsNewState.entries = null;
  });

  it('renders nothing until entries are set', () => {
    render(WhatsNewModal);
    expect(screen.queryByTestId('whats-new-body')).toBeNull();
  });

  it('shows the current version in the title and the scoped entries', () => {
    whatsNewState.entries = SINCE;
    render(WhatsNewModal);
    expect(screen.getByText(/What.s new in Lucerna 0\.23\.0/)).toBeTruthy();
    expect(screen.getByText('Post-update changelog prompt')).toBeTruthy();
  });

  it('closes, clearing whatsNewState', async () => {
    whatsNewState.entries = SINCE;
    render(WhatsNewModal);
    await fireEvent.click(screen.getByRole('button', { name: /close/i }));
    expect(whatsNewState.entries).toBeNull();
  });
});

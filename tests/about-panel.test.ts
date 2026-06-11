import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import { DISCLAIMER_TEXT, REPO_URL } from '$lib/settings/disclaimer';
import pkg from '../package.json' with { type: 'json' };

const openUrlMock = vi.fn().mockResolvedValue(undefined);
vi.mock('@tauri-apps/plugin-opener', () => ({
  openUrl: (url: string) => openUrlMock(url),
}));

import AboutPanel from '$lib/settings/AboutPanel.svelte';

describe('AboutPanel', () => {
  it('renders the verbatim Minecraft usage disclaimer', () => {
    render(AboutPanel);
    expect(screen.getByText(DISCLAIMER_TEXT)).toBeTruthy();
  });

  it('renders the app version sourced from package.json', () => {
    render(AboutPanel);
    expect(screen.getByText(`Lucerna v${pkg.version}`)).toBeTruthy();
  });

  it('opens the repo URL via tauri-plugin-opener when the repo button is clicked', async () => {
    render(AboutPanel);
    // Scope to the repo button specifically — the embedded changelog adds
    // version buttons whose labels also mention GitHub ("release notes on
    // GitHub"), so a bare /github/i would now be ambiguous.
    const link = screen.getByRole('button', { name: /repository on GitHub/i });
    await fireEvent.click(link);
    // The opener is dynamic-imported and called inside an awaited promise —
    // wait for the module load + then-chain to flush.
    await vi.waitFor(() => {
      expect(openUrlMock).toHaveBeenCalledWith(REPO_URL);
    });
  });

  it('renders the Mojang/Microsoft trademark attribution', () => {
    render(AboutPanel);
    expect(screen.getByText(/Minecraft and Mojang are trademarks/)).toBeTruthy();
  });

  it('tucks the changelog into a "What\'s new" disclosure', () => {
    render(AboutPanel);
    // The disclosure summary, and the embedded changelog content (the 0.1.0
    // first release is always present in CHANGELOG.md), are in the DOM.
    expect(screen.getByText("What's new")).toBeTruthy();
    expect(screen.getByText('v0.1.0')).toBeTruthy();
  });
});

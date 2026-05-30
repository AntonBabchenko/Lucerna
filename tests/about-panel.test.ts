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
    expect(screen.getByText(`FTlauncher v${pkg.version}`)).toBeTruthy();
  });

  it('opens the repo URL via tauri-plugin-opener when the repo button is clicked', async () => {
    render(AboutPanel);
    const link = screen.getByRole('button', { name: /github/i });
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
});

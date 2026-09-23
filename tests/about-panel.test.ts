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
    const link = screen.getByRole('button', { name: 'View on GitHub' });
    await fireEvent.click(link);
    // The opener is dynamic-imported behind openExternalHttps — wait for the
    // module load + then-chain to flush.
    await vi.waitFor(() => {
      expect(openUrlMock).toHaveBeenCalledWith(REPO_URL);
    });
  });

  it('names the GitHub button by its visible text; the URL is the tooltip, not the name', () => {
    // Label-in-name: a voice user says "View on GitHub" and expects the button
    // whose label that is. The destination stays in use:tooltip (DESIGN §5).
    render(AboutPanel);
    const btn = screen.getByRole('button', { name: 'View on GitHub' });
    expect(btn.hasAttribute('aria-label')).toBe(false);
  });

  it('states the licence and the Mojang download in plain words, keeping both facts', () => {
    render(AboutPanel);
    // ABOUT-07: "at runtime" was developer jargon. The two facts it carries —
    // GPL-3.0-or-later, and that the downloaded files are never modified —
    // must survive the rewording verbatim (decision 12 reserves the rest).
    const licence = screen.getByText(/GPL-3\.0-or-later/);
    expect(licence.textContent).toContain('when they are first needed');
    expect(licence.textContent).toContain('never modified');
    expect(licence.textContent).not.toContain('at runtime');
  });

  it('names the product in the trademark line instead of "This launcher"', () => {
    render(AboutPanel);
    const mark = screen.getByText(/Minecraft and Mojang are trademarks/);
    expect(mark.textContent).toContain('Lucerna is not affiliated');
    expect(mark.textContent).toContain('Mojang Synergies AB');
    expect(mark.textContent).toContain('Microsoft Corporation');
  });

  it('renders the Mojang/Microsoft trademark attribution', () => {
    render(AboutPanel);
    expect(screen.getByText(/Minecraft and Mojang are trademarks/)).toBeTruthy();
  });

  it('no longer renders the changelog (moved to the Updates section)', () => {
    render(AboutPanel);
    expect(screen.queryByText("What's new")).toBeNull();
    expect(screen.queryByText('v0.1.0')).toBeNull();
  });

  it('has no page title and opens three blocks with the shared heading recipe', () => {
    render(AboutPanel);
    expect(screen.queryByRole('heading', { name: 'About' })).toBeNull();
    const h3 = screen.getAllByRole('heading', { level: 3 });
    expect(h3.map((h) => h.textContent?.trim())).toEqual(['Lucerna', 'Your data', 'Legal']);
    for (const h of h3) expect(h.className).toContain('text-sm');
  });

  it('keeps the disclaimer as its own text-secondary paragraph', () => {
    render(AboutPanel);
    const p = screen.getByText(DISCLAIMER_TEXT);
    expect(p.tagName).toBe('P');
    expect(p.className).toContain('text-secondary');
  });
});

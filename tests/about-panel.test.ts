import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { DISCLAIMER_TEXT, REPO_URL } from '$lib/settings/disclaimer';
import pkg from '../package.json' with { type: 'json' };

const openUrlMock = vi.fn().mockResolvedValue(undefined);
vi.mock('@tauri-apps/plugin-opener', () => ({
  openUrl: (url: string) => openUrlMock(url),
}));

const h = vi.hoisted(() => ({
  buildInfo: vi.fn(),
  clipboard: vi.fn(),
  success: vi.fn(),
  warning: vi.fn(),
}));
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    appBuildInfo: () => h.buildInfo(),
    clipboardWriteText: (text: string) => h.clipboard(text),
  },
}));
vi.mock('$lib/toasts/toasts.svelte', async (orig) => ({
  ...(await orig<object>()),
  pushSuccess: (title: string) => h.success(title),
  pushWarning: (title: string) => h.warning(title),
}));

const RC = {
  version: '0.25.0',
  build: { kind: 'tagged', tag: 'v0.25.0-rc.2' },
  commit: 'a1b2c3d',
  fork: null,
  os: 'windows',
  arch: 'x86_64',
};
const RC_LINE = 'v0.25.0-rc.2 · a1b2c3d · windows x86_64';

beforeEach(() => {
  vi.clearAllMocks();
  h.buildInfo.mockResolvedValue(RC);
  h.clipboard.mockResolvedValue({ status: 'ok', data: null });
});

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

  it('shows which build this is under the version (ABOUT-03)', async () => {
    render(AboutPanel);
    expect(await screen.findByText(RC_LINE)).toBeTruthy();
  });

  it('copies a paste-ready version block and says so', async () => {
    render(AboutPanel);
    await screen.findByText(RC_LINE);
    await fireEvent.click(screen.getByRole('button', { name: 'Copy version info' }));
    await waitFor(() =>
      expect(h.clipboard).toHaveBeenCalledWith(
        'Lucerna 0.25.0\nBuild: v0.25.0-rc.2 (a1b2c3d)\nOS: windows x86_64',
      ),
    );
    await waitFor(() => expect(h.success).toHaveBeenCalledWith('Version info copied'));
    expect(h.warning).not.toHaveBeenCalled();
  });

  it('says so when the clipboard refused, and claims nothing', async () => {
    h.clipboard.mockResolvedValue({
      status: 'error',
      error: { kind: 'io', path: '<clipboard>', details: 'denied' },
    });
    render(AboutPanel);
    await screen.findByText(RC_LINE);
    await fireEvent.click(screen.getByRole('button', { name: 'Copy version info' }));
    await waitFor(() => expect(h.warning).toHaveBeenCalledWith("Couldn't copy the version info"));
    expect(h.success).not.toHaveBeenCalled();
  });

  it('still copies, saying unknown, when the build info could not be read', async () => {
    h.buildInfo.mockRejectedValue(new Error('ipc down'));
    render(AboutPanel);
    await fireEvent.click(screen.getByRole('button', { name: 'Copy version info' }));
    await waitFor(() =>
      expect(h.clipboard).toHaveBeenCalledWith(
        `Lucerna ${pkg.version}\nBuild: unknown\nOS: unknown`,
      ),
    );
  });

  it('says there is no telemetry and links the privacy policy (ABOUT-02)', async () => {
    render(AboutPanel);
    expect(screen.getByText(/sends no telemetry and has no servers of its own/)).toBeTruthy();
    await fireEvent.click(screen.getByRole('button', { name: 'Privacy policy' }));
    await vi.waitFor(() =>
      expect(openUrlMock).toHaveBeenCalledWith(`${REPO_URL}/blob/main/PRIVACY.md`),
    );
  });

  it('states the licence with its copyright holder and no warranty (ABOUT-05)', () => {
    render(AboutPanel);
    const licence = screen.getByText(/GPL-3\.0-or-later/);
    expect(licence.textContent).toContain('Copyright © 2026 Anton Babchenko');
    expect(licence.textContent).toContain('comes with no warranty');
    // One fact per line (ABOUT-07): the Mojang download is not in this line.
    expect(licence.textContent).not.toContain('Mojang');
  });

  it('opens the licence text from the licence line', async () => {
    render(AboutPanel);
    await fireEvent.click(screen.getByRole('button', { name: 'Read the licence' }));
    await vi.waitFor(() =>
      expect(openUrlMock).toHaveBeenCalledWith(`${REPO_URL}/blob/main/LICENSE`),
    );
  });

  it('scopes "never modified" to what Lucerna downloads, and names the mod loaders (Q3)', () => {
    render(AboutPanel);
    const line = screen.getByText(/downloaded from Mojang when they are first needed/);
    expect(line.textContent).toContain(
      'The files Lucerna downloads from Mojang are never modified',
    );
    expect(line.textContent).toContain('such as Forge');
    expect(line.textContent).not.toContain('at runtime');
  });

  it('names the product in the trademark line instead of "This launcher"', () => {
    render(AboutPanel);
    const mark = screen.getByText(/Minecraft and Mojang are trademarks/);
    expect(mark.textContent).toContain('Lucerna is not affiliated');
    expect(mark.textContent).toContain('Mojang AB and Microsoft Corporation');
    expect(mark.textContent).not.toContain('Synergies');
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
    expect(h3.map((el) => el.textContent?.trim())).toEqual(['Lucerna', 'Your data', 'Legal']);
    for (const el of h3) expect(el.className).toContain('text-sm');
  });

  it('keeps the disclaimer as its own text-secondary paragraph', () => {
    render(AboutPanel);
    const p = screen.getByText(DISCLAIMER_TEXT);
    expect(p.tagName).toBe('P');
    expect(p.className).toContain('text-secondary');
  });
});

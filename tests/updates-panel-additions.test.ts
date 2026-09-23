// Settings → Updates remembers what the startup check found, says which version
// you have, links the offered version's notes, and makes skipping a version an
// explicit, reversible choice (UPD-03, UPD-04, UPD-08).
import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import pkg from '../package.json' with { type: 'json' };

const openUrlMock = vi.fn().mockResolvedValue(undefined);
vi.mock('@tauri-apps/plugin-opener', () => ({
  openUrl: (url: string) => openUrlMock(url),
}));

const h = vi.hoisted(() => ({
  updateCheck: vi.fn(),
  updateDismiss: vi.fn(),
  updateClearDismissed: vi.fn(),
  updateSkippedVersion: vi.fn(),
}));
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    appSettingsGet: vi.fn(async () => ({
      status: 'ok',
      data: { general: { check_updates_on_startup: true } },
    })),
    appSettingsPatchGeneral: vi.fn(),
    updateCheck: () => h.updateCheck(),
    updateDismiss: (v: string) => h.updateDismiss(v),
    updateClearDismissed: () => h.updateClearDismissed(),
    updateSkippedVersion: () => h.updateSkippedVersion(),
    restartBlocked: vi.fn().mockResolvedValue('none'),
  },
}));

import type { UpdateInfo } from '$lib/ipc/bindings';
import { __resetAppSettingsForTest, loadAppSettings } from '$lib/settings/app-settings.svelte';
import UpdatesPanel from '$lib/settings/UpdatesPanel.svelte';
import { updateState } from '$lib/update/state.svelte';

const RELEASE = 'https://github.com/AntonBabchenko/Lucerna/releases/tag/v0.25.0';
const OFFER = {
  current: '0.24.0',
  latest: '0.25.0',
  available: true,
  release_url: RELEASE,
  installer: { name: 'Lucerna_0.25.0_x64-setup.exe', url: 'https://example.invalid/i', size: 1 },
  sha256sums: null,
  cosign_bundle: null,
} as unknown as UpdateInfo;

beforeEach(async () => {
  vi.clearAllMocks();
  updateState.value = null;
  h.updateCheck.mockResolvedValue({ status: 'ok', data: { ...OFFER } });
  h.updateDismiss.mockResolvedValue({ status: 'ok', data: '0.25.0' });
  h.updateClearDismissed.mockResolvedValue({ status: 'ok', data: null });
  h.updateSkippedVersion.mockResolvedValue({ status: 'ok', data: null });
  __resetAppSettingsForTest();
  await loadAppSettings();
});

describe('what the startup check found', () => {
  it('is on the page the moment it opens — no second check', async () => {
    updateState.value = { ...OFFER };
    render(UpdatesPanel);
    expect(await screen.findByText('Version 0.25.0 is available.')).toBeTruthy();
    expect(h.updateCheck).not.toHaveBeenCalled();
  });

  it('is forgotten when a check finds nothing newer', async () => {
    updateState.value = { ...OFFER };
    h.updateCheck.mockResolvedValue({
      status: 'ok',
      data: { ...OFFER, available: false, latest: '0.24.0' },
    });
    render(UpdatesPanel);
    await fireEvent.click(screen.getByTestId('check-updates-btn'));
    await waitFor(() => expect(updateState.value).toBeNull());
  });
});

describe('an offered update', () => {
  it('says which version you have and links its release notes', async () => {
    render(UpdatesPanel);
    await fireEvent.click(screen.getByTestId('check-updates-btn'));
    expect(await screen.findByText('You have 0.24.0.')).toBeTruthy();
    await fireEvent.click(screen.getByRole('button', { name: 'Release notes' }));
    await vi.waitFor(() => expect(openUrlMock).toHaveBeenCalledWith(RELEASE));
  });

  it('says what is verified before an in-app install', async () => {
    render(UpdatesPanel);
    await fireEvent.click(screen.getByTestId('check-updates-btn'));
    expect(await screen.findByText(/signature from Lucerna's release workflow/)).toBeTruthy();
  });

  it('does not claim a verification it will not run on a notify-only build', async () => {
    h.updateCheck.mockResolvedValue({ status: 'ok', data: { ...OFFER, installer: null } });
    render(UpdatesPanel);
    await fireEvent.click(screen.getByTestId('check-updates-btn'));
    await screen.findByText('Version 0.25.0 is available.');
    expect(screen.queryByText(/signature from Lucerna's release workflow/)).toBeNull();
  });
});

describe('skipping a version', () => {
  it('is an explicit button that asks the backend and then says so', async () => {
    render(UpdatesPanel);
    await fireEvent.click(screen.getByTestId('check-updates-btn'));
    await fireEvent.click(await screen.findByRole('button', { name: 'Skip this version' }));
    await waitFor(() => expect(h.updateDismiss).toHaveBeenCalledWith('0.25.0'));
    expect(
      await screen.findByText("You skipped 0.25.0. Lucerna won't mention it at startup."),
    ).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Skip this version' })).toBeNull();
  });

  it('says why when the skip could not be saved', async () => {
    h.updateDismiss.mockResolvedValue({
      status: 'error',
      error: { kind: 'io', path: '<app_file>', details: 'read-only' },
    });
    render(UpdatesPanel);
    await fireEvent.click(screen.getByTestId('check-updates-btn'));
    await fireEvent.click(await screen.findByRole('button', { name: 'Skip this version' }));
    expect(await screen.findByText(/Couldn't skip this version/)).toBeTruthy();
    expect(screen.queryByText(/You skipped/)).toBeNull();
  });

  it('shows a skip the backend reports, and Stop skipping undoes it', async () => {
    h.updateSkippedVersion.mockResolvedValue({ status: 'ok', data: '0.25.0' });
    render(UpdatesPanel);
    expect(await screen.findByText(/You skipped 0.25.0/)).toBeTruthy();
    await fireEvent.click(screen.getByRole('button', { name: 'Stop skipping' }));
    await waitFor(() => expect(h.updateClearDismissed).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(screen.queryByText(/You skipped/)).toBeNull());
  });
});

describe('the changelog on this page', () => {
  it('says it is the history up to the version you have', () => {
    render(UpdatesPanel);
    expect(screen.getByText(`Changes up to the version you have (${pkg.version}).`)).toBeTruthy();
  });
});

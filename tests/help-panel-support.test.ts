// Settings → Help is also the support page (HELP-01): report a bug, reach the
// launcher's own log folder, and know where a security problem goes.
import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, test, vi } from 'vitest';
import { REPO_URL } from '../src/lib/settings/disclaimer';
import { describedText } from './test-utils/aria';

const openUrlMock = vi.fn().mockResolvedValue(undefined);
vi.mock('@tauri-apps/plugin-opener', () => ({
  openUrl: (url: string) => openUrlMock(url),
}));

const h = vi.hoisted(() => ({ openLogs: vi.fn() }));
vi.mock('$lib/ipc/bindings', async (orig) => {
  const real = await orig<typeof import('$lib/ipc/bindings')>();
  return {
    ...real,
    commands: { ...real.commands, openLauncherLogFolder: () => h.openLogs() },
  };
});

import HelpPanel from '../src/lib/settings/HelpPanel.svelte';

beforeEach(() => {
  vi.clearAllMocks();
  h.openLogs.mockResolvedValue({ status: 'ok', data: null });
});

describe('Help as the support page', () => {
  test('has a "Report a problem" block', () => {
    render(HelpPanel);
    expect(screen.getByRole('heading', { level: 3, name: 'Report a problem' })).toBeTruthy();
  });

  test('"Report a bug" opens GitHub\'s bug form', async () => {
    render(HelpPanel);
    await fireEvent.click(screen.getByRole('button', { name: 'Report a bug' }));
    await vi.waitFor(() =>
      expect(openUrlMock).toHaveBeenCalledWith(`${REPO_URL}/issues/new?template=bug_report.md`),
    );
  });

  test('the bug link says what to include, and warns that logs carry the user name', () => {
    render(HelpPanel);
    const hint = describedText(screen.getByRole('button', { name: 'Report a bug' }));
    expect(hint).toContain('version info from About');
    expect(hint).toContain('user name');
  });

  test('opens the launcher log folder', async () => {
    render(HelpPanel);
    await fireEvent.click(screen.getByRole('button', { name: 'Open launcher log folder' }));
    await waitFor(() => expect(h.openLogs).toHaveBeenCalledTimes(1));
    expect(screen.queryByText(/Couldn't open the log folder/)).toBeNull();
  });

  test('says why when the log folder could not be opened', async () => {
    h.openLogs.mockResolvedValue({
      status: 'error',
      error: { kind: 'io', path: '<app_logs>', details: 'no opener' },
    });
    render(HelpPanel);
    await fireEvent.click(screen.getByRole('button', { name: 'Open launcher log folder' }));
    expect(await screen.findByText(/Couldn't open the log folder/)).toBeTruthy();
  });

  test('sends security problems to the private policy, not a public issue', async () => {
    render(HelpPanel);
    expect(screen.getByText(/Report it privately, not in a public issue/)).toBeTruthy();
    await fireEvent.click(screen.getByRole('button', { name: 'Security policy' }));
    await vi.waitFor(() => expect(openUrlMock).toHaveBeenCalledWith(`${REPO_URL}/security/policy`));
  });
});

import { beforeEach, describe, expect, it, vi } from 'vitest';

const openUrlMock = vi.fn().mockResolvedValue(undefined);
vi.mock('@tauri-apps/plugin-opener', () => ({
  openUrl: (url: string) => openUrlMock(url),
}));

// The helper pushes exactly these two. Spec L10: any suite that partial-mocks
// the toast store AND drives a refused or failed open must export both.
const toasts = vi.hoisted(() => ({ pushInfo: vi.fn(), pushWarning: vi.fn() }));
vi.mock('$lib/toasts/toasts.svelte', () => toasts);

import { openExternalHttps } from '$lib/ui/safe-open';

describe('openExternalHttps', () => {
  beforeEach(() => {
    openUrlMock.mockReset().mockResolvedValue(undefined); // module-level mock accumulates across tests
    toasts.pushInfo.mockClear();
    toasts.pushWarning.mockClear();
  });

  it('hands an https URL to tauri-plugin-opener', async () => {
    await openExternalHttps('https://modrinth.com/mod/sodium');
    expect(openUrlMock).toHaveBeenCalledWith('https://modrinth.com/mod/sodium');
    expect(toasts.pushInfo).not.toHaveBeenCalled();
    expect(toasts.pushWarning).not.toHaveBeenCalled();
  });

  it.each([
    'javascript:alert(1)',
    'file:///C:/Windows/System32/calc.exe',
    'vbscript:msgbox(1)',
    'ms-msdt:/id PCWDiagnostic', // custom protocol handler
    'mailto:victim@example.com?subject=hi',
    'tel:+1900PREMIUM',
    'http://example.com/cleartext',
    'HTTPS://case-variant.example', // stricter than the OS on purpose
    '',
  ])('never hands %s to the opener', async (url) => {
    await openExternalHttps(url);
    expect(openUrlMock).not.toHaveBeenCalled();
  });

  it('says a non-https URL was refused, with the URL as the copyable line', async () => {
    await openExternalHttps('http://example.com/cleartext');
    expect(openUrlMock).not.toHaveBeenCalled();
    expect(toasts.pushInfo).toHaveBeenCalledWith(
      'Only https:// links can be opened from Lucerna.',
      ['http://example.com/cleartext'],
    );
    expect(toasts.pushWarning).not.toHaveBeenCalled();
  });

  it('an empty URL is a no-op: nothing opens, nothing is said', async () => {
    await openExternalHttps('');
    expect(openUrlMock).not.toHaveBeenCalled();
    expect(toasts.pushInfo).not.toHaveBeenCalled();
    expect(toasts.pushWarning).not.toHaveBeenCalled();
  });

  it('reports a rejected openUrl as a warning toast carrying the reason', async () => {
    openUrlMock.mockRejectedValueOnce(new Error('xdg-open not found'));
    await expect(openExternalHttps('https://modrinth.com/mod/sodium')).resolves.toBeUndefined();
    expect(toasts.pushWarning).toHaveBeenCalledWith("Couldn't open the link in your browser.", [
      'xdg-open not found',
    ]);
    expect(toasts.pushInfo).not.toHaveBeenCalled();
  });

  it('resolves, never throws, when refusing', async () => {
    await expect(openExternalHttps('file:///etc/passwd')).resolves.toBeUndefined();
  });
});

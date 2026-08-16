import { beforeEach, describe, expect, it, vi } from 'vitest';

const openUrlMock = vi.fn().mockResolvedValue(undefined);
vi.mock('@tauri-apps/plugin-opener', () => ({
  openUrl: (url: string) => openUrlMock(url),
}));

import { openExternalHttps } from '$lib/ui/safe-open';

describe('openExternalHttps', () => {
  beforeEach(() => {
    openUrlMock.mockClear(); // module-level mock accumulates across tests
  });

  it('hands an https URL to tauri-plugin-opener', async () => {
    await openExternalHttps('https://modrinth.com/mod/sodium');
    expect(openUrlMock).toHaveBeenCalledWith('https://modrinth.com/mod/sodium');
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

  it('resolves rather than throwing when refusing (silent no-op contract)', async () => {
    await expect(openExternalHttps('file:///etc/passwd')).resolves.toBeUndefined();
  });
});

import { beforeEach, describe, expect, it, type Mock, vi } from 'vitest';

// The one contract every instant-apply setting goes through: unknown until
// loaded; optimistic on top of the confirmed block; rolled back only where
// still displayed; every failure named per field; one writer at a time.
vi.mock('$lib/ipc/bindings', () => ({
  commands: { appSettingsGet: vi.fn(), appSettingsPatchGeneral: vi.fn() },
}));

import { commands } from '$lib/ipc/bindings';
import {
  __resetAppSettingsForTest,
  appSettings,
  generalDisplayed,
  loadAppSettings,
  patchGeneral,
  saveFailure,
  saveFailureKind,
} from '$lib/settings/app-settings.svelte';

const FILE = {
  general: {
    theme: 'system',
    language: 'system',
    hidden_sidebar_buttons: [],
    log_retention: { enabled: false, max_files: 10, max_total_mb: 100 },
  },
  onboarding: {},
};
const get = commands.appSettingsGet as Mock;
const patch = commands.appSettingsPatchGeneral as Mock;
const flush = () => new Promise((r) => setTimeout(r, 0));
const IO_ERR = { status: 'error', error: { kind: 'io', path: 'app.json', details: 'locked' } };

beforeEach(() => {
  vi.clearAllMocks();
  __resetAppSettingsForTest();
  get.mockResolvedValue({ status: 'ok', data: FILE });
  patch.mockImplementation(async (p: object) => ({
    status: 'ok',
    data: { ...FILE.general, ...p },
  }));
});

describe('app-settings store', () => {
  it('is unknown until loaded, then confirmed', async () => {
    expect(generalDisplayed()).toBeNull();
    await loadAppSettings();
    expect(appSettings.loaded.kind).toBe('ok');
    expect(generalDisplayed()?.theme).toBe('system');
  });

  it('a patch shows at once, and the confirmed block becomes what the backend returned', async () => {
    await loadAppSettings();
    patch.mockResolvedValue({
      status: 'ok',
      data: { ...FILE.general, theme: 'dark', language: 'ru' },
    });
    const p = patchGeneral({ theme: 'dark' });
    expect(generalDisplayed()?.theme).toBe('dark'); // optimistic, synchronously
    expect(await p).toEqual({ ok: true });
    expect(generalDisplayed()?.language).toBe('ru'); // another writer's change, now confirmed
    expect(patch).toHaveBeenCalledWith({ theme: 'dark' });
  });

  it('a refused patch drops itself, names the failure, and leaves the rest', async () => {
    await loadAppSettings();
    patch.mockResolvedValue(IO_ERR);
    const r = await patchGeneral({ hidden_sidebar_buttons: ['x'] });
    expect(r).toMatchObject({ ok: false, kind: 'refused' });
    expect(generalDisplayed()?.hidden_sidebar_buttons).toEqual([]);
    expect(saveFailureKind('hidden_sidebar_buttons')).toBe('refused');
    expect(saveFailure('hidden_sidebar_buttons')).toContain("wasn't saved");
    expect(saveFailure('theme')).toBeNull();
  });

  it('a newer patch to the same field survives an older one failing', async () => {
    await loadAppSettings();
    let failFirst!: (v: unknown) => void;
    patch
      .mockReturnValueOnce(new Promise((r) => (failFirst = r)))
      .mockResolvedValueOnce({ status: 'ok', data: { ...FILE.general, theme: 'light' } });
    const first = patchGeneral({ theme: 'dark' });
    const second = patchGeneral({ theme: 'light' });
    expect(generalDisplayed()?.theme).toBe('light');
    failFirst(IO_ERR);
    await first;
    expect(generalDisplayed()?.theme).toBe('light'); // never snapped back to system
    await second;
    expect(generalDisplayed()?.theme).toBe('light');
    expect(saveFailure('theme')).toBeNull(); // the later success cleared it
  });

  it('sends patches one at a time, in order', async () => {
    await loadAppSettings();
    let releaseFirst!: (v: unknown) => void;
    patch.mockReturnValueOnce(new Promise((r) => (releaseFirst = r)));
    void patchGeneral({ theme: 'dark' });
    void patchGeneral({ language: 'ru' });
    await flush();
    expect(patch).toHaveBeenCalledTimes(1);
    releaseFirst({ status: 'ok', data: { ...FILE.general, theme: 'dark' } });
    await flush();
    expect(patch).toHaveBeenCalledTimes(2);
    expect(patch.mock.calls[1][0]).toEqual({ language: 'ru' });
  });

  it('a rejected call is "unconfirmed": the patch is dropped and the block re-read', async () => {
    await loadAppSettings();
    patch.mockRejectedValue(new Error('ipc down'));
    get.mockResolvedValue({
      status: 'ok',
      data: { ...FILE, general: { ...FILE.general, theme: 'dark' } },
    });
    const r = await patchGeneral({ theme: 'dark' });
    expect(r).toMatchObject({ ok: false, kind: 'unconfirmed' });
    expect(saveFailure('theme')).toContain("Couldn't confirm");
    await flush();
    expect(generalDisplayed()?.theme).toBe('dark'); // the re-read decided
  });

  it('a patch issued while the load is pending waits behind it', async () => {
    let releaseLoad!: (v: unknown) => void;
    get.mockReturnValueOnce(new Promise((r) => (releaseLoad = r)));
    const load = loadAppSettings();
    const p = patchGeneral({ theme: 'dark' });
    await flush();
    expect(patch).not.toHaveBeenCalled();
    releaseLoad({ status: 'ok', data: FILE });
    await load;
    expect(await p).toEqual({ ok: true });
  });

  it('refuses without a call when the load failed', async () => {
    get.mockResolvedValue(IO_ERR);
    await loadAppSettings();
    expect(appSettings.loaded.kind).toBe('failed');
    expect(await patchGeneral({ theme: 'dark' })).toMatchObject({ ok: false, kind: 'not_loaded' });
    expect(patch).not.toHaveBeenCalled();
  });
});

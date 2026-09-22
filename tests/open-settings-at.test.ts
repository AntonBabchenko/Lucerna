import { get } from 'svelte/store';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const h = vi.hoisted(() => ({ restartLauncher: vi.fn() }));
vi.mock('$lib/ipc/bindings', () => ({ commands: { restartLauncher: h.restartLauncher } }));

import { t } from '$lib/i18n';
import { restartLauncherOrExplain } from '$lib/settings/restart';
import { openSettingsAt, settingsOpen, settingsSearchFocus } from '$lib/settings/state.svelte';

const tr = get(t);

beforeEach(() => {
  vi.clearAllMocks();
  settingsOpen.value = null;
  settingsSearchFocus.value = null;
});

describe('openSettingsAt', () => {
  it('opens the modal at the section that owns the anchor, then points at the anchor', async () => {
    await openSettingsAt('storage.dataLocation');
    expect(settingsOpen.value).toEqual({ tab: 'storage' });
    expect(settingsSearchFocus.value).toBe('storage.dataLocation');
  });

  it('clears a stale anchor first, so the null → anchor edge fires again', async () => {
    // HelpPanel closes the modal without clearing the rune: the same anchor may still be set, and
    // a SettingsField only flashes on the null → anchor edge.
    settingsSearchFocus.value = 'storage.dataLocation';
    const done = openSettingsAt('storage.dataLocation');
    // Synchronously after the call: cleared, and the modal already told which section to show.
    expect(settingsSearchFocus.value).toBeNull();
    expect(settingsOpen.value).toEqual({ tab: 'storage' });
    await done;
    expect(settingsSearchFocus.value).toBe('storage.dataLocation');
  });
});

describe('restartLauncherOrExplain', () => {
  it('returns nothing to show when the restart goes through', async () => {
    // On success the real command never returns; a mock that does must not read as a failure.
    h.restartLauncher.mockResolvedValue({ status: 'ok', data: null });
    expect(await restartLauncherOrExplain(tr)).toBeNull();
  });

  it('explains a refusal with the way out', async () => {
    h.restartLauncher.mockResolvedValue({ status: 'error', error: { kind: 'data_location_busy' } });
    const text = await restartLauncherOrExplain(tr);
    expect(text).toContain("Couldn't restart");
    expect(text).toContain('Close Lucerna and open it again');
  });

  it('explains a transport failure too — typedError rethrows real Error instances', async () => {
    h.restartLauncher.mockRejectedValue(new Error('ipc channel closed'));
    const text = await restartLauncherOrExplain(tr);
    expect(text).toContain('ipc channel closed');
    expect(text).toContain('Close Lucerna and open it again');
  });
});

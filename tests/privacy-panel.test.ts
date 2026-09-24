// Settings → Privacy & network: one place that says what Lucerna contacts and
// what the user has allowed — the state of each channel, never a default dressed
// as a fact, and a way to the setting itself. The toggles stay where they are.
import { fireEvent, render, screen, within } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const h = vi.hoisted(() => ({
  get: vi.fn(),
  getDataLocation: vi.fn(),
  jump: vi.fn(),
  openUrl: vi.fn().mockResolvedValue(undefined),
}));
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    appSettingsGet: () => h.get(),
    appSettingsPatchGeneral: vi.fn(),
    getDataLocation: () => h.getDataLocation(),
  },
  events: { dataMigrationProgress: { listen: vi.fn().mockResolvedValue(() => {}) } },
}));
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: (u: string) => h.openUrl(u) }));
vi.mock('$lib/settings/state.svelte', async (orig) => ({
  ...(await orig<object>()),
  jumpInSettings: (a: string) => h.jump(a),
}));

import { __resetAppSettingsForTest, loadAppSettings } from '$lib/settings/app-settings.svelte';
import { dataLocation } from '$lib/settings/data-location.svelte';
import { PRIVACY_POLICY_URL } from '$lib/settings/disclaimer';
import PrivacyPanel from '$lib/settings/PrivacyPanel.svelte';

const GENERAL = {
  allow_server_ping: true,
  allow_ai_translation: false,
  ai_provider: 'gemini',
  check_updates_on_startup: true,
};
const status = (fellBack: boolean) => ({
  status: 'ok',
  data: {
    effective: '/data',
    configured: fellBack ? 'D:\\Data' : null,
    fell_back: fellBack,
    ...(fellBack ? { fallback: { kind: 'root_missing' } } : {}),
    default_dir: '/default',
    relocation: { kind: 'idle' },
  },
});
const row = (id: string) => screen.getByTestId(`privacy-row-${id}`);

async function load(general: Record<string, unknown> = GENERAL, fellBack = false) {
  h.get.mockResolvedValue({ status: 'ok', data: { general } });
  h.getDataLocation.mockResolvedValue(status(fellBack));
  __resetAppSettingsForTest();
  await loadAppSettings();
  await dataLocation.refresh();
}

beforeEach(() => {
  vi.clearAllMocks();
});

describe('Privacy & network', () => {
  it('says, per channel, whether it is allowed', async () => {
    await load();
    render(PrivacyPanel);
    expect(row('serverPing').textContent).toContain('Allowed');
    expect(row('ai').textContent).toContain('Not allowed');
    expect(row('updates').textContent).toContain('On');
    expect(row('serverPing').textContent).toContain('sees your IP address');
  });

  it('says a local AI sends nothing out, and names where a hosted one sends the text', async () => {
    await load({ ...GENERAL, allow_ai_translation: true, ai_provider: 'local' });
    const first = render(PrivacyPanel);
    expect(row('ai').textContent).toContain('Runs on this computer');
    first.unmount();
    await load({ ...GENERAL, allow_ai_translation: true, ai_provider: 'gemini' });
    render(PrivacyPanel);
    expect(row('ai').textContent).toContain('to Google Gemini');
  });

  it('claims nothing while the settings are being read', async () => {
    h.get.mockReturnValue(new Promise(() => {}));
    h.getDataLocation.mockResolvedValue(status(false));
    __resetAppSettingsForTest();
    void loadAppSettings();
    render(PrivacyPanel);
    for (const id of ['serverPing', 'ai', 'updates']) {
      expect(row(id).textContent).toContain('Checking…');
      expect(row(id).textContent).not.toMatch(/Allowed|Not allowed|\bOn\b|\bOff\b/);
    }
    // Where the text would go depends on a provider nobody has read yet.
    expect(row('ai').textContent).not.toContain('Runs on this computer');
    expect(row('ai').textContent).not.toContain('Gemini');
  });

  it("says when the settings couldn't be read", async () => {
    h.get.mockResolvedValue({
      status: 'error',
      error: { kind: 'io', path: 'app.json', details: 'x' },
    });
    h.getDataLocation.mockResolvedValue(status(false));
    __resetAppSettingsForTest();
    await loadAppSettings();
    render(PrivacyPanel);
    expect(row('ai').textContent).toContain("Couldn't read");
  });

  it('says the update check is paused in a temporary session', async () => {
    await load(GENERAL, true);
    render(PrivacyPanel);
    expect(row('updates').textContent).toContain('Paused in this temporary session');
  });

  it('says nothing about pausing a check that is off', async () => {
    await load({ ...GENERAL, check_updates_on_startup: false }, true);
    render(PrivacyPanel);
    expect(row('updates').textContent).toContain('Off');
    expect(row('updates').textContent).not.toContain('Paused');
  });

  it('each Change goes to the setting itself', async () => {
    await load();
    render(PrivacyPanel);
    await fireEvent.click(within(row('serverPing')).getByRole('button', { name: 'Change' }));
    await fireEvent.click(within(row('ai')).getByRole('button', { name: 'Change' }));
    await fireEvent.click(within(row('updates')).getByRole('button', { name: 'Change' }));
    expect(h.jump.mock.calls.map((c) => c[0])).toEqual([
      'game.serverPing',
      'integrations.aiTranslation',
      'updates.startupCheck',
    ]);
  });

  it('points to the privacy policy for everything else', async () => {
    await load();
    render(PrivacyPanel);
    expect(screen.getByText(/including Mojang, Microsoft, Modrinth and CurseForge/)).toBeTruthy();
    await fireEvent.click(screen.getByRole('button', { name: 'Privacy policy' }));
    await vi.waitFor(() => expect(h.openUrl).toHaveBeenCalledWith(PRIVACY_POLICY_URL));
  });
});

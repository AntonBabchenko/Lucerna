import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import { commands, type ServerWithStatus } from '$lib/ipc/bindings';
import ServerJavaField from '$lib/servers/settings/ServerJavaField.svelte';
import { __resetAppSettingsForTest, appSettings } from '$lib/settings/app-settings.svelte';

// The Java an own server runs on (spec 2026-10-11 custom-java §6.2): the same
// field as an instance's, judged against the server's Java version and saved
// to server.json.

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    javaDetect: vi.fn(),
    javaCheck: vi.fn(),
    serverSetJava: vi.fn(),
  },
  events: {
    serverLogLine: { listen: vi.fn().mockResolvedValue(() => {}) },
    serverSpawned: { listen: vi.fn().mockResolvedValue(() => {}) },
    serverExited: { listen: vi.fn().mockResolvedValue(() => {}) },
    serverUploadProgress: { listen: vi.fn().mockResolvedValue(() => {}) },
  },
}));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));

const found = (major: number, version: string, dir: string) => ({
  path: `/opt/${dir}/bin/java`,
  home: `/opt/${dir}`,
  major,
  version,
  vendor: null,
  arch: 'amd64',
  source: 'known_dir' as const,
});
const JDK21 = found(21, '21.0.4', 'jdk-21');
const JDK8 = found(8, '1.8.0_402', 'jdk8');

const server = (over: Partial<ServerWithStatus> = {}) =>
  ({ id: 'srv-1', mc_version: '1.21.1', java: null, java_major: 21, ...over }) as ServerWithStatus;

function setup(s: ServerWithStatus) {
  vi.mocked(commands.javaDetect).mockResolvedValue({ status: 'ok', data: [JDK21, JDK8] });
  render(ServerJavaField, { server: s });
}

beforeAll(() => locale.set('en'));
afterEach(() => {
  vi.clearAllMocks();
  __resetAppSettingsForTest();
});

describe('ServerJavaField', () => {
  it('picking a found Java saves it for the server', async () => {
    setup(server());
    vi.mocked(commands.serverSetJava).mockResolvedValue({
      status: 'ok',
      data: {
        server: server({ java: { kind: 'custom', path: JDK21.path } }),
        report: null,
      },
    } as never);
    await fireEvent.click(await screen.findByRole('combobox', { name: 'Java' }));
    await fireEvent.mouseDown(await screen.findByRole('option', { name: /21\.0\.4/ }));
    await waitFor(() =>
      expect(commands.serverSetJava).toHaveBeenCalledWith('srv-1', {
        kind: 'custom',
        path: JDK21.path,
      }),
    );
  });

  it("judges a Java against the server's Java version", async () => {
    setup(server());
    await fireEvent.click(await screen.findByRole('combobox', { name: 'Java' }));
    const old = await screen.findByRole('option', { name: /too old/ });
    expect(old.textContent).toMatch(/1\.8\.0_402/);
    expect(old.getAttribute('aria-disabled')).toBe('true');
  });

  it("Default names the Settings row for the server's Java version", async () => {
    appSettings.loaded = {
      kind: 'ok',
      file: { general: { java_defaults: [{ major: 21, path: JDK21.path }] } },
    } as never;
    setup(server());
    await fireEvent.click(await screen.findByRole('combobox', { name: 'Java' }));
    expect(
      await screen.findByRole('option', { name: /Default \(now: Settings' Java 21\)/ }),
    ).toBeTruthy();
  });
});

import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import { commands } from '$lib/ipc/bindings';
import JavaDefaultsSection from '$lib/java/JavaDefaultsSection.svelte';
import {
  __resetAppSettingsForTest,
  appSettings,
  generalDisplayed,
} from '$lib/settings/app-settings.svelte';

// Settings → Game → Java (spec 2026-10-11 custom-java §6.2): a row per Java
// version the instances need or Settings already names, each saying which Java
// every instance on that version runs on unless it chose its own.

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    javaRequiredMajors: vi.fn(),
    javaDetect: vi.fn(),
    javaCheck: vi.fn(),
    setJavaDefault: vi.fn(),
  },
}));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));

const JDK21 = {
  path: '/opt/jdk-21/bin/java',
  home: '/opt/jdk-21',
  major: 21,
  version: '21.0.4',
  vendor: null,
  arch: 'amd64',
  source: 'known_dir' as const,
};
const report = (path: string, major: number) => ({
  path,
  version: `${major}.0.1`,
  major,
  vendor: null,
  arch: 'x64',
  verdict: { kind: 'matches' as const },
  gpu_choice_ignored: true,
});

function setup(needed: number[] | 'fails', saved: { major: number; path: string }[] = []) {
  appSettings.loaded = { kind: 'ok', file: { general: { java_defaults: saved } } } as never;
  if (needed === 'fails') {
    vi.mocked(commands.javaRequiredMajors).mockResolvedValue({
      status: 'error',
      error: { kind: 'io', path: 'versions', details: 'denied' },
    } as never);
  } else {
    vi.mocked(commands.javaRequiredMajors).mockResolvedValue({ status: 'ok', data: needed });
  }
  vi.mocked(commands.javaDetect).mockResolvedValue({ status: 'ok', data: [JDK21] });
  vi.mocked(commands.javaCheck).mockImplementation(async (path) => ({
    status: 'ok',
    data: report(path, 17),
  }));
  render(JavaDefaultsSection);
}

beforeAll(() => locale.set('en'));
afterEach(() => {
  vi.clearAllMocks();
  __resetAppSettingsForTest();
});

describe('JavaDefaultsSection', () => {
  it('has a row per Java the instances need and per saved row, newest first', async () => {
    setup([8, 21], [{ major: 17, path: '/opt/jdk-17/bin/java' }]);
    await screen.findByRole('combobox', { name: 'Java 21' });
    const rows = screen.getAllByRole('combobox').map((c) => c.getAttribute('aria-label'));
    expect(rows).toEqual(['Java 21', 'Java 17', 'Java 8']);
  });

  it('picking a found Java makes it the Java for that version', async () => {
    setup([21]);
    vi.mocked(commands.setJavaDefault).mockResolvedValue({
      status: 'ok',
      data: {
        settings: { java_defaults: [{ major: 21, path: JDK21.path }] } as never,
        report: report(JDK21.path, 21),
      },
    });
    await fireEvent.click(await screen.findByRole('combobox', { name: 'Java 21' }));
    await fireEvent.mouseDown(await screen.findByRole('option', { name: /21\.0\.4/ }));
    await waitFor(() => expect(commands.setJavaDefault).toHaveBeenCalledWith(21, JDK21.path));
    // The answer is adopted as the settings block, so every Default follows it.
    await waitFor(() =>
      expect(generalDisplayed()?.java_defaults).toEqual([{ major: 21, path: JDK21.path }]),
    );
  });

  it("choosing Lucerna's Java removes the row's Java", async () => {
    setup([], [{ major: 17, path: '/opt/jdk-17/bin/java' }]);
    vi.mocked(commands.setJavaDefault).mockResolvedValue({
      status: 'ok',
      data: { settings: { java_defaults: [] } as never, report: null },
    });
    await fireEvent.click(await screen.findByRole('combobox', { name: 'Java 17' }));
    await fireEvent.mouseDown(await screen.findByRole('option', { name: /^Lucerna's Java/ }));
    await waitFor(() => expect(commands.setJavaDefault).toHaveBeenCalledWith(17, null));
  });

  it('claims no row until Settings is read', async () => {
    vi.mocked(commands.javaRequiredMajors).mockResolvedValue({ status: 'ok', data: [21] });
    render(JavaDefaultsSection);
    await waitFor(() => expect(commands.javaRequiredMajors).toHaveBeenCalled());
    await new Promise((r) => setTimeout(r, 0));
    expect(screen.queryByRole('combobox')).toBeNull();
  });

  it('says so when no instance needs a Java yet', async () => {
    setup([]);
    expect(await screen.findByText(/No instance needs a Java yet/)).toBeTruthy();
  });

  it('a list it could not read is not read as "nothing needed"', async () => {
    setup('fails');
    expect(await screen.findByText(/Couldn't read which Java versions/)).toBeTruthy();
    expect(screen.queryByText(/No instance needs a Java yet/)).toBeNull();
  });
});

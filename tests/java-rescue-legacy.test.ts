import { fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import { commands } from '$lib/ipc/bindings';
import JavaRescuePanel, { type JavaProblem } from '$lib/java/JavaRescuePanel.svelte';
import { __resetAppSettingsForTest } from '$lib/settings/app-settings.svelte';

// The rescue offers only a Java that can run the version: for Java 8 (Minecraft 1.16 and older)
// only a Java 8 — Forge for those versions does not start on a newer one, as the Java field's
// own caution says — and the installer stage says what to do next.

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    javaDetect: vi.fn(),
    javaCheck: vi.fn(),
    setInstanceJava: vi.fn(),
    setJavaDefault: vi.fn(),
    ensureInstanceJre: vi.fn(),
  },
}));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));

import { open } from '@tauri-apps/plugin-dialog';

const found = (major: number | null, version: string | null, dir: string) => ({
  path: `/opt/${dir}/bin/java`,
  home: `/opt/${dir}`,
  major,
  version,
  vendor: null,
  arch: 'amd64',
  source: 'known_dir' as const,
});
const JDK21 = found(21, '21.0.4', 'jdk-21');
const JDK17 = found(17, '17.0.2', 'jdk-17');
const JDK8 = found(8, '1.8.0_402', 'jdk8');

const noRuntime = (
  major: number,
  stage: 'game_runtime' | 'installer_tools' = 'game_runtime',
): JavaProblem => ({
  kind: 'jre_unavailable',
  component: 'jre-legacy',
  major,
  stage,
  reason: { kind: 'unsupported_platform', os: 'linux', arch: 'aarch64' },
});

function setup(problem: JavaProblem, detected: unknown[]) {
  vi.mocked(commands.javaDetect).mockResolvedValue({ status: 'ok', data: detected as never });
  render(JavaRescuePanel, {
    problem,
    message: "Lucerna couldn't get Java 8 for this version.",
    instanceId: 'a',
    followsSettings: true,
    onJavaSaved: vi.fn(),
    onRuntimeReady: vi.fn(),
    onProblem: vi.fn(),
    onDismiss: vi.fn(),
  });
}

async function optionNames(): Promise<string[]> {
  await fireEvent.click(await screen.findByRole('combobox', { name: 'Java to use' }));
  return (await screen.findAllByRole('option')).map((o) => o.textContent ?? '');
}

beforeAll(() => locale.set('en'));
afterEach(() => {
  vi.clearAllMocks();
  __resetAppSettingsForTest();
});

describe('JavaRescuePanel — Java 8', () => {
  it('offers only a Java 8, and says «a Java 8», not «or newer»', async () => {
    setup(noRuntime(8), [JDK21, JDK17, JDK8]);
    expect(await screen.findByText(/run on a Java 8 from this computer/)).toBeTruthy();
    expect(screen.queryByText(/or newer/)).toBeNull();
    expect(await optionNames()).toEqual([expect.stringContaining('1.8.0_402')]);
  });

  it('with only newer Javas, offers none and no «every instance on Java 8»', async () => {
    setup(noRuntime(8), [JDK21, JDK17]);
    expect(await screen.findByText(/No Java 8 was found on this computer/)).toBeTruthy();
    expect(screen.queryByRole('button', { name: /every instance on Java 8/ })).toBeNull();
    expect(screen.queryByRole('button', { name: 'Use for this instance' })).toBeNull();
  });

  it('a file picked by Browse is not claimed to be a Java 8', async () => {
    vi.mocked(open).mockResolvedValue('/home/me/jdk-21/bin/java');
    setup(noRuntime(8), [JDK21]);
    expect(await screen.findByText(/No Java 8 was found on this computer/)).toBeTruthy();
    await fireEvent.click(screen.getByRole('button', { name: 'Browse…' }));
    expect(await screen.findByRole('button', { name: 'Use for this instance' })).toBeTruthy();
    expect(screen.getByText(/No Java 8 was found on this computer/)).toBeTruthy();
    expect(screen.queryByText(/run on a Java 8 from this computer/)).toBeNull();
    expect(screen.queryByRole('button', { name: /every instance on Java 8/ })).toBeNull();
  });

  it('a Java whose version is unknown is offered for Java 8 only if it says it is Java 8', async () => {
    const unknown = found(null, null, 'mystery');
    vi.mocked(commands.javaCheck).mockResolvedValue({
      status: 'ok',
      data: {
        path: unknown.path,
        version: '21.0.4',
        major: 21,
        vendor: null,
        arch: 'x64',
        verdict: { kind: 'newer', found: 21, required: 8 },
        gpu_choice_ignored: false,
      },
    } as never);
    setup(noRuntime(8), [unknown]);
    expect(await screen.findByText(/No Java 8 was found on this computer/)).toBeTruthy();
  });
});

describe('JavaRescuePanel — other Javas keep «or newer»', () => {
  it('Java 17 lists 17 first, then 21', async () => {
    setup(noRuntime(17), [JDK21, JDK17, JDK8]);
    expect(await screen.findByText(/run on a Java 17 or newer from this computer/)).toBeTruthy();
    const names = await optionNames();
    expect(names).toHaveLength(2);
    expect(names[0]).toContain('17.0.2');
    expect(names[1]).toContain('21.0.4');
  });
});

describe('JavaRescuePanel — the installer stage says what to do', () => {
  it('a failed download: install again once it can be downloaded', async () => {
    setup(
      {
        ...noRuntime(17, 'installer_tools'),
        reason: { kind: 'download', cause: { kind: 'io', path: 'x', details: 'y' } },
      } as JavaProblem,
      [],
    );
    expect(await screen.findByText(/Install again once it can be downloaded/)).toBeTruthy();
  });

  it('no runtime for this computer: which instances can still run', async () => {
    setup(noRuntime(17, 'installer_tools'), []);
    expect(
      await screen.findByText(/Fabric, Quilt and vanilla instances can run on a Java of your own/),
    ).toBeTruthy();
  });
});

import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import { commands } from '$lib/ipc/bindings';
import JavaRescuePanel, { type JavaProblem } from '$lib/java/JavaRescuePanel.svelte';
import { __resetAppSettingsForTest } from '$lib/settings/app-settings.svelte';

// When Lucerna's Java cannot be had (spec 2026-10-11 custom-java §7): a Java on
// this computer can stand in — for this instance or for every instance on that
// Java — and the install runs again only after the choice was saved.

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    javaDetect: vi.fn(),
    javaCheck: vi.fn(),
    setInstanceJava: vi.fn(),
    setJavaDefault: vi.fn(),
    ensureInstanceJre: vi.fn(),
  },
}));
const openMock = vi.fn();
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: (...a: unknown[]) => openMock(...a) }));

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

const noRuntime: JavaProblem = {
  kind: 'jre_unavailable',
  component: 'java-runtime-gamma',
  major: 17,
  stage: 'game_runtime',
  reason: { kind: 'unsupported_platform', os: 'linux', arch: 'aarch64' },
};

function setup(
  problem: JavaProblem = noRuntime,
  detected = [JDK21, JDK17, JDK8],
  followsSettings = true,
) {
  vi.mocked(commands.javaDetect).mockResolvedValue({ status: 'ok', data: detected });
  const onJavaSaved = vi.fn();
  const onRuntimeReady = vi.fn();
  const onProblem = vi.fn();
  render(JavaRescuePanel, {
    problem,
    message: "Lucerna couldn't get Java 17 for this version.",
    instanceId: 'a',
    followsSettings,
    onJavaSaved,
    onRuntimeReady,
    onProblem,
    onDismiss: vi.fn(),
  });
  return { onJavaSaved, onRuntimeReady, onProblem };
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

describe('JavaRescuePanel', () => {
  it('offers the Javas that can run the version, the one it names first', async () => {
    setup();
    expect(await screen.findByText(/A Java 17 or newer on this computer/)).toBeTruthy();
    const names = await optionNames();
    expect(names).toHaveLength(2);
    expect(names[0]).toMatch(/17\.0\.2/);
    expect(names[1]).toMatch(/21\.0\.4/);
  });

  it('"Use for this instance" saves the Java, then runs the install again', async () => {
    const { onJavaSaved } = setup();
    vi.mocked(commands.setInstanceJava).mockResolvedValue({
      status: 'ok',
      data: { instance: { id: 'a' } as never, report: null },
    });
    await fireEvent.click(await screen.findByRole('button', { name: 'Use for this instance' }));
    await waitFor(() => expect(onJavaSaved).toHaveBeenCalledWith('a'));
    expect(commands.setInstanceJava).toHaveBeenCalledWith('a', {
      kind: 'custom',
      path: JDK17.path,
    });
  });

  it('a refused write says why and retries nothing', async () => {
    const { onJavaSaved } = setup();
    vi.mocked(commands.setInstanceJava).mockResolvedValue({
      status: 'error',
      error: {
        kind: 'java_unusable',
        path: JDK17.path,
        origin: { kind: 'instance' },
        reason: { kind: 'wrong_arch', found: 'aarch64', expected: 'x86_64' },
      },
    } as never);
    await fireEvent.click(await screen.findByRole('button', { name: 'Use for this instance' }));
    expect(await screen.findByText(/can't be used/)).toBeTruthy();
    expect(onJavaSaved).not.toHaveBeenCalled();
  });

  it('"Use for every instance on Java 17" writes the Settings row', async () => {
    const { onJavaSaved } = setup();
    vi.mocked(commands.setJavaDefault).mockResolvedValue({
      status: 'ok',
      data: {
        settings: { java_defaults: [{ major: 17, path: JDK17.path }] } as never,
        report: null,
      },
    });
    await fireEvent.click(
      await screen.findByRole('button', { name: 'Use for every instance on Java 17' }),
    );
    await waitFor(() => expect(onJavaSaved).toHaveBeenCalledWith('a'));
    expect(commands.setJavaDefault).toHaveBeenCalledWith(17, JDK17.path);
  });

  // A Settings row does not reach an instance pinned to Lucerna's Java or to
  // its own: offering it would write the row and fail the same way again.
  it('offers no Settings row to an instance that does not follow Settings', async () => {
    setup(noRuntime, [JDK17], false);
    expect(await screen.findByRole('button', { name: 'Use for this instance' })).toBeTruthy();
    expect(screen.queryByRole('button', { name: /every instance/ })).toBeNull();
  });

  it('a search that failed is not "nothing found"', async () => {
    vi.mocked(commands.javaDetect).mockRejectedValue(new Error('ipc down'));
    const onJavaSaved = vi.fn();
    render(JavaRescuePanel, {
      problem: noRuntime,
      message: 'm',
      instanceId: 'a',
      followsSettings: true,
      onJavaSaved,
      onRuntimeReady: vi.fn(),
      onProblem: vi.fn(),
      onDismiss: vi.fn(),
    });
    expect(await screen.findByText(/Couldn't look for a Java 17/)).toBeTruthy();
    expect(screen.queryByText(/No Java 17 or newer was found/)).toBeNull();
  });

  it('asks a Java that does not say its version, and offers it when it fits', async () => {
    const quiet = found(null, null, 'jre1.8.0_503');
    vi.mocked(commands.javaCheck).mockResolvedValue({
      status: 'ok',
      data: {
        path: quiet.path,
        version: '17.0.9',
        major: 17,
        vendor: null,
        arch: 'x64',
        verdict: { kind: 'matches' },
        gpu_choice_ignored: true,
      },
    });
    setup(noRuntime, [quiet]);
    const names = await optionNames();
    expect(commands.javaCheck).toHaveBeenCalledWith(quiet.path, 17);
    expect(names).toEqual([expect.stringMatching(/17\.0\.9/)]);
  });

  it('with nothing found, says so and lets Browse pick one', async () => {
    setup(noRuntime, [JDK8]);
    expect(await screen.findByText(/No Java 17 or newer was found/)).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Use for this instance' })).toBeNull();
    openMock.mockResolvedValue('/home/me/jdk-17/bin/java');
    vi.mocked(commands.setInstanceJava).mockResolvedValue({
      status: 'ok',
      data: { instance: { id: 'a' } as never, report: null },
    });
    await fireEvent.click(screen.getByRole('button', { name: 'Browse…' }));
    await fireEvent.click(await screen.findByRole('button', { name: 'Use for this instance' }));
    await waitFor(() =>
      expect(commands.setInstanceJava).toHaveBeenCalledWith('a', {
        kind: 'custom',
        path: '/home/me/jdk-17/bin/java',
      }),
    );
  });

  it('a Forge or NeoForge install step is explained, not offered a Java', async () => {
    setup({ ...noRuntime, stage: 'installer_tools' });
    expect(await screen.findByTestId('java-rescue-installer')).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Use for this instance' })).toBeNull();
    expect(commands.javaDetect).not.toHaveBeenCalled();
  });

  it('a runtime that is only not downloaded is downloaded on request', async () => {
    const { onRuntimeReady, onJavaSaved } = setup({
      kind: 'managed_java_missing',
      component: 'java-runtime-gamma',
    });
    vi.mocked(commands.ensureInstanceJre).mockResolvedValue({ status: 'ok', data: null });
    await fireEvent.click(await screen.findByRole('button', { name: 'Download Java' }));
    await waitFor(() => expect(onRuntimeReady).toHaveBeenCalledWith('a'));
    expect(commands.ensureInstanceJre).toHaveBeenCalledWith('a');
    // Only the runtime was missing: no full install runs after it.
    expect(onJavaSaved).not.toHaveBeenCalled();
  });

  it('a download that cannot get the runtime hands the new problem up', async () => {
    const { onRuntimeReady, onProblem } = setup({
      kind: 'managed_java_missing',
      component: 'java-runtime-gamma',
    });
    vi.mocked(commands.ensureInstanceJre).mockResolvedValue({ status: 'error', error: noRuntime });
    await fireEvent.click(await screen.findByRole('button', { name: 'Download Java' }));
    await waitFor(() => expect(onProblem).toHaveBeenCalledWith(noRuntime));
    expect(onRuntimeReady).not.toHaveBeenCalled();
  });
});

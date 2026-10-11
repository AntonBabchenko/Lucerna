import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import JavaPathField from '$lib/instances/JavaPathField.svelte';
import { commands } from '$lib/ipc/bindings';
import JavaPathFieldInList from './fixtures/JavaPathFieldInList.svelte';

// The Java an instance runs on (spec 2026-10-11 custom-java §6.1): what a user
// can name — pick by Browse, pick from what was found, be refused with a
// reason and keep the old value, go back to Lucerna's Java.

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    javaDetect: vi.fn(),
    javaCheck: vi.fn(),
    instanceJavaRequirement: vi.fn(),
    setInstanceJava: vi.fn(),
  },
}));
const openMock = vi.fn();
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: (...a: unknown[]) => openMock(...a) }));

const JDK21 = {
  path: '/opt/jdk-21/bin/java',
  home: '/opt/jdk-21',
  major: 21,
  version: '21.0.4',
  vendor: 'Eclipse Adoptium',
  arch: 'amd64',
  source: 'known_dir' as const,
};
const JDK8 = {
  ...JDK21,
  path: '/opt/jdk8/bin/java',
  home: '/opt/jdk8',
  major: 8,
  version: '1.8.0_402',
};
const report = (path: string) => ({
  path,
  version: '21.0.4',
  major: 21,
  vendor: 'Eclipse Adoptium',
  arch: 'x64',
  verdict: { kind: 'matches' as const },
  gpu_choice_ignored: false,
});
const instance = (java: unknown) => ({ id: 'a', java }) as never;

function setup(java: unknown = null, detected = [JDK21, JDK8], required = 21) {
  vi.mocked(commands.javaDetect).mockResolvedValue({ status: 'ok', data: detected });
  vi.mocked(commands.instanceJavaRequirement).mockResolvedValue({ status: 'ok', data: required });
  vi.mocked(commands.javaCheck).mockResolvedValue({
    status: 'ok',
    data: report('/opt/jdk-21/bin/java'),
  });
  const onChanged = vi.fn();
  render(JavaPathField, {
    instanceId: 'a',
    value: java as never,
    mcVersion: '1.21.1',
    loader: 'fabric',
    loaderVersion: '0.16.5',
    ready: true,
    onChanged,
  });
  return { onChanged };
}

beforeAll(() => locale.set('en'));
afterEach(() => vi.clearAllMocks());

describe('JavaPathField', () => {
  it("starts on Lucerna's Java and says what that means", async () => {
    setup();
    expect(await screen.findByText(/Lucerna downloads the Java/)).toBeTruthy();
  });

  it('Browse saves the picked file and shows its verdict', async () => {
    const { onChanged } = setup();
    openMock.mockResolvedValue('/opt/jdk-21/bin/java');
    vi.mocked(commands.setInstanceJava).mockResolvedValue({
      status: 'ok',
      data: {
        instance: instance({ kind: 'custom', path: '/opt/jdk-21/bin/java' }),
        report: report('/opt/jdk-21/bin/java'),
      },
    });
    await fireEvent.click(await screen.findByRole('button', { name: 'Browse…' }));
    await waitFor(() =>
      expect(commands.setInstanceJava).toHaveBeenCalledWith('a', {
        kind: 'custom',
        path: '/opt/jdk-21/bin/java',
      }),
    );
    expect(onChanged).toHaveBeenCalled();
    expect(await screen.findByText(/the version this Minecraft needs/)).toBeTruthy();
    // The host here never refreshes its `value` prop: the field must describe
    // what was saved, not the old "Lucerna's Java" it was given.
    expect(screen.queryByText(/Lucerna downloads the Java/)).toBeNull();
    expect(screen.getByText('/opt/jdk-21/bin/java')).toBeTruthy();
  });

  it('a refused pick shows why and saves nothing', async () => {
    const { onChanged } = setup();
    openMock.mockResolvedValue('/opt/jdk8/bin/java');
    vi.mocked(commands.setInstanceJava).mockResolvedValue({
      status: 'error',
      error: {
        kind: 'java_unusable',
        path: '/opt/jdk8/bin/java',
        reason: { kind: 'too_old', found: 8, required: 21 },
      },
    } as never);
    await fireEvent.click(await screen.findByRole('button', { name: 'Browse…' }));
    expect(await screen.findByText(/needs Java 21 or newer/)).toBeTruthy();
    expect(onChanged).not.toHaveBeenCalled();
  });

  it('a cancelled dialog changes nothing', async () => {
    setup();
    openMock.mockResolvedValue(null);
    await fireEvent.click(await screen.findByRole('button', { name: 'Browse…' }));
    await waitFor(() => expect(openMock).toHaveBeenCalled());
    expect(commands.setInstanceJava).not.toHaveBeenCalled();
  });

  it("choosing Lucerna's Java pins it explicitly", async () => {
    const { onChanged } = setup({ kind: 'custom', path: '/opt/jdk-21/bin/java' });
    vi.mocked(commands.setInstanceJava).mockResolvedValue({
      status: 'ok',
      data: { instance: instance({ kind: 'bundled' }), report: null },
    });
    await fireEvent.click(await screen.findByRole('combobox', { name: 'Java' }));
    await fireEvent.mouseDown(await screen.findByRole('option', { name: /Lucerna's Java/ }));
    await waitFor(() =>
      expect(commands.setInstanceJava).toHaveBeenCalledWith('a', { kind: 'bundled' }),
    );
    expect(onChanged).toHaveBeenCalled();
  });

  it('picking a found Java saves its path', async () => {
    setup();
    vi.mocked(commands.setInstanceJava).mockResolvedValue({
      status: 'ok',
      data: {
        instance: instance({ kind: 'custom', path: '/opt/jdk-21/bin/java' }),
        report: report('/opt/jdk-21/bin/java'),
      },
    });
    await screen.findByText(/Lucerna downloads the Java/);
    await fireEvent.click(await screen.findByRole('combobox', { name: 'Java' }));
    await fireEvent.mouseDown(await screen.findByRole('option', { name: /Java 21\.0\.4/ }));
    await waitFor(() =>
      expect(commands.setInstanceJava).toHaveBeenCalledWith('a', {
        kind: 'custom',
        path: '/opt/jdk-21/bin/java',
      }),
    );
  });

  it('a Java too old for this version is listed but cannot be picked', async () => {
    setup();
    await screen.findByText(/Lucerna downloads the Java/);
    await fireEvent.click(await screen.findByRole('combobox', { name: 'Java' }));
    const old = await screen.findByRole('option', { name: /too old/ });
    expect(old.getAttribute('aria-disabled')).toBe('true');
    await fireEvent.mouseDown(old);
    expect(commands.setInstanceJava).not.toHaveBeenCalled();
  });

  it('lists the Java the version names first', async () => {
    const JDK25 = {
      ...JDK21,
      path: '/opt/jdk-25/bin/java',
      home: '/opt/jdk-25',
      major: 25,
      version: '25.0.1',
    };
    setup(null, [JDK25, JDK21, JDK8], 8);
    await screen.findByText(/Lucerna downloads the Java/);
    await fireEvent.click(await screen.findByRole('combobox', { name: 'Java' }));
    const names = (await screen.findAllByRole('option')).map((o) => o.textContent ?? '');
    expect(names[1]).toMatch(/1\.8\.0_402/);
    expect(names[2]).toMatch(/25\.0\.1/);
    expect(names[3]).toMatch(/21\.0\.4/);
  });

  it('says so when nothing else is installed', async () => {
    setup(null, []);
    expect(await screen.findByText(/No other Java found/)).toBeTruthy();
  });

  it('a host refresh with the same values neither re-probes nor wipes "Saved"', async () => {
    // Manage replaces its instance list wholesale on every refresh, and a save
    // triggers one: the field must not treat a new object with the same values
    // as a new instance.
    vi.mocked(commands.javaDetect).mockResolvedValue({ status: 'ok', data: [JDK21] });
    vi.mocked(commands.instanceJavaRequirement).mockResolvedValue({ status: 'ok', data: 21 });
    vi.mocked(commands.javaCheck).mockResolvedValue({
      status: 'ok',
      data: report('/opt/jdk-21/bin/java'),
    });
    vi.mocked(commands.setInstanceJava).mockResolvedValue({
      status: 'ok',
      data: {
        instance: instance({ kind: 'custom', path: '/opt/jdk-21/bin/java' }),
        report: report('/opt/jdk-21/bin/java'),
      },
    });
    render(JavaPathFieldInList);
    await waitFor(() => expect(commands.javaCheck).toHaveBeenCalledTimes(1));
    openMock.mockResolvedValue('/opt/jdk-21/bin/java');
    await fireEvent.click(await screen.findByRole('button', { name: 'Browse…' }));
    expect(await screen.findByText('Saved')).toBeTruthy();
    await fireEvent.click(screen.getByRole('button', { name: 'host refresh' }));
    await new Promise((r) => setTimeout(r, 0));
    expect(screen.getByText('Saved')).toBeTruthy();
    expect(commands.instanceJavaRequirement).toHaveBeenCalledTimes(1);
    expect(commands.javaCheck).toHaveBeenCalledTimes(1);
  });

  it('checks the saved Java against what the instance needs', async () => {
    setup({ kind: 'custom', path: '/opt/jdk-21/bin/java' });
    await waitFor(() => expect(commands.javaCheck).toHaveBeenCalledTimes(1));
    expect(commands.javaCheck).toHaveBeenCalledWith('/opt/jdk-21/bin/java', 21);
  });
});

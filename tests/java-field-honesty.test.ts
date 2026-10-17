import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import JavaPathField from '$lib/instances/JavaPathField.svelte';
import { commands } from '$lib/ipc/bindings';
import JavaChoiceField from '$lib/java/JavaChoiceField.svelte';
import { __resetAppSettingsForTest, appSettings } from '$lib/settings/app-settings.svelte';

// What the Java field tells the user (v0.28 design review, B2–B4, B7): the pin reads as a pin, a
// Settings row speaks for the instances that follow it, a refused pick says nothing changed, and
// the keyboard focus survives a save.

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    javaDetect: vi.fn(),
    javaCheck: vi.fn(),
    instanceJavaRequirement: vi.fn(),
    setInstanceJava: vi.fn(),
  },
}));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));

const JDK21 = {
  path: '/opt/jdk-21/bin/java',
  home: '/opt/jdk-21',
  major: 21,
  version: '21.0.4',
  vendor: 'Eclipse Adoptium',
  arch: 'amd64',
  source: 'known_dir' as const,
};
const report = (verdict: unknown, path = JDK21.path) => ({
  path,
  version: '21.0.4',
  major: 21,
  vendor: 'Eclipse Adoptium',
  arch: 'x64',
  verdict,
  gpu_choice_ignored: false,
});

function instanceField(java: unknown = null) {
  appSettings.loaded = { kind: 'ok', file: { general: { java_defaults: [] } } } as never;
  vi.mocked(commands.javaDetect).mockResolvedValue({ status: 'ok', data: [JDK21] });
  vi.mocked(commands.instanceJavaRequirement).mockResolvedValue({ status: 'ok', data: 21 });
  vi.mocked(commands.javaCheck).mockResolvedValue({
    status: 'ok',
    data: report({ kind: 'matches' }),
  } as never);
  render(JavaPathField, {
    instanceId: 'a',
    value: java as never,
    mcVersion: '1.21.1',
    loader: 'fabric',
    loaderVersion: '0.16.5',
    ready: true,
    onChanged: vi.fn(),
  });
}

beforeAll(() => locale.set('en'));
afterEach(() => {
  vi.clearAllMocks();
  __resetAppSettingsForTest();
});

describe('Java field — the pin reads as a pin', () => {
  it("lists «Always Lucerna's Java» beside Default, with no «recommended»", async () => {
    instanceField();
    await fireEvent.click(await screen.findByRole('combobox', { name: 'Java' }));
    expect(await screen.findByRole('option', { name: /^Always Lucerna's Java$/ })).toBeTruthy();
    expect(screen.queryByRole('option', { name: /recommended/ })).toBeNull();
  });

  it('a pinned instance says it ignores Settings', async () => {
    instanceField({ kind: 'bundled' });
    expect(
      await screen.findByText(/always runs on Lucerna's Java, whatever Settings names/),
    ).toBeTruthy();
  });
});

describe('Java field — a refused pick says nothing changed', () => {
  it('names the refused Java and keeps the old one', async () => {
    instanceField();
    vi.mocked(commands.setInstanceJava).mockResolvedValue({
      status: 'error',
      error: {
        kind: 'java_unusable',
        path: '/opt/mystery/bin/java',
        origin: { kind: 'instance' },
        reason: { kind: 'probe_failed', details: 'exit code 1' },
      },
    } as never);
    await fireEvent.click(await screen.findByRole('combobox', { name: 'Java' }));
    await fireEvent.mouseDown(await screen.findByRole('option', { name: /21\.0\.4/ }));
    expect(
      await screen.findByText(/^Not changed: \/opt\/mystery\/bin\/java can't be used\./),
    ).toBeTruthy();
  });
});

describe('Java field — the keyboard focus survives a save', () => {
  it('gives the focus back to the Java list when the save took it away', async () => {
    instanceField();
    vi.mocked(commands.setInstanceJava).mockImplementation(async () => {
      // Chromium drops the focus to <body> when the focused control turns disabled; happy-dom does
      // not (nor blurs a disabled control), so the loss is made here: focus a stand-in, remove it.
      await new Promise((r) => setTimeout(r, 0));
      const standIn = document.createElement('button');
      document.body.append(standIn);
      standIn.focus();
      standIn.remove();
      expect(document.activeElement === document.body || document.activeElement === null).toBe(
        true,
      );
      return {
        status: 'ok',
        data: { instance: { id: 'a', java: { kind: 'custom', path: JDK21.path } }, report: null },
      } as never;
    });
    const select = await screen.findByRole('combobox', { name: 'Java' });
    select.focus();
    await fireEvent.keyDown(select, { key: 'ArrowDown' });
    await fireEvent.keyDown(select, { key: 'End' });
    await fireEvent.keyDown(select, { key: 'Enter' });
    await waitFor(() => expect(commands.setInstanceJava).toHaveBeenCalled());
    // The save has finished once the field marks it saved; only then is the focus judged.
    await screen.findByText('Saved');
    await waitFor(() => expect(document.activeElement).toBe(select));
  });
});

describe('Java field — a Settings row speaks for its instances', () => {
  it('a newer Java on the Java 8 row names who is affected', async () => {
    vi.mocked(commands.javaDetect).mockResolvedValue({ status: 'ok', data: [JDK21] });
    vi.mocked(commands.javaCheck).mockResolvedValue({
      status: 'ok',
      data: report({ kind: 'newer', found: 21, required: 8 }),
    } as never);
    render(JavaChoiceField, {
      reloadKey: '8',
      value: { kind: 'custom', path: JDK21.path },
      label: 'Java 8',
      loadRequired: async () => ({ status: 'ok', data: 8 }),
      save: vi.fn(),
      lucernaWrites: null,
      bundledStatus: 'row',
      statusScope: 'row',
    });
    expect(
      await screen.findByText(/every instance that needs Java 8 and follows Settings/),
    ).toBeTruthy();
    expect(screen.queryByText(/this Minecraft/)).toBeNull();
  });
});

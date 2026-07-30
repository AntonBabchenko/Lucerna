import { fireEvent, render, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    instanceQuickPlaySupport: vi.fn(),
    listWorldNames: vi.fn(),
    listSavedServers: vi.fn(),
    shortcutCreate: vi.fn(),
    shortcutDefaultName: vi.fn(),
  },
}));

vi.mock('$lib/i18n', () => ({
  t: {
    subscribe: (run: (v: (k: string, p?: Record<string, unknown>) => string) => void) => {
      run((k: string) => k);
      return () => {};
    },
  },
}));

vi.mock('$lib/ipc/format-error', () => ({
  formatError: (e: { kind: string }) => `formatted:${e.kind}`,
}));

const pushActionToast = vi.fn();
vi.mock('$lib/toasts/toasts.svelte', () => ({
  pushActionToast: (...args: unknown[]) => pushActionToast(...args),
}));

import CreateShortcutDialog from '$lib/instances/CreateShortcutDialog.svelte';
import type { InstanceWithStatus } from '$lib/ipc/bindings';
import { commands } from '$lib/ipc/bindings';

function instance(over: Partial<InstanceWithStatus> = {}): InstanceWithStatus {
  return {
    id: 'inst-1',
    name: 'My Pack',
    mc_version: '1.21.1',
    loader: 'fabric',
    loader_version: '0.16.0',
    max_heap_mb: 4096,
    min_heap_mb: null,
    extra_jvm_args: '',
    created_unix_ms: null,
    ready: true,
    has_icon: false,
    ...over,
  } as InstanceWithStatus;
}

/** Resolve the mounted dialog's async onMount work. */
function seed({
  quickPlay = true,
  worlds = ['New World'],
  servers = [{ name: 'Home', address: 'mc.example.com:25565' }],
}: {
  quickPlay?: boolean;
  worlds?: string[];
  servers?: { name: string; address: string }[];
} = {}) {
  vi.mocked(commands.instanceQuickPlaySupport).mockResolvedValue({
    status: 'ok',
    data: quickPlay,
  } as never);
  vi.mocked(commands.listWorldNames).mockResolvedValue({
    status: 'ok',
    // Unique folder_name per entry — the world Select is keyed by value.
    data: worlds.map((w) => ({ folder_name: w, modified_unix_ms: null })),
  } as never);
  vi.mocked(commands.listSavedServers).mockResolvedValue({
    status: 'ok',
    data: servers,
  } as never);
  vi.mocked(commands.shortcutDefaultName).mockResolvedValue('My Pack' as never);
}

describe('CreateShortcutDialog', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    seed();
  });

  it('defaults to the instance target and pre-fills the name from the backend', async () => {
    const { getByTestId } = render(CreateShortcutDialog, {
      props: { instance: instance(), onClose: vi.fn() },
    });
    await waitFor(() =>
      expect((getByTestId('shortcut-name-input') as HTMLInputElement).value).toBe('My Pack'),
    );
    expect((getByTestId('shortcut-kind-instance') as HTMLInputElement).checked).toBe(true);
    // The default name comes from the same Rust helper the writer uses.
    expect(commands.shortcutDefaultName).toHaveBeenCalledWith('My Pack', {
      kind: 'instance',
      instance_id: 'inst-1',
    });
  });

  it('disables the Quick Play targets when the version predates 1.20', async () => {
    seed({ quickPlay: false });
    const { getByTestId } = render(CreateShortcutDialog, {
      props: { instance: instance({ mc_version: '1.19.4' }), onClose: vi.fn() },
    });
    await waitFor(() => expect(getByTestId('shortcut-no-quick-play')).toBeTruthy());
    expect((getByTestId('shortcut-kind-world') as HTMLInputElement).disabled).toBe(true);
    expect((getByTestId('shortcut-kind-server') as HTMLInputElement).disabled).toBe(true);
    // The instance target still works on any version.
    expect((getByTestId('shortcut-kind-instance') as HTMLInputElement).disabled).toBe(false);
  });

  it('creates a world shortcut with the selected folder', async () => {
    vi.mocked(commands.shortcutCreate).mockResolvedValue({
      status: 'ok',
      data: 'C:/Users/A/Desktop/My Pack - New World.lnk',
    } as never);
    const onClose = vi.fn();
    const { getByTestId } = render(CreateShortcutDialog, {
      props: { instance: instance(), onClose },
    });
    await waitFor(() => expect(commands.listWorldNames).toHaveBeenCalled());

    await fireEvent.change(getByTestId('shortcut-kind-world'));
    await fireEvent.click(getByTestId('shortcut-submit'));

    await waitFor(() =>
      expect(commands.shortcutCreate).toHaveBeenCalledWith(
        { kind: 'world', instance_id: 'inst-1', folder: 'New World' },
        'My Pack',
      ),
    );
    // Success surfaces as a toast carrying the created path, then closes.
    expect(pushActionToast).toHaveBeenCalled();
    expect(onClose).toHaveBeenCalled();
  });

  it('creates a server shortcut with the selected address', async () => {
    vi.mocked(commands.shortcutCreate).mockResolvedValue({
      status: 'ok',
      data: 'C:/Users/A/Desktop/x.lnk',
    } as never);
    const { getByTestId } = render(CreateShortcutDialog, {
      props: { instance: instance(), onClose: vi.fn() },
    });
    await waitFor(() => expect(commands.listSavedServers).toHaveBeenCalled());

    await fireEvent.change(getByTestId('shortcut-kind-server'));
    await fireEvent.click(getByTestId('shortcut-submit'));

    await waitFor(() =>
      expect(commands.shortcutCreate).toHaveBeenCalledWith(
        { kind: 'server', instance_id: 'inst-1', address: 'mc.example.com:25565' },
        'My Pack',
      ),
    );
  });

  it('keeps the dialog open and shows the error when creation fails', async () => {
    vi.mocked(commands.shortcutCreate).mockResolvedValue({
      status: 'error',
      error: { kind: 'io', path: 'p', details: 'denied' },
    } as never);
    const onClose = vi.fn();
    const { getByTestId } = render(CreateShortcutDialog, {
      props: { instance: instance(), onClose },
    });
    await waitFor(() =>
      expect((getByTestId('shortcut-name-input') as HTMLInputElement).value).toBe('My Pack'),
    );

    await fireEvent.click(getByTestId('shortcut-submit'));

    await waitFor(() =>
      expect(getByTestId('shortcut-error').textContent).toContain('formatted:io'),
    );
    expect(onClose).not.toHaveBeenCalled();
  });

  it('reports an instance with no saved worlds instead of offering an empty picker', async () => {
    seed({ worlds: [] });
    const { getByTestId } = render(CreateShortcutDialog, {
      props: { instance: instance(), onClose: vi.fn() },
    });
    await waitFor(() => expect(commands.listWorldNames).toHaveBeenCalled());
    await fireEvent.change(getByTestId('shortcut-kind-world'));
    await waitFor(() => expect(getByTestId('shortcut-no-worlds')).toBeTruthy());
    // Nothing to point at, so the action stays blocked.
    expect((getByTestId('shortcut-submit') as HTMLButtonElement).disabled).toBe(true);
  });

  it('stops re-deriving the name once the user edits it', async () => {
    const { getByTestId } = render(CreateShortcutDialog, {
      props: { instance: instance(), onClose: vi.fn() },
    });
    await waitFor(() =>
      expect((getByTestId('shortcut-name-input') as HTMLInputElement).value).toBe('My Pack'),
    );

    await fireEvent.input(getByTestId('shortcut-name-input'), { target: { value: 'Mine' } });
    vi.mocked(commands.shortcutDefaultName).mockResolvedValue('My Pack - New World' as never);
    await fireEvent.change(getByTestId('shortcut-kind-world'));

    // Switching target must not overwrite a deliberate name.
    expect((getByTestId('shortcut-name-input') as HTMLInputElement).value).toBe('Mine');
  });
});

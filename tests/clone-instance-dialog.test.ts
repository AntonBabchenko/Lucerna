import { fireEvent, render, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    cloneInstanceScan: vi.fn(),
    getPlaytime: vi.fn(),
  },
}));

vi.mock('$lib/ops/op-queue.svelte', () => ({
  enqueueClone: vi.fn(),
}));

vi.mock('$lib/i18n', () => ({
  t: {
    subscribe: (run: (v: (k: string, p?: Record<string, unknown>) => string) => void) => {
      run((k: string) => k);
      return () => {};
    },
  },
}));

import CloneInstanceDialog from '$lib/instances/CloneInstanceDialog.svelte';
import type { InstanceWithStatus } from '$lib/ipc/bindings';
import { commands } from '$lib/ipc/bindings';
import { enqueueClone } from '$lib/ops/op-queue.svelte';

function instance(over: Partial<InstanceWithStatus> = {}): InstanceWithStatus {
  return {
    id: 'inst-1',
    name: 'My Pack',
    mc_version: '1.20.4',
    loader: 'fabric',
    loader_version: '0.15.7',
    max_heap_mb: 4096,
    min_heap_mb: null,
    extra_jvm_args: '',
    created_unix_ms: null,
    ready: true,
    has_icon: false,
    mrpack_name: null,
    mrpack_version: null,
    mrpack_project_id: null,
    mrpack_source: null,
    mrpack_summary: null,
    mrpack_version_id: null,
    integrity: null,
    imported_from: null,
    created_from_server: null,
    ...over,
  } as InstanceWithStatus;
}

// Unique category per entry — the option list is keyed by option key.
const fullScan = [
  { category: 'mods' as const, file_count: 42, total_bytes: 104857600 },
  { category: 'saves' as const, file_count: 3, total_bytes: 2048 },
  { category: 'config' as const, file_count: 5, total_bytes: 1024 },
  { category: 'resource_packs' as const, file_count: 1, total_bytes: 512 },
  { category: 'shaderpacks' as const, file_count: 1, total_bytes: 256 },
  { category: 'options_txt' as const, file_count: 1, total_bytes: 128 },
];

const playtime = (sessions: number) => ({
  status: 'ok' as const,
  data: {
    total_seconds: sessions * 60,
    session_count: sessions,
    last_session_seconds: 60,
    last_session_unix_ms: sessions > 0 ? 1_700_000_000_000 : null,
  },
});

beforeEach(() => {
  vi.clearAllMocks();
  (commands.cloneInstanceScan as ReturnType<typeof vi.fn>).mockResolvedValue({
    status: 'ok',
    data: fullScan,
  });
  (commands.getPlaytime as ReturnType<typeof vi.fn>).mockResolvedValue(playtime(2));
});

describe('CloneInstanceDialog', () => {
  it('renders all six option checkboxes checked plus the mods-always note', async () => {
    const { getByTestId } = render(CloneInstanceDialog, {
      props: { instance: instance(), onClose: vi.fn() },
    });

    for (const key of ['saves', 'settings', 'packs', 'config', 'options_txt', 'playtime']) {
      const box = getByTestId(`clone-opt-${key}`) as HTMLInputElement;
      expect(box.checked, key).toBe(true);
    }
    expect(getByTestId('clone-mods-always')).toBeTruthy();
  });

  it('seeds the name from the source and keeps it within the 32-char limit', () => {
    const { getByTestId } = render(CloneInstanceDialog, {
      props: { instance: instance({ name: 'A'.repeat(32) }), onClose: vi.fn() },
    });
    const input = getByTestId('clone-name-input') as HTMLInputElement;
    expect(input.value.length).toBeGreaterThan(0);
    expect(input.value.length).toBeLessThanOrEqual(32);
  });

  it('unchecking an option is reflected in the enqueued request', async () => {
    const onClose = vi.fn();
    const { getByTestId } = render(CloneInstanceDialog, {
      props: { instance: instance(), onClose },
    });

    await fireEvent.click(getByTestId('clone-opt-saves'));
    await fireEvent.input(getByTestId('clone-name-input'), { target: { value: 'Branch' } });
    await fireEvent.click(getByTestId('clone-submit'));

    expect(enqueueClone).toHaveBeenCalledTimes(1);
    const [name, request] = (enqueueClone as ReturnType<typeof vi.fn>).mock.calls[0];
    expect(name).toBe('Branch');
    expect(request).toEqual({
      sourceId: 'inst-1',
      newName: 'Branch',
      options: {
        saves: false,
        settings: true,
        packs: true,
        config: true,
        options_txt: true,
        playtime: true,
      },
    });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('disables and unchecks a category the scan found empty', async () => {
    (commands.cloneInstanceScan as ReturnType<typeof vi.fn>).mockResolvedValue({
      status: 'ok',
      // No saves entry at all — nothing to copy.
      data: fullScan.filter((e) => e.category !== 'saves'),
    });

    const { getByTestId } = render(CloneInstanceDialog, {
      props: { instance: instance(), onClose: vi.fn() },
    });

    await waitFor(() => {
      const box = getByTestId('clone-opt-saves') as HTMLInputElement;
      expect(box.disabled).toBe(true);
      expect(box.checked).toBe(false);
    });
  });

  it('disables playtime with a hint when no session was ever recorded', async () => {
    (commands.getPlaytime as ReturnType<typeof vi.fn>).mockResolvedValue(playtime(0));

    const { getByTestId, getByText } = render(CloneInstanceDialog, {
      props: { instance: instance(), onClose: vi.fn() },
    });

    await waitFor(() => {
      const box = getByTestId('clone-opt-playtime') as HTMLInputElement;
      expect(box.disabled).toBe(true);
      expect(box.checked).toBe(false);
    });
    expect(getByText('instance.clone.noPlaytime')).toBeTruthy();
  });

  it('disables the Clone button while the name is empty', async () => {
    const { getByTestId } = render(CloneInstanceDialog, {
      props: { instance: instance(), onClose: vi.fn() },
    });

    await fireEvent.input(getByTestId('clone-name-input'), { target: { value: '   ' } });
    expect((getByTestId('clone-submit') as HTMLButtonElement).disabled).toBe(true);
    expect(enqueueClone).not.toHaveBeenCalled();
  });
});

// The Launch section of the instance Settings tab: memory, Advanced (-Xms and
// the instance's own Java) and extra JVM arguments. The memory and -Xms tests
// moved here from manage-modal-tier1.

import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import InstanceLaunchSection from '$lib/instances/settings/InstanceLaunchSection.svelte';
import type { InstanceWithStatus } from '$lib/ipc/bindings';
import * as toasts from '$lib/toasts/toasts.svelte';

const m = vi.hoisted(() => ({
  // The command answers the bounds themselves, not a Result (memory-bounds.ts).
  instanceMemoryBounds: vi.fn().mockResolvedValue({
    min_mb: 1024,
    max_mb: 8192,
    default_mb: 2048,
    recommended_max_mb: 8192,
    step_mb: 256,
    ram_known: false,
  }),
  setInstanceMemory: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  setInstanceMinHeap: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  setInstanceJvmArgs: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  javaDetect: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  javaCheck: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  instanceJavaRequirement: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  setInstanceJava: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
}));

vi.mock('$lib/ipc/bindings', () => ({ commands: { ...m }, events: {} }));
vi.mock('$lib/toasts/toasts.svelte', async (importOriginal) => {
  const actual = await importOriginal<typeof import('$lib/toasts/toasts.svelte')>();
  return { ...actual, pushSuccess: vi.fn(), pushWarning: vi.fn(), pushInfo: vi.fn() };
});
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));

function makeInstance(over: Partial<InstanceWithStatus> = {}): InstanceWithStatus {
  return {
    id: 'inst-1',
    name: 'Default',
    mc_version: '1.20.1',
    loader: 'vanilla',
    loader_version: null,
    max_heap_mb: 2048,
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
    java: null,
    ...over,
  };
}

function mount(over: Partial<InstanceWithStatus> = {}, onChanged = vi.fn()) {
  const r = render(InstanceLaunchSection, {
    props: { instance: makeInstance(over), onChanged },
  });
  return { ...r, onChanged };
}

// Content inside a closed <details> is treated as hidden by role queries.
function openAdvanced() {
  const details = screen.getByText('Advanced').closest('details') as HTMLDetailsElement;
  details.open = true;
  details.dispatchEvent(new Event('toggle'));
}

describe('InstanceLaunchSection', () => {
  beforeAll(() => locale.set('en'));
  beforeEach(() => vi.clearAllMocks());

  it('persists memory only on release, not on every drag tick', async () => {
    mount({ max_heap_mb: 2048 });
    const slider = (await screen.findByRole('slider')) as HTMLInputElement;
    await fireEvent.input(slider, { target: { value: '4096' } });
    expect(m.setInstanceMemory).not.toHaveBeenCalled();
    await fireEvent.change(slider, { target: { value: '4096' } });
    expect(m.setInstanceMemory).toHaveBeenCalledTimes(1);
    expect(m.setInstanceMemory).toHaveBeenCalledWith('inst-1', 4096);
  });

  it('a background refresh of the same instance keeps a dragged value; another instance reseeds', async () => {
    const { rerender, onChanged } = mount({ max_heap_mb: 2048 });
    const slider = (await screen.findByRole('slider')) as HTMLInputElement;
    await fireEvent.input(slider, { target: { value: '4096' } });
    await rerender({ instance: makeInstance({ max_heap_mb: 2048 }), onChanged });
    expect((screen.getByLabelText('Memory in MiB') as HTMLInputElement).value).toBe('4096');
    await rerender({ instance: makeInstance({ id: 'inst-2', max_heap_mb: 3072 }), onChanged });
    await waitFor(() =>
      expect((screen.getByLabelText('Memory in MiB') as HTMLInputElement).value).toBe('3072'),
    );
  });

  it('shows Saved after a JVM arguments change and hides it on re-edit', async () => {
    mount();
    const input = screen.getByLabelText(/jvm args/i) as HTMLInputElement;
    await fireEvent.change(input, { target: { value: '-XX:+UseG1GC' } });
    await waitFor(() => expect(screen.getByText('Saved')).toBeTruthy());
    expect(m.setInstanceJvmArgs).toHaveBeenCalledWith('inst-1', '-XX:+UseG1GC');
    await fireEvent.input(input, { target: { value: '-XX:+UseG1GC -X' } });
    expect(screen.queryByText('Saved')).toBeNull();
  });

  it('keeps Advanced collapsed, and mounts the Java field only once it is open', async () => {
    const { container } = mount();
    const details = container.querySelector('details');
    expect(details?.hasAttribute('open')).toBe(false);
    expect(m.instanceJavaRequirement).not.toHaveBeenCalled();
    expect(m.javaDetect).not.toHaveBeenCalled();
    openAdvanced();
    await waitFor(() => expect(m.instanceJavaRequirement).toHaveBeenCalled());
  });

  it('persists a typed initial heap, capped at the current max', async () => {
    mount({ max_heap_mb: 4096 });
    openAdvanced();
    const xms = (await screen.findByLabelText(/initial heap/i)) as HTMLInputElement;
    await fireEvent.input(xms, { target: { value: '2048' } });
    await fireEvent.change(xms, { target: { value: '2048' } });
    expect(m.setInstanceMinHeap).toHaveBeenCalledWith('inst-1', 2048);
    await fireEvent.input(xms, { target: { value: '9000' } });
    await fireEvent.change(xms, { target: { value: '9000' } });
    expect(m.setInstanceMinHeap).toHaveBeenLastCalledWith('inst-1', 4096);
  });

  it('"= max" sets the initial heap to the current maximum', async () => {
    mount({ max_heap_mb: 4096 });
    openAdvanced();
    await fireEvent.click(screen.getByRole('button', { name: /=\s*max/i }));
    expect(m.setInstanceMinHeap).toHaveBeenCalledWith('inst-1', 4096);
  });

  it('clearing the field clears the initial heap (null)', async () => {
    mount({ max_heap_mb: 4096, min_heap_mb: 2048 });
    openAdvanced();
    const xms = (await screen.findByLabelText(/initial heap/i)) as HTMLInputElement;
    await fireEvent.input(xms, { target: { value: '' } });
    await fireEvent.change(xms, { target: { value: '' } });
    expect(m.setInstanceMinHeap).toHaveBeenCalledWith('inst-1', null);
  });

  // Leaving the tab blurs the field and unmounts the section in one click: the
  // save lands after the section is gone.
  it('a save that fails after the tab is gone is a toast naming the instance', async () => {
    let resolve!: (v: unknown) => void;
    m.setInstanceJvmArgs.mockReturnValueOnce(new Promise((r) => (resolve = r)));
    const { unmount, onChanged } = mount();
    const input = screen.getByLabelText(/jvm args/i) as HTMLInputElement;
    await fireEvent.change(input, { target: { value: '-Xbad' } });
    unmount();
    resolve({ status: 'error', error: { kind: 'io', path: 'instance.json', details: 'denied' } });
    await waitFor(() => expect(vi.mocked(toasts.pushWarning)).toHaveBeenCalled());
    expect(vi.mocked(toasts.pushWarning).mock.calls[0][0]).toBe(
      "Extra JVM args of Default wasn't saved",
    );
    expect(onChanged).not.toHaveBeenCalled();
  });

  it('a save that succeeds after the tab is gone still refreshes the list', async () => {
    let resolve!: (v: unknown) => void;
    m.setInstanceMemory.mockReturnValueOnce(new Promise((r) => (resolve = r)));
    const { unmount, onChanged } = mount();
    const slider = (await screen.findByRole('slider')) as HTMLInputElement;
    await fireEvent.change(slider, { target: { value: '4096' } });
    unmount();
    resolve({ status: 'ok', data: null });
    await waitFor(() => expect(onChanged).toHaveBeenCalledTimes(1));
    expect(vi.mocked(toasts.pushWarning)).not.toHaveBeenCalled();
  });
});

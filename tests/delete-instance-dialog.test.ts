import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import DeleteInstanceDialog from '$lib/instances/DeleteInstanceDialog.svelte';
import type { InstanceWithStatus } from '$lib/ipc/bindings';

const m = vi.hoisted(() => ({ deleteInstance: vi.fn() }));
vi.mock('$lib/ipc/bindings', () => ({ commands: { deleteInstance: m.deleteInstance } }));

function instance(over: Partial<InstanceWithStatus> = {}): InstanceWithStatus {
  return {
    id: 'inst-2',
    name: 'Other',
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

function mount() {
  const props = {
    instance: instance(),
    refresh: vi.fn().mockResolvedValue(undefined),
    onDeleted: vi.fn(),
    onClose: vi.fn(),
  };
  render(DeleteInstanceDialog, { props });
  return props;
}

const confirm = () => screen.getByTestId('instance-delete-confirm') as HTMLButtonElement;

describe('DeleteInstanceDialog', () => {
  beforeAll(() => locale.set('en'));
  beforeEach(() => vi.clearAllMocks());

  // One warning, no word to type (maintainer, 2026-10-12) — as deleting a world
  // or a server asks.
  it('asks once: names the instance, warns it cannot be undone, Delete is on at once', () => {
    mount();
    expect(screen.getByText(/Other/)).toBeTruthy();
    expect(screen.getByText(/cannot be undone/)).toBeTruthy();
    expect(screen.queryByRole('textbox')).toBeNull();
    expect(confirm().disabled).toBe(false);
  });

  it('starts on Cancel, so Enter on open does not delete', async () => {
    mount();
    await waitFor(() =>
      expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Cancel' })),
    );
  });

  it('deletes once and cannot be cancelled while it runs', async () => {
    let resolve!: (v: unknown) => void;
    m.deleteInstance.mockReturnValue(new Promise((r) => (resolve = r)));
    const p = mount();
    await fireEvent.click(confirm());
    await fireEvent.click(confirm());
    expect(m.deleteInstance).toHaveBeenCalledTimes(1);
    expect(m.deleteInstance).toHaveBeenCalledWith('inst-2');
    await fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    await fireEvent.keyDown(window, { key: 'Escape' });
    expect(p.onClose).not.toHaveBeenCalled();
    resolve({ status: 'ok', data: null });
    await waitFor(() => expect(p.onClose).toHaveBeenCalledTimes(1));
    expect(p.refresh).toHaveBeenCalled();
    expect(p.onDeleted).toHaveBeenCalledTimes(1);
  });

  it('a failure stays in the dialog and still refreshes — part may already be gone', async () => {
    m.deleteInstance.mockResolvedValue({
      status: 'error',
      error: { kind: 'io', path: 'C:/x/mods/a.jar', message: 'in use' },
    });
    const p = mount();
    await fireEvent.click(confirm());
    await waitFor(() => expect(p.refresh).toHaveBeenCalled());
    expect(p.onClose).not.toHaveBeenCalled();
    expect(p.onDeleted).not.toHaveBeenCalled();
    expect(screen.getByTestId('instance-delete-error').textContent).toBeTruthy();
    // The user can try again or leave.
    expect(confirm().disabled).toBe(false);
  });
});

import { render } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { InstanceWithStatus } from '$lib/ipc/bindings';

// The modal fans out to IPC on mount (compat check, integrity, memory bounds,
// and the loader picker's version list). Every one of them is stubbed so an
// unhandled rejection can never be mistaken for a flash that did not happen.
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    instancePathStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'ok' }),
    previewInstanceDirName: vi.fn().mockResolvedValue('Preview-Name'),
    checkInstanceModCompat: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    instanceIntegrityStatus: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    instanceMemoryBounds: vi
      .fn()
      .mockResolvedValue({ status: 'ok', data: { min_mb: 1024, max_mb: 16384, step_mb: 512 } }),
    // Must contain the fixture's loader_version: an empty list makes the
    // picker auto-correct to stable and commit that back through
    // setInstanceLoader, which is a write this test has no business making.
    listFabricLoaders: vi
      .fn()
      .mockResolvedValue({ status: 'ok', data: [{ version: '0.16.5', stable: true }] }),
  },
}));

import ManageInstancesModal from '$lib/instances/ManageInstancesModal.svelte';

function instance(over: Partial<InstanceWithStatus> = {}): InstanceWithStatus {
  return {
    id: 'i1',
    name: 'Skyblock',
    mc_version: '1.21.1',
    loader: 'fabric',
    loader_version: '0.16.5',
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

function mount(focusField: string | null) {
  const inst = instance();
  return render(ManageInstancesModal, {
    props: {
      open: true,
      instances: [inst],
      activeInstance: inst,
      versions: [{ id: '1.21.1', version_type: 'release' }],
      onChanged: () => {},
      focusField,
    } as never,
  });
}

beforeEach(() => {
  document.body.innerHTML = '';
});

describe('ManageInstancesModal focusField', () => {
  it('flashes the memory field without stealing focus', () => {
    mount('memory');
    const zone = document.querySelector('[data-focus-field="memory"]');
    expect(zone?.classList.contains('field-flash')).toBe(true);
    expect(zone?.contains(document.activeElement)).toBe(false);
  });

  it('flashes AND focuses the Minecraft version field', () => {
    mount('mc');
    const zone = document.querySelector('[data-focus-field="mc"]');
    expect(zone?.classList.contains('field-flash')).toBe(true);
    expect(zone?.contains(document.activeElement)).toBe(true);
  });

  it('flashes the integrity section without stealing focus', () => {
    mount('integrity');
    const zone = document.querySelector('[data-focus-field="integrity"]');
    expect(zone?.classList.contains('field-flash')).toBe(true);
    expect(zone?.contains(document.activeElement)).toBe(false);
  });

  it('flashes nothing when no field was requested', () => {
    mount(null);
    expect(document.querySelector('.field-flash')).toBeNull();
  });
});

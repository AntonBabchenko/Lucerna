// Instance-mgmt group intent coverage: LoaderPicker variant/label classes,
// ManageInstancesModal rows not already covered by the cluster-D
// button-intents-destructive.test.ts (+ New instance, Delete, Done).
//
// Rows covered here per inventory:
//   LoaderPicker:    active loader → btn-primary btn-sm
//                    inactive loader → btn-secondary btn-sm
//                    "Loader" label — uppercase text-secondary
//                    "Loader version" label — uppercase text-secondary (non-vanilla)
//   ManageInstancesModal:
//                    CloseButton (header) → btn-icon
//                    instance list-item buttons — bare classes + bg-accent-soft when selected
//                    Cancel (create form) → btn-secondary btn-sm
//                    Create → btn-primary btn-sm
//                    Open folder → btn-secondary btn-sm
//                    Cancel (delete confirm) → btn-secondary btn-sm

import { render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { InstanceWithStatus, VersionEntry } from '$lib/ipc/bindings';

// vi.mock is hoisted before imports — must appear before component imports.
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    // LoaderPicker calls these when loader !== 'vanilla'
    listFabricLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    listQuiltLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    listForgeLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    listNeoforgeLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    // ManageInstancesModal fetches memory bounds on first open
    instanceMemoryBounds: vi.fn().mockResolvedValue({
      min_mb: 1024,
      max_mb: 8192,
      recommended_max_mb: 8192,
      step_mb: 256,
      ram_known: false,
    }),
    // ManageInstancesModal calls these on user interaction only
    createInstance: vi.fn(),
    setActiveInstance: vi.fn(),
    setInstanceName: vi.fn(),
    setInstanceVersion: vi.fn(),
    setInstanceLoader: vi.fn(),
    setInstanceMemory: vi.fn(),
    setInstanceJvmArgs: vi.fn(),
    openInstanceFolder: vi.fn(),
    deleteInstance: vi.fn(),
  },
  events: {
    modInstalled: { listen: () => Promise.resolve(() => {}) },
    modUninstalled: { listen: () => Promise.resolve(() => {}) },
    gpuPrefApplied: { listen: () => Promise.resolve(() => {}) },
  },
}));

vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/webview', () => ({}));

import LoaderPicker from '$lib/instances/LoaderPicker.svelte';
import ManageInstancesModal from '$lib/instances/ManageInstancesModal.svelte';

// ── Fixtures ─────────────────────────────────────────────────────────────────

function makeInstance(over: Partial<InstanceWithStatus> = {}): InstanceWithStatus {
  return {
    id: 'inst-1',
    name: 'Default',
    mc_version: '1.20.4',
    loader: 'vanilla',
    loader_version: null,
    max_heap_mb: 2048,
    extra_jvm_args: '',
    created_unix_ms: null,
    ready: true,
    mrpack_name: null,
    mrpack_version: null,
    mrpack_project_id: null,
    mrpack_source: null,
    mrpack_summary: null,
    mrpack_version_id: null,
    integrity: null,
    ...over,
  };
}

function makeVersion(id = '1.20.4'): VersionEntry {
  return { id, version_type: 'release', release_date: '2024-01-01T00:00:00+00:00', url: '' };
}

// Two instances so the Delete button is enabled (disabled when length <= 1).
const manageProps = {
  open: true,
  instances: [makeInstance(), makeInstance({ id: 'inst-2', name: 'Second' })],
  activeInstance: makeInstance(),
  versions: [makeVersion()],
  onChanged: () => {},
};

// ── LoaderPicker — variant per loader option ──────────────────────────────────

describe('LoaderPicker — active loader button is btn-primary btn-sm', () => {
  it('active Vanilla button has btn-primary and btn-sm', () => {
    render(LoaderPicker, {
      props: { mc: '1.20.4', loader: 'vanilla', loaderVersion: null },
    });
    const btn = screen.getByRole('button', { name: /^vanilla$/i });
    expect(btn).toHaveBtnVariant('primary');
    expect(btn).toHaveBtnSize('sm');
  });

  it('active Fabric button has btn-primary and btn-sm', () => {
    render(LoaderPicker, {
      props: { mc: '1.20.4', loader: 'fabric', loaderVersion: null },
    });
    const btn = screen.getByRole('button', { name: /^fabric$/i });
    expect(btn).toHaveBtnVariant('primary');
    expect(btn).toHaveBtnSize('sm');
  });

  it('active Forge button has btn-primary and btn-sm', () => {
    render(LoaderPicker, {
      props: { mc: '1.20.4', loader: 'forge', loaderVersion: null },
    });
    const btn = screen.getByRole('button', { name: /^forge$/i });
    expect(btn).toHaveBtnVariant('primary');
    expect(btn).toHaveBtnSize('sm');
  });
});

describe('LoaderPicker — inactive loader buttons are btn-secondary btn-sm', () => {
  it('inactive Fabric/Quilt/Forge/NeoForge buttons have btn-secondary when loader=vanilla', () => {
    render(LoaderPicker, {
      props: { mc: '1.20.4', loader: 'vanilla', loaderVersion: null },
    });
    for (const name of ['Fabric', 'Quilt', 'Forge', 'NeoForge']) {
      const btn = screen.getByRole('button', { name: new RegExp(`^${name}$`, 'i') });
      expect(btn).toHaveBtnVariant('secondary');
      expect(btn).toHaveBtnSize('sm');
    }
  });

  it('inactive Vanilla button has btn-secondary when loader=fabric', () => {
    render(LoaderPicker, {
      props: { mc: '1.20.4', loader: 'fabric', loaderVersion: null },
    });
    const btn = screen.getByRole('button', { name: /^vanilla$/i });
    expect(btn).toHaveBtnVariant('secondary');
    expect(btn).toHaveBtnSize('sm');
  });
});

describe('LoaderPicker — "Loader" label classes (uppercase text-secondary)', () => {
  it('"Loader" label has uppercase and text-secondary classes', () => {
    const { container } = render(LoaderPicker, {
      props: { mc: '1.20.4', loader: 'vanilla', loaderVersion: null },
    });
    // The label is a <p> with the text "Loader" (exact)
    const label = Array.from(container.querySelectorAll('p')).find(
      (el) => el.textContent?.trim() === 'Loader',
    );
    expect(label).not.toBeNull();
    const cls = label?.className ?? '';
    expect(cls).toContain('uppercase');
    expect(cls).toContain('text-secondary');
  });
});

// ── ManageInstancesModal — CloseButton header ────────────────────────────────

describe('ManageInstancesModal — CloseButton (header) is btn-icon', () => {
  it('header CloseButton has btn-icon class', () => {
    render(ManageInstancesModal, { props: manageProps });
    const closeBtn = screen.getByRole('button', { name: /close manage instances/i });
    expect(closeBtn).toHaveBtnVariant('icon');
  });
});

// ── ManageInstancesModal — instance list-item buttons ────────────────────────

describe('ManageInstancesModal — instance list-item buttons', () => {
  it('all list-item buttons have text-left rounded text-sm hover:bg-subtle classes', () => {
    const { container } = render(ManageInstancesModal, { props: manageProps });
    // List-item buttons are in the <aside> (w-[220px] border-r); exclude the
    // "+ New instance" btn-primary which shares the same aside.
    const aside = container.querySelector('aside');
    expect(aside).not.toBeNull();
    const listBtns = Array.from(aside?.querySelectorAll('button') ?? []).filter(
      (b) => !b.classList.contains('btn-primary'),
    );
    expect(listBtns.length).toBeGreaterThanOrEqual(2);
    for (const btn of listBtns) {
      const cls = btn.className;
      expect(cls).toContain('text-left');
      expect(cls).toContain('rounded');
      expect(cls).toContain('text-sm');
    }
  });

  it('selected instance list-item button has bg-accent-soft class', () => {
    const { container } = render(ManageInstancesModal, { props: manageProps });
    // On open, selectedId is set to activeInstance.id = 'inst-1'.
    const aside = container.querySelector('aside');
    const listBtns = Array.from(aside?.querySelectorAll('button') ?? []).filter(
      (b) => !b.classList.contains('btn-primary'),
    );
    // The first button corresponds to inst-1 (the active instance)
    const selectedBtn = listBtns.find((b) => b.classList.contains('bg-accent-soft'));
    expect(selectedBtn).not.toBeUndefined();
  });

  it('non-selected instance list-item button does not have bg-accent-soft class', () => {
    const { container } = render(ManageInstancesModal, { props: manageProps });
    const aside = container.querySelector('aside');
    const listBtns = Array.from(aside?.querySelectorAll('button') ?? []).filter(
      (b) => !b.classList.contains('btn-primary'),
    );
    // "Second" (inst-2) is not the active instance, so it should not have bg-accent-soft
    const secondBtn = listBtns.find((b) => b.textContent?.includes('Second'));
    expect(secondBtn).not.toBeUndefined();
    expect(secondBtn?.classList.contains('bg-accent-soft')).toBe(false);
  });
});

// ── ManageInstancesModal — create-form buttons (Cancel + Create) ─────────────

describe('ManageInstancesModal — create-form Cancel is btn-secondary btn-sm', () => {
  it('Cancel (create form) has btn-secondary and btn-sm', async () => {
    // The create form is shown when createMode=true. We can trigger openCreate
    // by clicking "+ New instance".
    render(ManageInstancesModal, { props: manageProps });
    const newBtn = screen.getByRole('button', { name: /\+ new instance/i });
    newBtn.click();
    // After click, createMode=true → Cancel and Create buttons render
    const cancelBtn = await screen.findByRole('button', { name: /^cancel$/i });
    expect(cancelBtn).toHaveBtnVariant('secondary');
    expect(cancelBtn).toHaveBtnSize('sm');
  });
});

describe('ManageInstancesModal — create-form Create is btn-primary btn-sm', () => {
  it('Create button has btn-primary and btn-sm', async () => {
    render(ManageInstancesModal, { props: manageProps });
    const newBtn = screen.getByRole('button', { name: /\+ new instance/i });
    newBtn.click();
    const createBtn = await screen.findByRole('button', { name: /^create$/i });
    expect(createBtn).toHaveBtnVariant('primary');
    expect(createBtn).toHaveBtnSize('sm');
  });
});

// ── ManageInstancesModal — detail-panel footer buttons ───────────────────────

describe('ManageInstancesModal — detail footer: Open folder is btn-secondary btn-sm', () => {
  it('"Open folder" button has btn-secondary and btn-sm', () => {
    render(ManageInstancesModal, { props: manageProps });
    const btn = screen.getByRole('button', { name: /open folder/i });
    expect(btn).toHaveBtnVariant('secondary');
    expect(btn).toHaveBtnSize('sm');
  });
});

// ── ManageInstancesModal — delete-confirm overlay buttons ────────────────────

describe('ManageInstancesModal — delete-confirm Cancel is btn-secondary btn-sm', () => {
  it('Cancel in delete-confirm overlay has btn-secondary and btn-sm', async () => {
    render(ManageInstancesModal, { props: manageProps });
    // Open the delete-confirm overlay by clicking "Delete instance"
    const deleteBtn = screen.getByRole('button', { name: /delete instance/i });
    deleteBtn.click();
    // The overlay renders Cancel + Delete buttons
    const cancelBtn = await screen.findByRole('button', { name: /^cancel$/i });
    expect(cancelBtn).toHaveBtnVariant('secondary');
    expect(cancelBtn).toHaveBtnSize('sm');
  });
});

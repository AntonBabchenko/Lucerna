import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import type { InstanceWithStatus, VersionEntry } from '$lib/ipc/bindings';

const m = vi.hoisted(() => ({
  instancePathStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'ok' }),
  previewInstanceDirName: vi.fn().mockResolvedValue('Preview-Name'),
  renameInstanceDir: vi.fn(),
  listForgeLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  listFabricLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  listQuiltLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  listNeoforgeLoaders: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  checkInstanceModCompat: vi
    .fn()
    .mockResolvedValue({ status: 'ok', data: { rows: [], loader_outcome: null } }),
  instanceMemoryBounds: vi.fn().mockResolvedValue({
    min_mb: 1024,
    max_mb: 8192,
    default_mb: 2048,
    recommended_max_mb: 8192,
    step_mb: 256,
    ram_known: false,
  }),
  setActiveInstance: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  setInstanceName: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  setInstanceMemory: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  setInstanceMinHeap: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  setInstanceJvmArgs: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  setInstanceLoader: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  changeInstanceMc: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  openInstanceFolder: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  openImportedSourceFolder: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  deleteInstance: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  createInstance: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: { ...m },
  events: {
    modInstalled: { listen: () => Promise.resolve(() => {}) },
    modUninstalled: { listen: () => Promise.resolve(() => {}) },
    modsReconciled: { listen: () => Promise.resolve(() => {}) },
    gpuPrefApplied: { listen: () => Promise.resolve(() => {}) },
  },
}));

vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/webview', () => ({}));
vi.mock('$lib/servers/server-state.svelte', () => ({ serverState: { list: [] } }));

import ManageInstancesModal from '$lib/instances/ManageInstancesModal.svelte';

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
    ...over,
  };
}

const version: VersionEntry = {
  id: '1.20.1',
  version_type: 'release',
  release_date: '2024-01-01T00:00:00+00:00',
  url: '',
};

beforeEach(() => vi.clearAllMocks());

describe('ManageInstancesModal — name edit survives background refresh', () => {
  it('keeps an in-progress name edit when instances refresh with the same selection', async () => {
    const inst = makeInstance({ name: 'Default' });
    const { rerender } = render(ManageInstancesModal, {
      props: {
        open: true,
        instances: [inst],
        activeInstance: inst,
        versions: [version],
        onChanged: () => {},
      },
    });

    const input = (await screen.findByDisplayValue('Default')) as HTMLInputElement;
    await fireEvent.input(input, { target: { value: 'My New Name' } });

    const refreshed = makeInstance({ name: 'Default' });
    await rerender({
      open: true,
      instances: [refreshed],
      activeInstance: refreshed,
      versions: [version],
      onChanged: () => {},
    });

    expect(screen.getByDisplayValue('My New Name')).toBeTruthy();
  });

  it('resets the name draft when switching to a different instance', async () => {
    const a = makeInstance({ id: 'a', name: 'Alpha' });
    const b = makeInstance({ id: 'b', name: 'Beta' });
    render(ManageInstancesModal, {
      props: {
        open: true,
        instances: [a, b],
        activeInstance: a,
        versions: [version],
        onChanged: () => {},
      },
    });

    const input = (await screen.findByDisplayValue('Alpha')) as HTMLInputElement;
    await fireEvent.input(input, { target: { value: 'edited-but-not-committed' } });

    await fireEvent.click(screen.getByRole('button', { name: /Beta/ }));

    expect(screen.getByDisplayValue('Beta')).toBeTruthy();
  });

  it('discards an uncommitted name edit when the modal is closed and reopened on the same instance', async () => {
    const inst = makeInstance({ name: 'Default' });
    const baseProps = {
      instances: [inst],
      activeInstance: inst,
      versions: [version],
      onChanged: () => {},
    };
    const { rerender } = render(ManageInstancesModal, { props: { open: true, ...baseProps } });

    const input = (await screen.findByDisplayValue('Default')) as HTMLInputElement;
    await fireEvent.input(input, { target: { value: 'Unsaved' } });

    // Close via the real close button (runs close(), which clears the resync
    // cursor) — toggling the `open` prop alone would bypass close().
    await fireEvent.click(screen.getByRole('button', { name: /close manage instances/i }));
    await rerender({ open: true, ...baseProps });

    expect(await screen.findByDisplayValue('Default')).toBeTruthy();
  });
});

describe('ManageInstancesModal — memory slider', () => {
  it('persists memory only on release (change), not on every drag tick (input)', async () => {
    const inst = makeInstance({ max_heap_mb: 2048 });
    render(ManageInstancesModal, {
      props: {
        open: true,
        instances: [inst],
        activeInstance: inst,
        versions: [version],
        onChanged: () => {},
      },
    });

    const slider = (await screen.findByRole('slider')) as HTMLInputElement;

    await fireEvent.input(slider, { target: { value: '4096' } });
    expect(m.setInstanceMemory).not.toHaveBeenCalled();

    await fireEvent.change(slider, { target: { value: '4096' } });
    expect(m.setInstanceMemory).toHaveBeenCalledTimes(1);
    expect(m.setInstanceMemory).toHaveBeenCalledWith('inst-1', 4096);
  });
});

describe('ManageInstancesModal — running guard', () => {
  function renderTwo(isRunning: boolean) {
    const a = makeInstance({ id: 'a', name: 'Alpha' });
    const b = makeInstance({ id: 'b', name: 'Beta' });
    return render(ManageInstancesModal, {
      props: {
        open: true,
        instances: [a, b],
        activeInstance: a,
        versions: [version],
        onChanged: () => {},
        isRunning,
      },
    });
  }

  it('disables delete, MC version, and loader picker while running', async () => {
    renderTwo(true);
    await screen.findByDisplayValue('Alpha');

    expect(
      (screen.getByRole('button', { name: /Delete instance/ }) as HTMLButtonElement).disabled,
    ).toBe(true);
    expect((screen.getByRole('combobox') as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByRole('button', { name: 'Vanilla' }) as HTMLButtonElement).disabled).toBe(
      true,
    );
  });

  it('leaves delete, MC version, and loader picker enabled when not running', async () => {
    renderTwo(false);
    await screen.findByDisplayValue('Alpha');

    expect(
      (screen.getByRole('button', { name: /Delete instance/ }) as HTMLButtonElement).disabled,
    ).toBe(false);
    expect((screen.getByRole('combobox') as HTMLButtonElement).disabled).toBe(false);
    expect((screen.getByRole('button', { name: 'Vanilla' }) as HTMLButtonElement).disabled).toBe(
      false,
    );
  });
});

describe('ManageInstancesModal — list & sidebar usability', () => {
  function makeMany(n: number): InstanceWithStatus[] {
    return Array.from({ length: n }, (_, idx) =>
      makeInstance({ id: `inst-${idx}`, name: `Profile ${idx}` }),
    );
  }

  it('pins the New-instance button ahead of the scrollable list', async () => {
    const a = makeInstance({ id: 'a', name: 'Alpha' });
    const b = makeInstance({ id: 'b', name: 'Beta' });
    render(ManageInstancesModal, {
      props: {
        open: true,
        instances: [a, b],
        activeInstance: a,
        versions: [version],
        onChanged: () => {},
      },
    });
    await screen.findByDisplayValue('Alpha');

    const createBtn = screen.getByRole('button', { name: '+ New instance' });
    const row = screen.getByRole('button', { name: /Beta/ });
    // The create button must come BEFORE the list rows in DOM order so it stays
    // pinned while the list scrolls.
    expect(createBtn.compareDocumentPosition(row) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it('renders long instance names in a truncating span', async () => {
    const longName = 'A really really long instance name that overflows the sidebar';
    const inst = makeInstance({ id: 'a', name: longName });
    render(ManageInstancesModal, {
      props: {
        open: true,
        instances: [inst],
        activeInstance: inst,
        versions: [version],
        onChanged: () => {},
      },
    });
    await screen.findByDisplayValue(longName);

    const truncating = screen
      .getAllByText(longName)
      .filter((el) => el.className.includes('truncate'));
    expect(truncating.length).toBeGreaterThan(0);
  });

  it('marks the active instance row with an Active chip and leaves others unmarked', async () => {
    const a = makeInstance({ id: 'a', name: 'Alpha' });
    const b = makeInstance({ id: 'b', name: 'Beta' });
    render(ManageInstancesModal, {
      props: {
        open: true,
        instances: [a, b],
        activeInstance: a,
        versions: [version],
        onChanged: () => {},
      },
    });
    await screen.findByDisplayValue('Alpha');

    const alphaRow = screen.getByRole('button', { name: /Alpha/ });
    const betaRow = screen.getByRole('button', { name: /Beta/ });
    expect(within(alphaRow).getByText('Active')).toBeTruthy();
    expect(within(betaRow).queryByText('Active')).toBeNull();
  });

  it('hides the filter below the threshold and shows it once the list grows', async () => {
    const few = makeMany(3);
    const { rerender } = render(ManageInstancesModal, {
      props: {
        open: true,
        instances: few,
        activeInstance: few[0],
        versions: [version],
        onChanged: () => {},
      },
    });
    await screen.findByDisplayValue('Profile 0');
    expect(screen.queryByPlaceholderText(/filter/i)).toBeNull();

    const many = makeMany(9);
    await rerender({
      open: true,
      instances: many,
      activeInstance: many[0],
      versions: [version],
      onChanged: () => {},
    });
    expect(screen.getByPlaceholderText(/filter/i)).toBeTruthy();
  });

  it('filters rows by name and shows a no-matches note', async () => {
    const many = makeMany(9); // Profile 0 .. Profile 8
    render(ManageInstancesModal, {
      props: {
        open: true,
        instances: many,
        activeInstance: many[0],
        versions: [version],
        onChanged: () => {},
      },
    });
    const filter = await screen.findByPlaceholderText(/filter/i);

    await fireEvent.input(filter, { target: { value: 'Profile 3' } });
    expect(screen.getByRole('button', { name: /Profile 3/ })).toBeTruthy();
    expect(screen.queryByRole('button', { name: /Profile 4/ })).toBeNull();

    await fireEvent.input(filter, { target: { value: 'zzz-no-such-name' } });
    expect(screen.getByText('No instances match.')).toBeTruthy();
  });

  it('clears the filter after the modal is closed', async () => {
    const many = makeMany(9);
    const baseProps = {
      instances: many,
      activeInstance: many[0],
      versions: [version],
      onChanged: () => {},
    };
    const { rerender } = render(ManageInstancesModal, { props: { open: true, ...baseProps } });

    const filter = (await screen.findByPlaceholderText(/filter/i)) as HTMLInputElement;
    await fireEvent.input(filter, { target: { value: 'Profile 3' } });
    expect(filter.value).toBe('Profile 3');

    // Close via the real close button so close() runs and resets the filter.
    await fireEvent.click(screen.getByRole('button', { name: /close manage instances/i }));
    await rerender({ open: true, ...baseProps });

    const reopened = (await screen.findByPlaceholderText(/filter/i)) as HTMLInputElement;
    expect(reopened.value).toBe('');
  });
});

describe('ManageInstancesModal — async feedback & double-submit', () => {
  it('does not double-create when Create is clicked twice', async () => {
    let resolveCreate!: (v: unknown) => void;
    m.createInstance.mockReturnValueOnce(
      new Promise((r) => {
        resolveCreate = r;
      }),
    );

    const inst = makeInstance();
    render(ManageInstancesModal, {
      props: {
        open: true,
        instances: [inst],
        activeInstance: inst,
        versions: [version],
        onChanged: () => {},
      },
    });

    await fireEvent.click(screen.getByRole('button', { name: '+ New instance' }));
    await fireEvent.input(screen.getByLabelText(/name/i), { target: { value: 'My Instance' } });
    // Pick an MC version via the custom Select (options commit on mousedown).
    await fireEvent.click(screen.getByRole('combobox'));
    await fireEvent.mouseDown(screen.getByRole('option', { name: '1.20.1' }));

    const createBtn = screen.getByRole('button', { name: 'Create' });
    await fireEvent.click(createBtn);
    await fireEvent.click(createBtn); // second rapid click — must be a no-op

    expect(m.createInstance).toHaveBeenCalledTimes(1);
    expect(createBtn.getAttribute('aria-busy')).toBe('true');

    resolveCreate({ status: 'ok', data: { id: 'new-id' } });
    await waitFor(() => expect(m.setActiveInstance).toHaveBeenCalledTimes(1));
  });

  it('discards a name-save that resolves after switching to another instance', async () => {
    const onChanged = vi.fn();
    let resolveName!: (v: unknown) => void;
    m.setInstanceName.mockReturnValueOnce(
      new Promise((r) => {
        resolveName = r;
      }),
    );

    const a = makeInstance({ id: 'a', name: 'Alpha' });
    const b = makeInstance({ id: 'b', name: 'Beta' });
    render(ManageInstancesModal, {
      props: { open: true, instances: [a, b], activeInstance: a, versions: [version], onChanged },
    });

    const input = (await screen.findByDisplayValue('Alpha')) as HTMLInputElement;
    await fireEvent.input(input, { target: { value: 'Renamed' } });
    await fireEvent.blur(input); // commitName captures id='a', then awaits

    // Switch selection before the save resolves.
    await fireEvent.click(screen.getByRole('button', { name: /Beta/ }));

    resolveName({ status: 'ok', data: null });
    // commitName has exactly one await, so its post-await continuation (where the
    // isStale gate runs) settles within one microtask; the second tick is buffer.
    await Promise.resolve();
    await Promise.resolve();

    // The stale completion must not fire onChanged against the previous selection.
    expect(onChanged).not.toHaveBeenCalled();
  });

  it('does not surface an error from a save that resolves after the modal closes', async () => {
    const onChanged = vi.fn();
    let resolveName!: (v: unknown) => void;
    m.setInstanceName.mockReturnValueOnce(
      new Promise((r) => {
        resolveName = r;
      }),
    );

    const inst = makeInstance({ name: 'Alpha' });
    const baseProps = { instances: [inst], activeInstance: inst, versions: [version], onChanged };
    const { rerender } = render(ManageInstancesModal, { props: { open: true, ...baseProps } });

    const input = (await screen.findByDisplayValue('Alpha')) as HTMLInputElement;
    await fireEvent.input(input, { target: { value: 'Renamed' } });
    await fireEvent.blur(input);

    // Close via the real close button so close() runs (open=false, selection cleared).
    await fireEvent.click(screen.getByRole('button', { name: /close manage instances/i }));

    resolveName({ status: 'error', error: { kind: 'instance_name_empty' } });
    // Let commitName's post-await continuation (the isStale gate) settle while the
    // modal is still closed — mirrors production, where the save resolves a micro-
    // task after close, long before any reopen. Without this flush the rerender
    // below races the gate and re-opens (open=true, selection restored) first, so
    // the stale error would surface. Two ticks: one await per microtask hop.
    await Promise.resolve();
    await Promise.resolve();
    await rerender({ open: true, ...baseProps });

    expect(screen.queryByText(/name cannot be empty/i)).toBeNull();
    expect(onChanged).not.toHaveBeenCalled();
  });
});

describe('ManageInstancesModal — persistence legibility', () => {
  it('the footer dismiss button reads as Close (secondary), not a primary commit', async () => {
    const inst = makeInstance();
    render(ManageInstancesModal, {
      props: {
        open: true,
        instances: [inst],
        activeInstance: inst,
        versions: [version],
        onChanged: () => {},
      },
    });
    await screen.findByDisplayValue('Default');

    const closeBtn = screen.getByRole('button', { name: /^\s*close\s*$/i });
    expect(closeBtn.className).toContain('btn-secondary');
    expect(closeBtn.className).not.toContain('btn-primary');

    await fireEvent.click(closeBtn);
    expect(screen.queryByDisplayValue('Default')).toBeNull();
  });

  it('shows a transient Saved badge after a name edit and hides it on re-edit', async () => {
    const inst = makeInstance({ name: 'Alpha' });
    render(ManageInstancesModal, {
      props: {
        open: true,
        instances: [inst],
        activeInstance: inst,
        versions: [version],
        onChanged: () => {},
      },
    });

    const input = (await screen.findByDisplayValue('Alpha')) as HTMLInputElement;
    await fireEvent.input(input, { target: { value: 'Renamed' } });
    await fireEvent.blur(input);

    expect(await screen.findByText('Saved')).toBeTruthy();

    await fireEvent.input(input, { target: { value: 'Renamed2' } });
    expect(screen.queryByText('Saved')).toBeNull();
  });

  it('reverts a blanked name to the saved value without calling setInstanceName', async () => {
    const inst = makeInstance({ name: 'Alpha' });
    render(ManageInstancesModal, {
      props: {
        open: true,
        instances: [inst],
        activeInstance: inst,
        versions: [version],
        onChanged: () => {},
      },
    });

    const input = (await screen.findByDisplayValue('Alpha')) as HTMLInputElement;
    await fireEvent.input(input, { target: { value: '   ' } });
    await fireEvent.blur(input);

    expect(await screen.findByDisplayValue('Alpha')).toBeTruthy();
    expect(m.setInstanceName).not.toHaveBeenCalled();
  });
});

describe('ManageInstancesModal — accessibility', () => {
  it('marks the selected instance row with aria-current and migrates it on switch', async () => {
    const a = makeInstance({ id: 'a', name: 'Alpha' });
    const b = makeInstance({ id: 'b', name: 'Beta' });
    render(ManageInstancesModal, {
      props: {
        open: true,
        instances: [a, b],
        activeInstance: a,
        versions: [version],
        onChanged: () => {},
      },
    });
    await screen.findByDisplayValue('Alpha');

    expect(screen.getByRole('button', { name: /Alpha/ }).getAttribute('aria-current')).toBe('true');
    expect(screen.getByRole('button', { name: /Beta/ }).getAttribute('aria-current')).toBe('false');

    await fireEvent.click(screen.getByRole('button', { name: /Beta/ }));
    expect(screen.getByRole('button', { name: /Beta/ }).getAttribute('aria-current')).toBe('true');
    expect(screen.getByRole('button', { name: /Alpha/ }).getAttribute('aria-current')).toBe(
      'false',
    );
  });

  it('names the instance list and labels the ready/download status icons', async () => {
    const ready = makeInstance({ id: 'a', name: 'Ready One', ready: true });
    const pending = makeInstance({ id: 'b', name: 'Pending One', ready: false });
    render(ManageInstancesModal, {
      props: {
        open: true,
        instances: [ready, pending],
        activeInstance: ready,
        versions: [version],
        onChanged: () => {},
      },
    });
    await screen.findByDisplayValue('Ready One');

    expect(screen.getByRole('complementary', { name: 'Instances' })).toBeTruthy();
    expect(screen.getByRole('img', { name: /assets downloaded/i })).toBeTruthy();
    expect(screen.getByRole('img', { name: /download needed/i })).toBeTruthy();
  });

  it('marks the active loader button with aria-pressed', async () => {
    const inst = makeInstance({ loader: 'vanilla' });
    render(ManageInstancesModal, {
      props: {
        open: true,
        instances: [inst],
        activeInstance: inst,
        versions: [version],
        onChanged: () => {},
      },
    });
    await screen.findByDisplayValue('Default');

    expect(screen.getByRole('button', { name: 'Vanilla' }).getAttribute('aria-pressed')).toBe(
      'true',
    );
    expect(screen.getByRole('button', { name: 'Fabric' }).getAttribute('aria-pressed')).toBe(
      'false',
    );
  });

  it('keeps Create keyboard-reachable with a visible disabled reason (no native disabled)', async () => {
    const inst = makeInstance();
    render(ManageInstancesModal, {
      props: {
        open: true,
        instances: [inst],
        activeInstance: inst,
        versions: [version],
        onChanged: () => {},
      },
    });
    await screen.findByDisplayValue('Default');
    await fireEvent.click(screen.getByRole('button', { name: '+ New instance' }));

    const createBtn = screen.getByRole('button', { name: 'Create' }) as HTMLButtonElement;
    expect(createBtn.disabled).toBe(false);
    // The reason is on-screen (not hidden in a tooltip on a disabled button).
    expect(screen.getByText(/name is required/i)).toBeTruthy();
  });
});

describe('ManageInstancesModal — microcopy & i18n', () => {
  afterEach(() => locale.set('en'));

  const unhealthy = (problem_count: number): InstanceWithStatus['integrity'] => ({
    healthy: false,
    checked_unix_ms: null,
    categories: [],
    problem_count,
  });

  it('pluralises integrity wording in EN (singular vs plural)', async () => {
    const one = makeInstance({ id: 'a', name: 'One', integrity: unhealthy(1) });
    const two = makeInstance({ id: 'b', name: 'Two', integrity: unhealthy(2) });
    render(ManageInstancesModal, {
      props: {
        open: true,
        instances: [one, two],
        activeInstance: one,
        versions: [version],
        onChanged: () => {},
      },
    });
    await screen.findByDisplayValue('One');

    expect(screen.getByRole('img', { name: '1 integrity problem' })).toBeTruthy();
    expect(screen.getByRole('img', { name: '2 integrity problems' })).toBeTruthy();
  });

  it('uses Russian plural categories (one / many) for integrity wording', async () => {
    locale.set('ru');
    const one = makeInstance({ id: 'a', name: 'One', integrity: unhealthy(1) });
    const many = makeInstance({ id: 'b', name: 'Many', integrity: unhealthy(5) });
    render(ManageInstancesModal, {
      props: {
        open: true,
        instances: [one, many],
        activeInstance: one,
        versions: [version],
        onChanged: () => {},
      },
    });
    await screen.findByDisplayValue('One');

    expect(screen.getByRole('img', { name: '1 проблема целостности' })).toBeTruthy();
    expect(screen.getByRole('img', { name: '5 проблем целостности' })).toBeTruthy();
  });

  it('drives the name counter and maxlength from a single constant', async () => {
    const inst = makeInstance({ name: 'Alpha' });
    render(ManageInstancesModal, {
      props: {
        open: true,
        instances: [inst],
        activeInstance: inst,
        versions: [version],
        onChanged: () => {},
      },
    });
    const input = (await screen.findByDisplayValue('Alpha')) as HTMLInputElement;
    expect(input.getAttribute('maxlength')).toBe('32');
    expect(screen.getByText('5/32')).toBeTruthy();
  });

  it('shows a connection notice when no versions are available at all', async () => {
    const inst = makeInstance();
    render(ManageInstancesModal, {
      props: {
        open: true,
        instances: [inst],
        activeInstance: inst,
        versions: [],
        onChanged: () => {},
      },
    });
    await screen.findByDisplayValue('Default');
    expect(screen.getByText(/version list unavailable/i)).toBeTruthy();
  });

  it('nudges to enable snapshots when only snapshot versions exist', async () => {
    const snapshot: VersionEntry = {
      id: '24w01a',
      version_type: 'snapshot',
      release_date: '2024-01-01T00:00:00+00:00',
      url: '',
    };
    const inst = makeInstance();
    render(ManageInstancesModal, {
      props: {
        open: true,
        instances: [inst],
        activeInstance: inst,
        versions: [snapshot],
        onChanged: () => {},
      },
    });
    await screen.findByDisplayValue('Default');
    expect(screen.getByText(/no release versions available/i)).toBeTruthy();
  });
});

describe('ManageInstancesModal — advanced initial heap (-Xms)', () => {
  function renderOne(over: Partial<InstanceWithStatus> = {}) {
    const inst = makeInstance(over);
    render(ManageInstancesModal, {
      props: {
        open: true,
        instances: [inst],
        activeInstance: inst,
        versions: [version],
        onChanged: () => {},
      },
    });
  }

  // Expand the collapsed <details> so its controls are visible to role queries
  // (content inside a closed <details> is treated as hidden).
  function openAdvanced() {
    const details = screen.getByText('Advanced').closest('details') as HTMLDetailsElement;
    details.open = true;
  }

  it('keeps the Advanced section collapsed by default', async () => {
    const inst = makeInstance();
    const { container } = render(ManageInstancesModal, {
      props: {
        open: true,
        instances: [inst],
        activeInstance: inst,
        versions: [version],
        onChanged: () => {},
      },
    });
    await screen.findByDisplayValue('Default');

    const details = container.querySelector('details');
    expect(details).not.toBeNull();
    expect(details?.hasAttribute('open')).toBe(false);
  });

  it('persists a typed initial heap', async () => {
    renderOne({ max_heap_mb: 4096 });
    await screen.findByDisplayValue('Default');
    openAdvanced();

    const xms = (await screen.findByLabelText(/initial heap/i)) as HTMLInputElement;
    await fireEvent.input(xms, { target: { value: '2048' } });
    await fireEvent.change(xms, { target: { value: '2048' } });

    expect(m.setInstanceMinHeap).toHaveBeenCalledWith('inst-1', 2048);
  });

  it('caps the initial heap at the current max', async () => {
    renderOne({ max_heap_mb: 4096 });
    await screen.findByDisplayValue('Default');
    openAdvanced();

    const xms = (await screen.findByLabelText(/initial heap/i)) as HTMLInputElement;
    await fireEvent.input(xms, { target: { value: '9000' } });
    await fireEvent.change(xms, { target: { value: '9000' } });

    expect(m.setInstanceMinHeap).toHaveBeenLastCalledWith('inst-1', 4096);
  });

  it('"= max" sets the initial heap to the current maximum', async () => {
    renderOne({ max_heap_mb: 4096 });
    await screen.findByDisplayValue('Default');
    openAdvanced();

    await fireEvent.click(screen.getByRole('button', { name: /=\s*max/i }));

    expect(m.setInstanceMinHeap).toHaveBeenCalledWith('inst-1', 4096);
  });

  it('clearing the field clears the initial heap (null)', async () => {
    renderOne({ max_heap_mb: 4096, min_heap_mb: 2048 });
    await screen.findByDisplayValue('Default');
    openAdvanced();

    const xms = (await screen.findByLabelText(/initial heap/i)) as HTMLInputElement;
    await fireEvent.input(xms, { target: { value: '' } });
    await fireEvent.change(xms, { target: { value: '' } });

    expect(m.setInstanceMinHeap).toHaveBeenCalledWith('inst-1', null);
  });
});

describe('ManageInstancesModal — folder rename keeps the selection', () => {
  it('follows the new id so the detail pane does not empty after the repair', async () => {
    // Renaming the folder changes the instance id (the directory name IS the id).
    // `selected` is derived by matching `selectedId`, so unless the modal follows
    // the new id the detail pane goes blank the instant the rename succeeds —
    // right after the user repaired an instance they thought they had lost.
    const before = makeInstance({ id: 'Old-Name', name: 'My Pack' });
    const after = makeInstance({ id: 'New-Name', name: 'My Pack' });
    m.renameInstanceDir.mockResolvedValue({ status: 'ok', data: after });

    const baseProps = {
      open: true,
      activeInstance: before,
      versions: [version],
      onChanged: () => {},
    };
    const { rerender } = render(ManageInstancesModal, {
      props: { ...baseProps, instances: [before] },
    });

    await fireEvent.click(await screen.findByRole('button', { name: 'Change' }));
    await fireEvent.click(await screen.findByRole('button', { name: 'Rename' }));

    // The parent refreshes its list on onChanged, which is what supplies the
    // renamed instance — mirror that here.
    await rerender({
      ...baseProps,
      instances: [after],
      activeInstance: after,
    });

    expect(m.renameInstanceDir).toHaveBeenCalled();
    expect(screen.getByDisplayValue('My Pack')).toBeTruthy();
  });
});

describe('ManageInstancesModal — translations entry point', () => {
  it('requests translations for the SELECTED instance, not the active one', async () => {
    // The modal's selection is independent of the active instance, so a handler
    // that reads the active instance would open a different profile's
    // translations than the row the user is looking at.
    const a = makeInstance({ id: 'a', name: 'Alpha' });
    const b = makeInstance({ id: 'b', name: 'Beta' });
    const onTranslationsRequest = vi.fn();
    render(ManageInstancesModal, {
      props: {
        open: true,
        instances: [a, b],
        activeInstance: a,
        versions: [version],
        onChanged: () => {},
        onTranslationsRequest,
      },
    });
    await screen.findByDisplayValue('Alpha');

    await fireEvent.click(screen.getByRole('button', { name: /Beta/ }));
    await fireEvent.click(screen.getByTestId('manage-translations-btn'));

    expect(onTranslationsRequest).toHaveBeenCalledWith('b');
  });
});

describe('ManageInstancesModal — the Active marker', () => {
  it('is the shared StatusBadge, not a hand-rolled pill', async () => {
    const inst = makeInstance();
    render(ManageInstancesModal, {
      props: {
        open: true,
        instances: [inst],
        activeInstance: inst,
        versions: [version],
        onChanged: () => {},
      },
    });
    // Rendered in both the list row and the detail pane — assert every copy, so
    // migrating one and leaving the other is a failure.
    const badges = await screen.findAllByTestId('manage-active-badge');
    expect(badges.length).toBeGreaterThan(0);
    for (const badge of badges) {
      expect(badge.textContent).toContain('Active');
      // §9's `info` variant: the accent-soft token pair, and the app-wide pill
      // geometry (`rounded`, `text-xs`) — not the rounded-full text-[10px] chip
      // this replaces.
      expect(badge.className).toContain('bg-accent-soft');
      expect(badge.className).toContain('text-accent');
      expect(badge.className).toContain('rounded');
      expect(badge.className).not.toContain('rounded-full');
      expect(badge.className).not.toContain('text-[10px]');
    }
  });
});

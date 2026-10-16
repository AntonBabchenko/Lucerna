import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import type {
  InstanceWithStatus,
  ItemPreview,
  OtherPreview,
  TransferPreview,
} from '$lib/ipc/bindings';

// TransferDataDialog (spec 2026-10-15 §A1): the source picker, the rows the
// preview lists, the conflict choice, the space warning, and the hand-off to
// the op queue. Real i18n and the real task registry are used.

const m = vi.hoisted(() => ({
  transferScan: vi.fn(),
  runningInstances: vi.fn(),
  enqueueTransfer: vi.fn(),
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    transferScan: m.transferScan,
    runningInstances: m.runningInstances,
  },
  events: {
    processSpawned: { listen: () => Promise.resolve(() => {}) },
    processExited: { listen: () => Promise.resolve(() => {}) },
  },
}));

vi.mock('$lib/ops/op-queue.svelte', () => ({
  enqueueTransfer: m.enqueueTransfer,
}));

import TransferDataDialog from '$lib/instances/transfer/TransferDataDialog.svelte';
import { __resetTasksForTest } from '$lib/tasks/registry.svelte';

function instance(over: Partial<InstanceWithStatus> = {}): InstanceWithStatus {
  return {
    id: 'inst-1',
    name: 'New pack',
    mc_version: '1.21.1',
    loader: 'fabric',
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

const target = instance();
const old = instance({ id: 'inst-2', name: 'Old pack', mc_version: '1.20.1' });
const other = instance({ id: 'inst-3', name: 'Other', loader: 'forge' });

function item(id: ItemPreview['id'], over: Partial<ItemPreview> = {}): ItemPreview {
  return {
    id,
    roots: [{ rel: id, source: { kind: 'local' }, target: null }],
    files: 2,
    bytes: 2048,
    conflicts: 0,
    skipped: 0,
    refusal: null,
    ...over,
  };
}

function folder(name: string, over: Partial<OtherPreview> = {}): OtherPreview {
  return {
    name,
    root: { rel: name, source: { kind: 'local' }, target: null },
    files: 1,
    bytes: 100,
    conflicts: 0,
    skipped: 0,
    refusal: null,
    ...over,
  };
}

function scan(preview: Partial<TransferPreview>) {
  m.transferScan.mockResolvedValue({
    status: 'ok',
    data: { items: [], other: [], target_free_mb: null, ...preview },
  });
}

function renderDialog(onClose = vi.fn()) {
  return render(TransferDataDialog, {
    props: { target, instances: [target, old, other], onClose },
  });
}

async function pickSource(name: RegExp) {
  await fireEvent.click(screen.getByRole('combobox'));
  await fireEvent.mouseDown(screen.getByRole('option', { name }));
  await waitFor(() => expect(m.transferScan).toHaveBeenCalled());
}

beforeEach(() => {
  locale.set('en');
  vi.clearAllMocks();
  __resetTasksForTest();
  m.runningInstances.mockResolvedValue([]);
});

describe('TransferDataDialog', () => {
  it('offers every other instance as a source, never the target itself', async () => {
    renderDialog();
    await fireEvent.click(screen.getByRole('combobox'));
    expect(screen.getByRole('option', { name: /Old pack · Fabric 1\.20\.1/ })).toBeTruthy();
    expect(screen.getByRole('option', { name: /Other · Forge/ })).toBeTruthy();
    expect(screen.queryByRole('option', { name: /New pack/ })).toBeNull();
  });

  it('lists only what the source has: catalog checked, other folders unchecked', async () => {
    scan({ items: [item('schematics')], other: [folder('kubejs')] });
    renderDialog();
    await pickSource(/Old pack/);
    const schematics = (await screen.findByTestId('transfer-item-schematics')) as HTMLInputElement;
    expect(schematics.checked).toBe(true);
    expect((screen.getByTestId('transfer-other-kubejs') as HTMLInputElement).checked).toBe(false);
    expect(screen.queryByTestId('transfer-item-xaero')).toBeNull();
    expect(m.transferScan).toHaveBeenCalledWith('inst-2', 'inst-1');
  });

  it('asks about same-named files only when a checked row has them, defaulting to replace', async () => {
    scan({ items: [item('schematics', { conflicts: 1 })] });
    renderDialog();
    await pickSource(/Old pack/);
    const replace = (await screen.findByTestId('transfer-conflict-replace')) as HTMLInputElement;
    expect(replace.checked).toBe(true);
    await fireEvent.click(screen.getByTestId('transfer-item-schematics'));
    expect(screen.queryByTestId('transfer-conflict-replace')).toBeNull();
  });

  it('a refused row cannot be checked and says why', async () => {
    scan({ items: [item('schematics', { refusal: { kind: 'overlap', rel: 'schematics' } })] });
    renderDialog();
    await pickSource(/Old pack/);
    const box = (await screen.findByTestId('transfer-item-schematics')) as HTMLInputElement;
    expect(box.disabled).toBe(true);
    expect(box.checked).toBe(false);
    expect(screen.getByTestId('transfer-item-schematics-refusal').textContent).toContain(
      'schematics',
    );
    expect((screen.getByTestId('transfer-submit') as HTMLButtonElement).disabled).toBe(true);
  });

  it('Copy enqueues the checked rows with the conflict choice and closes', async () => {
    scan({ items: [item('schematics', { conflicts: 2 })], other: [folder('kubejs')] });
    const onClose = vi.fn();
    renderDialog(onClose);
    await pickSource(/Old pack/);
    await fireEvent.click(await screen.findByTestId('transfer-other-kubejs'));
    await fireEvent.click(screen.getByTestId('transfer-conflict-keep'));
    await fireEvent.click(screen.getByTestId('transfer-submit'));
    expect(m.enqueueTransfer).toHaveBeenCalledWith({
      sourceId: 'inst-2',
      sourceName: 'Old pack',
      targetId: 'inst-1',
      targetName: 'New pack',
      selection: { items: ['schematics'], other: ['kubejs'] },
      onConflict: 'keep_existing',
    });
    expect(onClose).toHaveBeenCalled();
  });

  it('a running source is listed but cannot be picked', async () => {
    m.runningInstances.mockResolvedValue([{ instance_id: 'inst-2' }]);
    renderDialog();
    await waitFor(() => expect(m.runningInstances).toHaveBeenCalled());
    await fireEvent.click(screen.getByRole('combobox'));
    await waitFor(() =>
      expect(screen.getByRole('option', { name: /Old pack/ }).getAttribute('aria-disabled')).toBe(
        'true',
      ),
    );
  });

  it('warns, without blocking, when the selection is larger than the free space', async () => {
    scan({ items: [item('schematics', { bytes: 3 * 1024 * 1024 })], target_free_mb: 1 });
    renderDialog();
    await pickSource(/Old pack/);
    expect(await screen.findByTestId('transfer-space-warning')).toBeTruthy();
    expect((screen.getByTestId('transfer-submit') as HTMLButtonElement).disabled).toBe(false);
  });

  it('shows the version caution once, above the rows it applies to', async () => {
    scan({ items: [item('xaero'), item('bobby'), item('schematics')] });
    renderDialog();
    await pickSource(/Old pack/);
    await screen.findByTestId('transfer-item-xaero');
    expect(screen.getAllByTestId('transfer-version-caution')).toHaveLength(1);
    expect(screen.getAllByText(/newer version of a mod/)).toHaveLength(1);
  });

  it('has no version caution when only schematics are listed', async () => {
    scan({ items: [item('schematics')] });
    renderDialog();
    await pickSource(/Old pack/);
    await screen.findByTestId('transfer-item-schematics');
    expect(screen.queryByTestId('transfer-version-caution')).toBeNull();
  });

  it('says so when the source has nothing to copy', async () => {
    scan({});
    renderDialog();
    await pickSource(/Old pack/);
    expect(await screen.findByTestId('transfer-nothing')).toBeTruthy();
  });
});

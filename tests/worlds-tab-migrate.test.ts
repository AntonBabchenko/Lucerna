// What the Worlds tab does once a migration lands (world-migration spec §7
// "Completion", §8 "Not errors"): the source list and the parent's Play menu
// are refreshed, a moved world's detail dialog is closed, and the completion
// toast says only what the outcome actually reports.
import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { get } from 'svelte/store';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { t } from '$lib/i18n';
import type { InstanceWithStatus, MigrationOutcome, World } from '$lib/ipc/bindings';
import { markSeen } from '$lib/onboarding/contextual-tours';
import { buildMigrationToast } from '$lib/worlds/migrate-toast';
import WorldsTab from '$lib/worlds/WorldsTab.svelte';

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    listWorlds: vi.fn(),
    // WorldsTab queries these alongside the world list; without them the
    // component rejects and vitest reports unhandled errors even though the
    // assertions still pass.
    listOrphanedBackupWorlds: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    listStrandedWorlds: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    recoverStrandedWorld: vi.fn(),
    // The detail dialog's Backups tab lists on mount.
    listBackups: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    backupWorld: vi.fn(),
    deleteBackup: vi.fn(),
    openSavesFolder: vi.fn(),
    openBackupsFolder: vi.fn(),
    datapacksListForWorld: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    datapacksListLibrary: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  },
  events: {
    processExited: { listen: vi.fn().mockResolvedValue(() => {}) },
    processSpawned: { listen: vi.fn().mockResolvedValue(() => {}) },
  },
}));

// The real MigrateWorldDialog fetches a plan and runs the task; this file is
// about what WorldsTab does AFTER the dialog reports `onDone`. Svelte 5 calls
// a component as `(anchor, props)`, so the stub is a function that renders
// nothing and keeps the props it was handed (same trick as the noop stubs in
// tests/button-intents-servers-mode.test.ts, plus the capture).
const stub = vi.hoisted(() => ({ props: null as Record<string, unknown> | null }));
vi.mock('$lib/worlds/MigrateWorldDialog.svelte', () => ({
  default: function stubMigrateWorldDialog(_anchor: unknown, props: Record<string, unknown>) {
    stub.props = props;
    return {};
  },
}));

const toasts = vi.hoisted(() => ({
  pushSuccess: vi.fn(),
  pushWarning: vi.fn(),
  pushInfo: vi.fn(),
}));
vi.mock('$lib/toasts/toasts.svelte', () => toasts);

const MY_WORLD: World = {
  folder_name: 'My World',
  size_bytes: 1024,
  modified_unix_ms: 1,
  backup_count: 0,
};
const OTHER: World = {
  folder_name: 'Other World',
  size_bytes: 2048,
  modified_unix_ms: 1,
  backup_count: 0,
};

function instance(over: Partial<InstanceWithStatus>): InstanceWithStatus {
  return {
    id: 'src',
    name: 'Source',
    mc_version: '1.21.1',
    loader: 'fabric',
    loader_version: '0.16.0',
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
  };
}

function outcome(over: Partial<MigrationOutcome> = {}): MigrationOutcome {
  return {
    final_folder_name: 'My World',
    path: 'renamed',
    datapacks: [],
    datapacks_folders_copied: 0,
    links_skipped: 0,
    source_state: { kind: 'removed' },
    backups_moved: 0,
    backups_left: 0,
    ...over,
  };
}

type Done = (r: { mode: 'copy' | 'move'; outcome: MigrationOutcome; targetName: string }) => void;

/** Open the detail dialog for the first row, click its migrate action, and
 *  return the captured `onDone` of the (stubbed) migrate dialog. */
async function openMigrate(container: HTMLElement): Promise<Done> {
  await screen.findByText('My World');
  await fireEvent.click(container.querySelector('[data-testid="world-row"]') as HTMLElement);
  await screen.findByTestId('world-detail-dialog');
  await fireEvent.click(await screen.findByTestId('world-migrate-btn'));
  await waitFor(() => expect(stub.props).not.toBeNull());
  return stub.props?.onDone as Done;
}

beforeEach(() => markSeen('worlds'));
afterEach(() => {
  vi.clearAllMocks();
  stub.props = null;
});

describe('WorldsTab — after a migration outcome', () => {
  it('a Move drops the world from the list, closes its detail dialog and tells the parent', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.listWorlds)
      .mockResolvedValueOnce({ status: 'ok', data: [MY_WORLD, OTHER] })
      .mockResolvedValue({ status: 'ok', data: [OTHER] });
    const onWorldsChanged = vi.fn();
    const { container } = render(WorldsTab, {
      props: {
        instanceId: 'src',
        instanceName: 'Source',
        instances: [
          instance({ id: 'src', name: 'Source' }),
          instance({ id: 'dst', name: 'Target' }),
        ],
        onListChanged: () => {},
        onWorldsChanged,
      },
    });
    const onDone = await openMigrate(container);
    // The dialog was handed the source by display NAME and the clicked world.
    expect(stub.props?.instanceName).toBe('Source');
    expect((stub.props?.world as World).folder_name).toBe('My World');

    onDone({ mode: 'move', outcome: outcome(), targetName: 'Target' });

    await waitFor(() => expect(screen.queryByText('My World')).toBeNull());
    expect(screen.getByText('Other World')).toBeTruthy();
    expect(screen.queryByTestId('world-detail-dialog')).toBeNull();
    await waitFor(() => expect(onWorldsChanged).toHaveBeenCalledTimes(1));
    expect(commands.listWorlds).toHaveBeenCalledTimes(2);
    expect(toasts.pushSuccess).toHaveBeenCalledTimes(1);
    expect(toasts.pushWarning).not.toHaveBeenCalled();
    const [title, lines] = toasts.pushSuccess.mock.calls[0] as [string, string[]];
    expect(title).toContain('My World');
    expect(lines.join('\n')).toContain('Target');
  });

  it('a Copy keeps the world and its open detail dialog, and still refreshes both lists', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.listWorlds).mockResolvedValue({ status: 'ok', data: [MY_WORLD, OTHER] });
    const onWorldsChanged = vi.fn();
    const { container } = render(WorldsTab, {
      props: {
        instanceId: 'src',
        instanceName: 'Source',
        instances: [
          instance({ id: 'src', name: 'Source' }),
          instance({ id: 'dst', name: 'Target' }),
        ],
        onListChanged: () => {},
        onWorldsChanged,
      },
    });
    const onDone = await openMigrate(container);

    onDone({
      mode: 'copy',
      outcome: outcome({ path: 'copied', source_state: { kind: 'untouched' } }),
      targetName: 'Target',
    });

    await waitFor(() => expect(onWorldsChanged).toHaveBeenCalledTimes(1));
    expect(commands.listWorlds).toHaveBeenCalledTimes(2);
    expect(screen.getByTestId('world-detail-dialog')).toBeTruthy();
    expect(screen.getAllByText('My World').length).toBeGreaterThan(0);
    expect(toasts.pushSuccess).toHaveBeenCalledTimes(1);
  });
});

describe('buildMigrationToast — says only what happened', () => {
  const tr = get(t);
  const base = { sourceWorld: 'My World', sourceName: 'Source', targetName: 'Target' };

  it('names a datapack left as a copy together with the translated reason, and only that one', () => {
    const r = buildMigrationToast(tr, {
      ...base,
      mode: 'copy',
      outcome: outcome({
        path: 'copied',
        source_state: { kind: 'untouched' },
        datapacks: [
          {
            filename: 'terralith.zip',
            result: { kind: 'left_as_copy', reason: { kind: 'name_held_by_different_pack' } },
          },
          { filename: 'tectonic.zip', result: { kind: 'linked' } },
        ],
      }),
    });
    expect(r.kind).toBe('warning');
    const text = r.lines.join('\n');
    expect(text).toContain('terralith.zip');
    expect(text).toContain(tr('worlds.migrate.leftReason.nameHeldByDifferentPack'));
    expect(text).not.toContain('tectonic.zip');
  });

  it('names the resource-pack case exactly, not an umbrella "not a datapack"', () => {
    // `NotADatapack` carries a typed `DatapackRejection`; the toast is where
    // the user learns which one, because "it is a resource pack" is the case
    // they can act on and the same sentence the library's own error shows.
    const r = buildMigrationToast(tr, {
      ...base,
      mode: 'copy',
      outcome: outcome({
        path: 'copied',
        source_state: { kind: 'untouched' },
        datapacks: [
          {
            filename: 'faithful.zip',
            result: {
              kind: 'left_as_copy',
              reason: { kind: 'not_a_datapack', reason: 'is_a_resource_pack' },
            },
          },
        ],
      }),
    });
    expect(r.kind).toBe('warning');
    const text = r.lines.join('\n');
    expect(text).toContain('faithful.zip');
    expect(text).toContain(tr('errors.datapackInvalidReason.isAResourcePack'));
    expect(text).toMatch(/resource pack/i);
  });

  it('a copy where every pack linked or was adopted is a success with no failure wording', () => {
    const r = buildMigrationToast(tr, {
      ...base,
      mode: 'copy',
      outcome: outcome({
        path: 'copied',
        source_state: { kind: 'untouched' },
        datapacks: [
          { filename: 'a.zip', result: { kind: 'linked' } },
          { filename: 'b.zip', result: { kind: 'adopted' } },
        ],
      }),
    });
    expect(r.kind).toBe('success');
    expect(r.lines).toHaveLength(1);
    const text = [r.title, ...r.lines].join('\n');
    expect(text).toContain('Target');
    expect(text).toContain('My World');
    expect(text).not.toMatch(/failed|could not|couldn't/i);
  });

  it('says a pack that could not be hardlinked uses its own disk space', () => {
    // The dialog promised at plan time that a pack already in the target
    // library "will be linked to it"; `copied_not_linked` is the filesystem
    // fallback, and the toast is the only place that correction can appear.
    const r = buildMigrationToast(tr, {
      ...base,
      mode: 'copy',
      outcome: outcome({
        path: 'copied',
        source_state: { kind: 'untouched' },
        datapacks: [{ filename: 'terralith.zip', result: { kind: 'copied_not_linked' } }],
      }),
    });
    expect(r.kind).toBe('warning');
    const text = r.lines.join('\n');
    expect(text).toContain('terralith.zip');
    expect(text).toMatch(/could not be hardlinked/i);
  });

  it('a move whose original could not be removed names the SOURCE as the place to delete from and never asks for a retry', () => {
    const r = buildMigrationToast(tr, {
      ...base,
      mode: 'move',
      outcome: outcome({ path: 'copied', source_state: { kind: 'left_intact', reason: 'EBUSY' } }),
    });
    expect(r.kind).toBe('warning');
    const text = r.lines.join('\n');
    expect(text).toContain('"Source"');
    expect(text).toContain('"Target"');
    expect(text).not.toContain('EBUSY');
    expect(text).not.toMatch(/try again/i);
  });

  it('backups that stayed behind are reported on a move, with a worded plural', () => {
    const moved = buildMigrationToast(tr, {
      ...base,
      mode: 'move',
      outcome: outcome({ backups_moved: 3, backups_left: 2 }),
    });
    expect(moved.kind).toBe('warning');
    expect(moved.lines.join('\n')).toContain('2 backups');
  });

  it('never claims a copy left backups behind — they stay with the original by design', () => {
    // D4: a copy reports `(0, 0)` backups, so this fixture cannot come from the
    // backend; what is pinned is the wording, not the toast kind (the shared
    // `isPartialOutcome` predicate does flag a stray count here).
    const r = buildMigrationToast(tr, {
      ...base,
      mode: 'copy',
      outcome: outcome({ path: 'copied', source_state: { kind: 'untouched' }, backups_left: 2 }),
    });
    expect(r.lines.join('\n')).not.toMatch(/backup/i);
  });

  it('a link the copy skipped is counted in words', () => {
    const r = buildMigrationToast(tr, {
      ...base,
      mode: 'copy',
      outcome: outcome({ path: 'copied', source_state: { kind: 'untouched' }, links_skipped: 1 }),
    });
    expect(r.kind).toBe('warning');
    expect(r.lines.join('\n')).toContain('1 symbolic link');
  });
});

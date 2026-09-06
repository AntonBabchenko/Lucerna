import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import type { InstanceWithStatus, MigrationOutcome, MigrationPlan, World } from '$lib/ipc/bindings';

// MigrateWorldDialog (spec §7): the target picker, the Copy/Move choice, the
// per-target plan summary, the confirm gate, and the hand-off to the
// `migrateWorld` adapter. Real i18n and the real task registry are used, so
// the locale-switch and `taskFor` cases exercise the real mechanisms.

const m = vi.hoisted(() => ({
  worldMigrationPlan: vi.fn(),
  runningInstances: vi.fn(),
  getDataLocation: vi.fn(),
  migrateWorld: vi.fn(),
}));

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    worldMigrationPlan: m.worldMigrationPlan,
    runningInstances: m.runningInstances,
    getDataLocation: m.getDataLocation,
  },
  events: {
    processSpawned: { listen: () => Promise.resolve(() => {}) },
    processExited: { listen: () => Promise.resolve(() => {}) },
  },
}));

// The adapter is the dialog's only write path and has its own test
// (tests/tasks/adapters/world-migrate.test.ts); stubbing it keeps this file
// about the dialog.
vi.mock('$lib/tasks/adapters/world-migrate', () => ({
  migrateWorld: m.migrateWorld,
}));

import { dataLocation } from '$lib/settings/data-location.svelte';
import { __resetTasksForTest, start } from '$lib/tasks/registry.svelte';
import MigrateWorldDialog from '$lib/worlds/MigrateWorldDialog.svelte';

function instance(over: Partial<InstanceWithStatus> = {}): InstanceWithStatus {
  return {
    id: 'src',
    name: 'Source',
    mc_version: '1.20.1',
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
    ...over,
  };
}

const source = instance();
const alpha = instance({ id: 'a', name: 'Alpha', mc_version: '1.21.1', loader: 'fabric' });
const beta = instance({ id: 'b', name: 'Beta', mc_version: '' });
const gamma = instance({ id: 'c', name: 'Gamma', mc_version: '1.20.1', loader: 'forge' });
const instances = [source, alpha, beta, gamma];

const world: World = {
  folder_name: 'World A',
  size_bytes: 1536,
  modified_unix_ms: null,
  backup_count: 3,
};

function makePlan(over: Partial<MigrationPlan> = {}): MigrationPlan {
  return {
    world_version_name: '1.20.1',
    verdict: { kind: 'will_upgrade' },
    source_loader: 'fabric',
    target_loader: 'fabric',
    mods_missing_in_target: 0,
    datapacks: [],
    datapacks_folders: 0,
    ...over,
  };
}

const outcome: MigrationOutcome = {
  final_folder_name: 'World A',
  path: 'copied',
  datapacks: [],
  datapacks_folders_copied: 0,
  links_skipped: 0,
  source_state: { kind: 'untouched' },
  backups_moved: 0,
  backups_left: 0,
};

function renderDialog(
  over: {
    onDone?: (r: unknown) => void;
    onClose?: () => void;
    instances?: InstanceWithStatus[];
  } = {},
) {
  return render(MigrateWorldDialog, {
    props: {
      instanceId: 'src',
      instanceName: 'Source',
      world,
      instances: over.instances ?? instances,
      onClose: over.onClose ?? (() => {}),
      onDone: over.onDone ?? (() => {}),
    },
  });
}

async function pickTarget(name: RegExp) {
  await fireEvent.click(screen.getByRole('combobox'));
  await fireEvent.mouseDown(screen.getByRole('option', { name }));
}

const dataLocationStatus = (fell_back: boolean) => ({
  status: 'ok' as const,
  data: { effective: 'C:\\Temp\\Lucerna', configured: 'D:\\LucernaData', fell_back },
});

beforeEach(() => {
  vi.clearAllMocks();
  __resetTasksForTest();
  m.runningInstances.mockResolvedValue([]);
  m.worldMigrationPlan.mockResolvedValue({ status: 'ok', data: makePlan() });
  m.migrateWorld.mockResolvedValue({ status: 'ok', outcome });
  m.getDataLocation.mockResolvedValue(dataLocationStatus(false));
});

afterEach(async () => {
  locale.set('en');
  // `dataLocation` is a module singleton (like the data-root gating test
  // notes): put it back to "not fallen back" so the fallback case below
  // cannot leak into whichever test runs next.
  m.getDataLocation.mockResolvedValue(dataLocationStatus(false));
  await dataLocation.refresh();
});

describe('MigrateWorldDialog — target picker', () => {
  it('offers other versioned instances, never the source, and names the excluded', async () => {
    renderDialog();
    expect(screen.getByRole('dialog', { name: 'Migrate "World A"' })).toBeTruthy();
    await fireEvent.click(screen.getByRole('combobox'));
    expect(screen.queryByRole('option', { name: /Source/ })).toBeNull();
    expect(screen.queryByRole('option', { name: /Beta/ })).toBeNull();
    expect(screen.getByRole('option', { name: /Alpha · Fabric 1\.21\.1/ })).toBeTruthy();
    expect(screen.getByRole('option', { name: /Gamma · Forge 1\.20\.1/ })).toBeTruthy();
    expect(screen.getByTestId('migrate-excluded').textContent).toContain('Beta');
  });

  it('disables a running target with the Quick Play running reason', async () => {
    m.runningInstances.mockResolvedValue([
      { instance_id: 'c', pid: 1, max_heap_mb: 2048, started_unix_ms: null },
    ]);
    renderDialog();
    await fireEvent.click(screen.getByRole('combobox'));
    await waitFor(() => {
      const running = screen.getByRole('option', { name: /Gamma/ });
      expect(running.getAttribute('aria-disabled')).toBe('true');
      expect(running.textContent).toContain('Minecraft is already running');
    });
  });
});

describe('MigrateWorldDialog — confirm gate', () => {
  it('is disabled with the reason while a registry task is active for the source', () => {
    start({
      id: 'v1',
      kind: 'verify',
      scope: { instanceId: 'src' },
      title: 'Source',
      phase: null,
      progress: null,
      lane: 'serial',
    });
    renderDialog();
    const confirm = screen.getByTestId('migrate-confirm') as HTMLButtonElement;
    expect(confirm.disabled).toBe(true);
    expect(confirm).toHaveBtnVariant('primary');
    expect(confirm).toHaveBtnSize('sm');
    expect(screen.getByTestId('migrate-disabled-reason').textContent?.trim()).toBe(
      'An operation on this instance is already running.',
    );
  });

  it('is disabled until a target is chosen, then enabled once the plan arrives', async () => {
    renderDialog();
    const confirm = screen.getByTestId('migrate-confirm') as HTMLButtonElement;
    expect(confirm.disabled).toBe(true);
    expect(screen.getByTestId('migrate-disabled-reason').textContent?.trim()).toBe(
      'Choose a target instance first.',
    );
    await pickTarget(/Alpha/);
    await screen.findByTestId('migrate-verdict');
    expect(confirm.disabled).toBe(false);
    expect(screen.queryByTestId('migrate-disabled-reason')).toBeNull();
  });

  it('is disabled with the data-root reason while the data root is fallen back', async () => {
    m.getDataLocation.mockResolvedValue(dataLocationStatus(true));
    await dataLocation.refresh();
    renderDialog();
    expect((screen.getByTestId('migrate-confirm') as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByTestId('migrate-disabled-reason').textContent).toContain(
      'disabled while the data folder is unavailable',
    );
  });

  it('says there is nowhere to migrate to instead of asking for a target', () => {
    // Only the source and a version-less instance: the picker is replaced by
    // the note, so "Choose a target instance first." would be an instruction
    // with nothing to follow it with.
    renderDialog({ instances: [source, beta] });
    expect(screen.getByTestId('migrate-no-targets')).toBeTruthy();
    expect(screen.queryByRole('combobox')).toBeNull();
    expect((screen.getByTestId('migrate-confirm') as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByTestId('migrate-disabled-reason').textContent?.trim()).toBe(
      'No other instance can receive this world.',
    );
  });

  it('keeps the reason reachable by keyboard while the confirm is blocked', async () => {
    // A disabled button receives no hover and `title=` is banned (DESIGN.md
    // §5), so the reason rides the wrapping span — focusable only while the
    // action is blocked, the WorldDatapacks / WorldDetailDialog shape.
    renderDialog();
    const confirm = screen.getByTestId('migrate-confirm') as HTMLButtonElement;
    const wrap = confirm.parentElement as HTMLElement;
    expect(wrap.tagName).toBe('SPAN');
    expect(confirm.getAttribute('title')).toBeNull();
    expect(wrap.getAttribute('tabindex')).toBe('0');

    await pickTarget(/Alpha/);
    await screen.findByTestId('migrate-verdict');
    expect(confirm.disabled).toBe(false);
    expect(wrap.hasAttribute('tabindex')).toBe(false);
  });
});

describe('MigrateWorldDialog — mode', () => {
  it('defaults to Copy and shows the shortcuts note only for Move', async () => {
    renderDialog();
    expect((screen.getByTestId('migrate-mode-copy') as HTMLInputElement).checked).toBe(true);
    expect(screen.queryByTestId('migrate-move-note')).toBeNull();
    expect(screen.getByTestId('migrate-backups').textContent).toContain(
      '3 backups — will stay here',
    );
    expect(screen.getByTestId('migrate-confirm').textContent).toContain('Copy world');

    await fireEvent.click(screen.getByTestId('migrate-mode-move'));
    expect(screen.getByTestId('migrate-move-note').textContent).toContain(
      'Desktop shortcuts and Quick Play entries that open this world in Source will stop working',
    );
    expect(screen.getByTestId('migrate-backups').textContent).toContain(
      '3 backups — will move with the world',
    );
    expect(screen.getByTestId('migrate-confirm').textContent).toContain('Move world');
  });
});

describe('MigrateWorldDialog — plan summary', () => {
  it('renders size, datapack counts, the mods and the loader sentences', async () => {
    m.worldMigrationPlan.mockResolvedValue({
      status: 'ok',
      data: makePlan({
        target_loader: 'forge',
        mods_missing_in_target: 2,
        datapacks_folders: 1,
        datapacks: [
          { filename: 'a.zip', predicted: { kind: 'linked' } },
          { filename: 'b.zip', predicted: { kind: 'linked' } },
          { filename: 'c.zip', predicted: { kind: 'adopted' } },
          {
            filename: 'd.zip',
            predicted: { kind: 'left_as_copy', reason: { kind: 'name_held_by_different_pack' } },
          },
        ],
      }),
    });
    renderDialog();
    await pickTarget(/Gamma/);
    const summary = screen.getByTestId('migrate-summary');
    await waitFor(() => expect(summary.textContent).toContain('4 datapacks'));
    expect(summary.textContent).toContain('1.5 KB on disk');
    expect(summary.textContent).toContain('Last saved by Minecraft 1.20.1');
    expect(summary.textContent).toContain('2 already in the target library');
    expect(summary.textContent).toContain('1 will be added to the target library');
    expect(summary.textContent).toContain('1 kept as a plain copy');
    expect(summary.textContent).toContain('1 folder pack will be copied as it is');
    expect(screen.getByTestId('migrate-mods-missing').textContent).toContain(
      '2 mods this world was played with are not in the target',
    );
    expect(screen.getByTestId('migrate-loader-note').textContent).toContain(
      'Last played with Fabric; the target runs Forge.',
    );
  });

  // Only `NameHeldByDifferentPack` was compared against another pack. A pack
  // left as a copy because its bytes could not be read must NOT borrow that
  // sentence — nobody established whose name it is.
  it('never claims "name in use" for a copy left because the pack was unreadable', async () => {
    m.worldMigrationPlan.mockResolvedValue({
      status: 'ok',
      data: makePlan({
        datapacks: [
          {
            filename: 'a.zip',
            predicted: { kind: 'left_as_copy', reason: { kind: 'unreadable' } },
          },
        ],
      }),
    });
    renderDialog();
    await pickTarget(/Alpha/);
    const summary = screen.getByTestId('migrate-summary');
    await waitFor(() => expect(summary.textContent).toContain('1 datapack'));
    expect(summary.textContent).toContain(
      '1 kept as a plain copy — it will not be added to the target library',
    );
    expect(summary.textContent).not.toContain('its name is in use');
  });

  it('shows a plan failure inline and keeps the button enabled (D3)', async () => {
    m.worldMigrationPlan.mockResolvedValue({
      status: 'error',
      error: { kind: 'world_in_use', folder_name: 'World A' },
    });
    renderDialog();
    await pickTarget(/Alpha/);
    const err = await screen.findByTestId('migrate-plan-error');
    expect(err.textContent).toContain('World "World A" is currently in use');
    expect((screen.getByTestId('migrate-confirm') as HTMLButtonElement).disabled).toBe(false);
  });

  it('re-renders the verdict and the backups line on an en → ru → en switch', async () => {
    renderDialog();
    await pickTarget(/Alpha/);
    const verdict = await screen.findByTestId('migrate-verdict');
    expect(verdict.textContent).toContain(
      'Minecraft will upgrade this world to 1.21.1 the first time it opens.',
    );
    locale.set('ru');
    await waitFor(() => {
      expect(screen.getByTestId('migrate-verdict').textContent).toContain(
        'Minecraft обновит этот мир до 1.21.1 при первом открытии.',
      );
      expect(screen.getByTestId('migrate-backups').textContent).toContain(
        '3 резервные копии — останутся здесь',
      );
    });
    locale.set('en');
    await waitFor(() => {
      expect(screen.getByTestId('migrate-verdict').textContent).toContain(
        'Minecraft will upgrade this world to 1.21.1',
      );
      expect(screen.getByTestId('migrate-backups').textContent).toContain(
        '3 backups — will stay here',
      );
    });
  });
});

describe('MigrateWorldDialog — confirm', () => {
  it('runs the migration through the adapter and reports the outcome', async () => {
    const onDone = vi.fn();
    renderDialog({ onDone });
    await pickTarget(/Alpha/);
    await screen.findByTestId('migrate-verdict');
    await fireEvent.click(screen.getByTestId('migrate-confirm'));
    await waitFor(() => expect(onDone).toHaveBeenCalledTimes(1));
    expect(m.migrateWorld).toHaveBeenCalledWith('World A', {
      fromInstance: 'src',
      worldFolder: 'World A',
      toInstance: 'a',
      mode: 'copy',
    });
    expect(onDone).toHaveBeenCalledWith({ mode: 'copy', outcome, targetName: 'Alpha' });
  });

  it('renders a typed failure inline and reports no outcome', async () => {
    m.migrateWorld.mockResolvedValue({
      status: 'error',
      error: { kind: 'world_in_use', folder_name: 'World A' },
    });
    const onDone = vi.fn();
    renderDialog({ onDone });
    await pickTarget(/Alpha/);
    await screen.findByTestId('migrate-verdict');
    await fireEvent.click(screen.getByTestId('migrate-confirm'));
    const err = await screen.findByTestId('migrate-error');
    expect(err.textContent).toContain('World "World A" is currently in use');
    expect(onDone).not.toHaveBeenCalled();
    expect((screen.getByTestId('migrate-confirm') as HTMLButtonElement).disabled).toBe(false);
  });
});

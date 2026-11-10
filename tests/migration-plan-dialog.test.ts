import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import type { McMigrationPlan_Serialize, ModVersion_Serialize } from '$lib/ipc/bindings';

const modsPlanMcMigration = vi.fn();
const modsApplyMcMigration = vi.fn();
const listInstances = vi.fn();
const modpackFetchToTemp = vi.fn();
const modpackComputeUpdate = vi.fn();
const applyModpackUpdate = vi.fn();
const listWorldNames = vi.fn();

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsPlanMcMigration: (...a: unknown[]) => modsPlanMcMigration(...a),
    modsApplyMcMigration: (...a: unknown[]) => modsApplyMcMigration(...a),
    listInstances: (...a: unknown[]) => listInstances(...a),
    modpackFetchToTemp: (...a: unknown[]) => modpackFetchToTemp(...a),
    modpackComputeUpdate: (...a: unknown[]) => modpackComputeUpdate(...a),
    listWorldNames: (...a: unknown[]) => listWorldNames(...a),
  },
}));

// Every Channel the plan runner constructs, in construction order, so a test
// can push progress ticks the way the backend does. `vi.hoisted` is required:
// the `vi.mock` factory below is lifted above every `const`.
const { channels } = vi.hoisted(() => ({
  channels: [] as { onmessage: ((m: unknown) => void) | null }[],
}));

// The runner does `new Channel()` and then assigns `.onmessage` — `Channel:
// vi.fn()` would not survive that assignment (same stub as
// tests/l10n-prefill-dialog.test.ts and tests/export-pack-dialog.test.ts).
vi.mock('@tauri-apps/api/core', () => ({
  Channel: class {
    onmessage: ((m: unknown) => void) | null = null;
    constructor() {
      channels.push(this as { onmessage: ((m: unknown) => void) | null });
    }
  },
}));

// The restore applies through the task adapter; its registry and Tauri
// channels are not what these tests are about.
vi.mock('$lib/tasks/adapters/pack-update', () => ({
  applyModpackUpdate: (...a: unknown[]) => applyModpackUpdate(...a),
}));

import MigrationPlanDialog from '$lib/mods/MigrationPlanDialog.svelte';
import { instancesChanged, modpacksNav } from '$lib/settings/state.svelte';
import { __resetTasksForTest, finish, start } from '$lib/tasks/registry.svelte';

function modVersion(projectId: string, versionId: string): ModVersion_Serialize {
  return {
    source: 'modrinth',
    project_id: projectId,
    version_id: versionId,
    name: `${projectId}-name`,
    version_number: versionId,
    mc_versions: ['1.20.1'],
    loaders: ['forge'],
    primary_file: {
      filename: `${projectId}-${versionId}.jar`,
      url: 'https://example/mod.jar',
      sha1: 'aa',
      size: 1,
      distribution_allowed: true,
      sha256: null,
    },
    deps: [],
    published_at: null,
  };
}

// Three fits, one replaceable, one new dependency (needed by the replaceable
// row), one stranded, one no-release, and two unjudged — every number distinct
// so an assertion that reads "3" can only mean fits.
const PLAN: McMigrationPlan_Serialize = {
  fits: [
    { sha1: 'f1', name: 'Fine Mod One' },
    { sha1: 'f2', name: 'Fine Mod Two' },
    { sha1: 'f3', name: 'Fine Mod Three' },
  ],
  replaceable: [
    {
      sha1: 'r1',
      name: 'Biomes O Plenty',
      source: 'modrinth',
      project_id: 'bop',
      target: modVersion('bop', 'v-1201'),
    },
  ],
  new_dependencies: [
    {
      source: 'modrinth',
      project_id: 'terrablender',
      target: modVersion('terrablender', 't2'),
      needed_by: ['Biomes O Plenty'],
    },
  ],
  stranded: [{ sha1: 's1', name: 'Old Mod', reason: { kind: 'no_build_for_target' } }],
  no_platform_build: [{ sha1: 'n1', name: 'Jade' }],
  unjudged: [
    { sha1: 'u1', name: 'Kiwi', reason: 'file_not_listed' },
    { sha1: 'u2', name: 'Hand Dropped', reason: 'no_mod_page' },
  ],
  pack: null,
};

function renderDialog(onApplied = vi.fn(), onClose = vi.fn()) {
  return render(MigrationPlanDialog, {
    props: { instanceId: 'inst-1', onClose, onApplied },
  });
}

describe('MigrationPlanDialog', () => {
  beforeAll(() => locale.set('en'));
  afterEach(() => vi.clearAllMocks());

  it('renders the replaceable, new-dependency, and stranded buckets with their counts', async () => {
    modsPlanMcMigration.mockResolvedValue({ status: 'ok', data: PLAN });
    renderDialog();

    await waitFor(() => expect(screen.getByTestId('migration-replaceable-section')).toBeTruthy());
    expect(screen.getByTestId('migration-replaceable-section').textContent).toContain('(1)');
    expect(screen.getByTestId('migration-new-deps-section').textContent).toContain('(1)');
    expect(screen.getByTestId('migration-stranded-section').textContent).toContain('(1)');
  });

  it('lists the unjudged mods by name with the true reason, apart from fits', async () => {
    modsPlanMcMigration.mockResolvedValue({ status: 'ok', data: PLAN });
    renderDialog();

    await waitFor(() => expect(screen.getByTestId('migration-unjudged')).toBeTruthy());
    const block = screen.getByTestId('migration-unjudged');
    // The count is still its own line…
    expect(block.querySelector('summary')?.textContent).toContain('2');
    // …and the mods are no longer anonymous.
    expect(screen.getByTestId('migration-unjudged-row-u1').textContent).toContain('Kiwi');
    expect(screen.getByTestId('migration-unjudged-row-u1').textContent).toMatch(
      /doesn't list this exact file/i,
    );
    expect(screen.getByTestId('migration-unjudged-row-u2').textContent).toMatch(
      /isn't linked to a mod page/i,
    );
    // Fits stays the real fits count (3), never 3 + 2.
    const summaryText = screen.getByTestId('migration-summary').textContent ?? '';
    expect(summaryText).toContain('3 mods already fit');
    expect(summaryText).not.toContain('5 mods');
  });

  it('a needed_by name appears on the new-dependency row', async () => {
    modsPlanMcMigration.mockResolvedValue({ status: 'ok', data: PLAN });
    renderDialog();

    await waitFor(() => expect(screen.getByTestId('migration-new-deps-section')).toBeTruthy());
    expect(screen.getByTestId('migration-new-deps-section').textContent).toContain(
      'Biomes O Plenty',
    );
  });

  it('enables Apply the moment anything is selected, without forcing a stranded choice', async () => {
    // A user who only wants the top-section reinstall must not be blocked by
    // the stranded section. Apply is gated on "something is selected", not on
    // "every stranded row decided" — an undecided stranded mod is simply left
    // in place (see the keep-as-is apply test below).
    modsPlanMcMigration.mockResolvedValue({ status: 'ok', data: PLAN });
    renderDialog();

    await waitFor(() => expect(screen.getByTestId('migration-apply-btn')).toBeTruthy());
    const applyBtn = screen.getByTestId('migration-apply-btn') as HTMLButtonElement;
    // Nothing selected yet — there is genuinely nothing to apply.
    expect(applyBtn.disabled).toBe(true);
    expect(screen.getByTestId('migration-apply-disabled-reason').textContent).toMatch(/select/i);

    // Check only the top-section reinstall; the stranded row stays untouched.
    await fireEvent.click(screen.getByRole('checkbox', { name: 'Biomes O Plenty' }));

    expect(applyBtn.disabled).toBe(false);
    expect(screen.getByTestId('migration-apply-disabled-reason').textContent).toBe('');
  });

  it('select-all checks every replaceable row and clears them again', async () => {
    // Maintainer request during the Forge→Fabric smoke: ten replaceable rows
    // and no way to take them all at once.
    modsPlanMcMigration.mockResolvedValue({ status: 'ok', data: PLAN });
    renderDialog();

    await waitFor(() => expect(screen.getByTestId('migration-replace-select-all')).toBeTruthy());
    const master = screen.getByTestId('migration-replace-select-all') as HTMLInputElement;
    const applyBtn = screen.getByTestId('migration-apply-btn') as HTMLButtonElement;
    const row = screen.getByRole('checkbox', { name: 'Biomes O Plenty' }) as HTMLInputElement;

    await fireEvent.click(master);
    expect(row.checked).toBe(true);
    expect(applyBtn.disabled).toBe(false);

    await fireEvent.click(master);
    expect(row.checked).toBe(false);
    expect(applyBtn.disabled).toBe(true);
  });

  it('applies the checked reinstall and keeps every undecided stranded mod as-is', async () => {
    // The user reinstalls the top-section mod and never touches the stranded
    // section. Its rows must be sent as `keep` (a no-op that leaves the jar in
    // place) — never dropped from the payload, and never defaulted to a
    // destructive disable/remove. Checking BoP also auto-includes its mandatory
    // TerraBlender dependency (see the coupling test below), so it rides along.
    modsPlanMcMigration.mockResolvedValue({ status: 'ok', data: PLAN });
    modsApplyMcMigration.mockResolvedValue({ status: 'ok', data: { outcomes: [] } });
    renderDialog();

    await waitFor(() => expect(screen.getByTestId('migration-apply-btn')).toBeTruthy());
    await fireEvent.click(screen.getByRole('checkbox', { name: 'Biomes O Plenty' }));
    await fireEvent.click(screen.getByTestId('migration-apply-btn'));

    await waitFor(() => expect(modsApplyMcMigration).toHaveBeenCalledTimes(1));
    expect(modsApplyMcMigration).toHaveBeenCalledWith('inst-1', {
      replace: [{ old_sha1: 'r1', target: PLAN.replaceable[0]?.target }],
      new_dependencies: [PLAN.new_dependencies[0]?.target],
      stranded: [
        { sha1: 's1', disposition: 'keep' },
        { sha1: 'n1', disposition: 'keep' },
      ],
    });
  });

  it('auto-includes and locks a mandatory new-dependency when its dependent replace is checked', async () => {
    // A checked replace must never be applied without the mandatory dependency
    // the plan itself surfaced for its target: reinstalling BoP without
    // TerraBlender reintroduces the exact pre-load crash the migration exists to
    // prevent, while the report shows "Replaced ✓". So a dep whose `needed_by`
    // includes a checked replace is force-included and its checkbox locked.
    modsPlanMcMigration.mockResolvedValue({ status: 'ok', data: PLAN });
    modsApplyMcMigration.mockResolvedValue({ status: 'ok', data: { outcomes: [] } });
    renderDialog();

    await waitFor(() => expect(screen.getByTestId('migration-apply-btn')).toBeTruthy());
    const depCb = () =>
      screen
        .getByTestId('migration-new-dep-row-modrinth:terrablender')
        .querySelector('input[type="checkbox"]') as HTMLInputElement;

    // Before checking BoP the dep is optional (unchecked, editable).
    expect(depCb().checked).toBe(false);
    expect(depCb().disabled).toBe(false);

    // Checking BoP forces the dependency on and locks it.
    await fireEvent.click(screen.getByRole('checkbox', { name: 'Biomes O Plenty' }));
    expect(depCb().checked).toBe(true);
    expect(depCb().disabled).toBe(true);

    // Unchecking BoP releases the dependency back to optional/unchecked.
    await fireEvent.click(screen.getByRole('checkbox', { name: 'Biomes O Plenty' }));
    expect(depCb().checked).toBe(false);
    expect(depCb().disabled).toBe(false);
  });

  it('sends only checked replaceable/new-dependency rows, plus every stranded row with its chosen disposition', async () => {
    modsPlanMcMigration.mockResolvedValue({ status: 'ok', data: PLAN });
    modsApplyMcMigration.mockResolvedValue({ status: 'ok', data: { outcomes: [] } });
    renderDialog();

    await waitFor(() => expect(screen.getByTestId('migration-apply-btn')).toBeTruthy());
    // Leave the replaceable and new-dependency rows unchecked (opt-in
    // default) and only settle the mandatory stranded choice.
    await fireEvent.click(screen.getByTestId('migration-disposition-remove-s1'));
    await fireEvent.click(screen.getByTestId('migration-apply-btn'));

    await waitFor(() => expect(modsApplyMcMigration).toHaveBeenCalledTimes(1));
    expect(modsApplyMcMigration).toHaveBeenCalledWith('inst-1', {
      replace: [],
      new_dependencies: [],
      stranded: [
        { sha1: 's1', disposition: 'remove' },
        { sha1: 'n1', disposition: 'keep' },
      ],
    });
  });

  it('a checked replaceable row sends the EXACT target the plan showed', async () => {
    modsPlanMcMigration.mockResolvedValue({ status: 'ok', data: PLAN });
    modsApplyMcMigration.mockResolvedValue({ status: 'ok', data: { outcomes: [] } });
    renderDialog();

    await waitFor(() => expect(screen.getByTestId('migration-apply-btn')).toBeTruthy());
    await fireEvent.click(screen.getByRole('checkbox', { name: 'Biomes O Plenty' }));
    await fireEvent.click(screen.getByTestId('migration-disposition-keep-s1'));
    await fireEvent.click(screen.getByTestId('migration-apply-btn'));

    await waitFor(() => expect(modsApplyMcMigration).toHaveBeenCalledTimes(1));
    const [, selections] = modsApplyMcMigration.mock.calls[0] as [string, { replace: unknown[] }];
    expect(selections.replace).toEqual([{ old_sha1: 'r1', target: PLAN.replaceable[0]?.target }]);
  });

  it('a busy refusal says why, keeps the reviewed plan, and lets the user apply again', async () => {
    // The apply takes the instance's maintenance claim: while the game runs
    // or starts, or a pack update, world migration or clone holds the
    // instance, the backend refuses with InstanceBusy before touching a jar.
    // Nothing was applied, so this must read as "not now" — not a result
    // view, not onApplied — and the same selections must still be one click
    // away once the other operation ends.
    modsPlanMcMigration.mockResolvedValue({ status: 'ok', data: PLAN });
    modsApplyMcMigration.mockResolvedValueOnce({
      status: 'error',
      error: { kind: 'instance_busy' },
    });
    const onApplied = vi.fn();
    renderDialog(onApplied);

    await waitFor(() => expect(screen.getByTestId('migration-apply-btn')).toBeTruthy());
    await fireEvent.click(screen.getByTestId('migration-disposition-remove-s1'));
    await fireEvent.click(screen.getByTestId('migration-apply-btn'));

    const error = await screen.findByTestId('migration-apply-error');
    expect(error.textContent).toBe('An operation is already in progress, or the game is running.');
    expect(screen.queryByTestId('migration-result-summary')).toBeNull();
    expect(onApplied).not.toHaveBeenCalled();
    await waitFor(() =>
      expect((screen.getByTestId('migration-apply-btn') as HTMLButtonElement).disabled).toBe(false),
    );

    modsApplyMcMigration.mockResolvedValueOnce({
      status: 'ok',
      data: { outcomes: [{ kind: 'removed', sha1: 's1', name: 'Old Mod' }] },
    });
    await fireEvent.click(screen.getByTestId('migration-apply-btn'));
    await waitFor(() => expect(screen.getByTestId('migration-done-btn')).toBeTruthy());
    expect(modsApplyMcMigration).toHaveBeenCalledTimes(2);
    expect(modsApplyMcMigration.mock.calls[1]).toEqual(modsApplyMcMigration.mock.calls[0]);
    expect(onApplied).toHaveBeenCalledTimes(1);
  });

  it('shows the result view with a Done button after a successful apply, and calls onApplied', async () => {
    modsPlanMcMigration.mockResolvedValue({ status: 'ok', data: PLAN });
    modsApplyMcMigration.mockResolvedValue({
      status: 'ok',
      data: { outcomes: [{ kind: 'removed', sha1: 's1', name: 'Old Mod' }] },
    });
    const onApplied = vi.fn();
    renderDialog(onApplied);

    await waitFor(() => expect(screen.getByTestId('migration-apply-btn')).toBeTruthy());
    await fireEvent.click(screen.getByTestId('migration-disposition-remove-s1'));
    await fireEvent.click(screen.getByTestId('migration-apply-btn'));

    await waitFor(() => expect(screen.getByTestId('migration-done-btn')).toBeTruthy());
    expect(onApplied).toHaveBeenCalledTimes(1);
    expect(screen.getByTestId('migration-result-summary').textContent).toContain('1 of 1');
  });

  it('shows «no release» mods in their own section, never under «needs your decision»', async () => {
    modsPlanMcMigration.mockResolvedValue({ status: 'ok', data: PLAN });
    renderDialog();

    await waitFor(() => expect(screen.getByTestId('migration-no-release-section')).toBeTruthy());
    const section = screen.getByTestId('migration-no-release-section');
    expect(section.textContent).toContain('(1)');
    expect(section.textContent).toContain('Jade');
    // Both outcomes are stated — «keep it» alone would be as dishonest as «remove it».
    expect(section.textContent).toMatch(/may fail to load/i);
    expect(section.textContent).toMatch(/keep it if the game runs/i);
    expect(screen.getByTestId('migration-stranded-section').textContent).not.toContain('Jade');
    expect(screen.getByTestId('migration-summary').textContent).toContain(
      '1 have no release for this version',
    );
  });

  it('sends an undecided no-release row as keep, inside selections.stranded', async () => {
    modsPlanMcMigration.mockResolvedValue({ status: 'ok', data: PLAN });
    modsApplyMcMigration.mockResolvedValue({ status: 'ok', data: { outcomes: [] } });
    renderDialog();

    await waitFor(() => expect(screen.getByTestId('migration-apply-btn')).toBeTruthy());
    await fireEvent.click(screen.getByRole('checkbox', { name: 'Biomes O Plenty' }));
    await fireEvent.click(screen.getByTestId('migration-apply-btn'));

    await waitFor(() => expect(modsApplyMcMigration).toHaveBeenCalledTimes(1));
    const selections = modsApplyMcMigration.mock.calls[0][1] as {
      stranded: { sha1: string; disposition: string }[];
    };
    // Dropping the row from the payload would be silent; defaulting it to a
    // destructive choice would be worse.
    expect(selections.stranded).toContainEqual({ sha1: 'n1', disposition: 'keep' });
  });

  it('a decided no-release row carries its disposition and alone enables Apply', async () => {
    modsPlanMcMigration.mockResolvedValue({ status: 'ok', data: PLAN });
    modsApplyMcMigration.mockResolvedValue({ status: 'ok', data: { outcomes: [] } });
    renderDialog();

    await waitFor(() =>
      expect(screen.getByTestId('migration-disposition-disable-n1')).toBeTruthy(),
    );
    await fireEvent.click(screen.getByTestId('migration-disposition-disable-n1'));
    const applyBtn = screen.getByTestId('migration-apply-btn') as HTMLButtonElement;
    expect(applyBtn.disabled).toBe(false);
    await fireEvent.click(applyBtn);

    await waitFor(() => expect(modsApplyMcMigration).toHaveBeenCalledTimes(1));
    const selections = modsApplyMcMigration.mock.calls[0][1] as {
      stranded: { sha1: string; disposition: string }[];
    };
    expect(selections.stranded).toContainEqual({ sha1: 'n1', disposition: 'disable' });
  });

  it('a plan with nothing but fits says so, and the summary names no release clause', async () => {
    modsPlanMcMigration.mockResolvedValue({
      status: 'ok',
      data: {
        fits: [{ sha1: 'f1', name: 'Fine Mod One' }],
        replaceable: [],
        new_dependencies: [],
        stranded: [],
        no_platform_build: [],
        unjudged: [],
      },
    });
    renderDialog();

    await waitFor(() => expect(screen.getByTestId('migration-nothing-to-do')).toBeTruthy());
    expect(screen.getByTestId('migration-summary').textContent).not.toMatch(/no release/i);
    expect(screen.queryByTestId('migration-unjudged')).toBeNull();
    expect(screen.queryByTestId('migration-no-release-section')).toBeNull();
  });

  it('reports the loaded plan to its opener, once, and only on success', async () => {
    // D8 leg 3: the opener uses this to re-run the chip's live check AFTER the
    // plan's own probes have warmed the shared version cache — one network
    // burst, two surfaces answering from the same data.
    const onPlanLoaded = vi.fn();
    modsPlanMcMigration.mockResolvedValue({ status: 'ok', data: PLAN });
    render(MigrationPlanDialog, {
      props: { instanceId: 'inst-1', onClose: vi.fn(), onPlanLoaded },
    });
    await waitFor(() => expect(screen.getByTestId('migration-summary')).toBeTruthy());
    expect(onPlanLoaded).toHaveBeenCalledTimes(1);
  });
});

describe('MigrationPlanDialog — while the plan loads', () => {
  beforeAll(() => locale.set('en'));
  // `beforeEach`, not `afterEach`: the describe above renders the dialog too,
  // and every render constructs a Channel.
  beforeEach(() => {
    vi.clearAllMocks();
    channels.length = 0;
  });

  it('reads as it always did until the backend reports progress', async () => {
    // (pin)
    modsPlanMcMigration.mockReturnValue(new Promise(() => {}));
    renderDialog();
    expect(await screen.findByRole('status', { name: 'Checking installed mods…' })).toBeTruthy();
    expect(screen.queryByText(/\d+ of \d+/)).toBeNull();
  });

  it('asks for the plan with a progress channel', async () => {
    modsPlanMcMigration.mockReturnValue(new Promise(() => {}));
    renderDialog();
    await waitFor(() =>
      expect(modsPlanMcMigration).toHaveBeenCalledWith('inst-1', expect.anything()),
    );
    expect(channels).toHaveLength(1);
  });

  it('names the phase and counts through it as ticks arrive', async () => {
    modsPlanMcMigration.mockReturnValue(new Promise(() => {}));
    renderDialog();
    await waitFor(() => expect(channels).toHaveLength(1));
    const tick = (m: unknown) => channels[0].onmessage?.(m);

    tick({ phase: 'checking_installed', done: 130, total: 136 });
    expect(await screen.findByText('130 of 136')).toBeTruthy();
    expect(await screen.findByRole('status', { name: 'Checking installed mods…' })).toBeTruthy();

    tick({ phase: 'finding_replacements', done: 2, total: 9 });
    expect(await screen.findByText('2 of 9')).toBeTruthy();
    expect(await screen.findByRole('status', { name: 'Looking for replacements…' })).toBeTruthy();

    tick({ phase: 'resolving_dependencies', done: 0, total: 3 });
    expect(await screen.findByText('0 of 3')).toBeTruthy();
    expect(
      await screen.findByRole('status', { name: 'Checking what the replacements need…' }),
    ).toBeTruthy();
  });
});

// The real case this block exists for: Fabulously Optimized 14.0.0-beta.4 is
// built for Minecraft 26.2, the instance was re-versioned to 26.1.1 with the
// pack left attached, and the pack's own mods are stranded.
describe('MigrationPlanDialog — the modpack block', () => {
  beforeAll(() => locale.set('en'));
  afterEach(() => {
    vi.clearAllMocks();
    modpacksNav.value = null;
    __resetTasksForTest();
  });

  const CURRENT = {
    id: 'cur',
    name: '14.0.0-beta.4',
    version_number: '14.0.0-beta.4',
    game_versions: ['26.2'],
    loaders: ['fabric'],
    date_published: '2026-08-02T12:20:00Z',
  };

  function packPlan(pack: McMigrationPlan_Serialize['pack']): McMigrationPlan_Serialize {
    return {
      fits: [],
      replaceable: [],
      new_dependencies: [],
      stranded: [
        { sha1: 'p1', name: 'Sodium Extra', reason: { kind: 'pack_origin' } },
        { sha1: 'p2', name: 'Zoomify', reason: { kind: 'pack_origin' } },
      ],
      no_platform_build: [],
      unjudged: [],
      pack,
    };
  }

  const DIFFERS: McMigrationPlan_Serialize['pack'] = {
    kind: 'known',
    pack_name: 'Fabulously Optimized',
    pack_version: '14.0.0-beta.4',
    current: CURRENT,
    instance_mc: '26.1.1',
    instance_loader: 'fabric',
    minecraft_differs: true,
    loader_differs: false,
    other_versions_for_instance: 0,
    alternatives_by_loader: true,
  };

  const INSTANCE = {
    id: 'inst-1',
    name: 'Fabulously Optimized',
    mrpack_source: 'modrinth',
    mrpack_project_id: '1KVo5zza',
    mrpack_version_id: 'cur',
  };

  const RESTORE_DIFF = {
    added: [],
    removed: [],
    updated: [],
    version_bump: {
      old_game_version: '26.1.1',
      new_game_version: '26.2',
      old_loader_version: '0.19.3',
      new_loader_version: '0.19.3',
    },
    new_version_number: '14.0.0-beta.4',
  };

  it('names what the pack is built for and offers to restore it', async () => {
    modsPlanMcMigration.mockResolvedValue({ status: 'ok', data: packPlan(DIFFERS) });
    renderDialog();

    await waitFor(() => expect(screen.getByTestId('migration-pack-block')).toBeTruthy());
    const block = screen.getByTestId('migration-pack-block').textContent ?? '';
    expect(block).toContain('Fabulously Optimized 14.0.0-beta.4');
    expect(block).toContain('Minecraft 26.2');
    expect(block).toContain('26.1.1');
    expect(screen.getByTestId('migration-pack-restore').textContent).toContain(
      'Restore Minecraft 26.2',
    );
    // The dead-end sentence is gone from the rows the block covers.
    const section = screen.getByTestId('migration-stranded-section').textContent ?? '';
    expect(section).toContain('the button above fixes it');
    expect(section).not.toContain('modpack update flow');
  });

  it('says plainly when the pack has no other version for this Minecraft', async () => {
    modsPlanMcMigration.mockResolvedValue({ status: 'ok', data: packPlan(DIFFERS) });
    renderDialog();

    await waitFor(() => expect(screen.getByTestId('migration-pack-block')).toBeTruthy());
    expect(screen.queryByTestId('migration-pack-switch')).toBeNull();
    // The count is per Minecraft AND loader, so the sentence names both.
    expect(screen.getByTestId('migration-pack-block').textContent).toContain(
      'no other version for Minecraft 26.1.1 · Fabric',
    );
  });

  it('opens the version switch pre-filtered to this Minecraft when alternatives exist', async () => {
    modsPlanMcMigration.mockResolvedValue({
      status: 'ok',
      data: packPlan({ ...DIFFERS, other_versions_for_instance: 2 }),
    });
    const onClose = vi.fn();
    renderDialog(vi.fn(), onClose);

    await waitFor(() => expect(screen.getByTestId('migration-pack-switch')).toBeTruthy());
    expect(screen.getByTestId('migration-pack-switch').textContent).toContain(
      '2 for Minecraft 26.1.1 · Fabric',
    );
    await fireEvent.click(screen.getByTestId('migration-pack-switch'));

    expect(modpacksNav.value).toEqual({
      openDrawerForInstance: 'inst-1',
      openSwitch: { mcFilter: '26.1.1', loaderFilter: 'fabric' },
    });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('reports a failed pack check honestly and retries the plan', async () => {
    modsPlanMcMigration.mockResolvedValue({
      status: 'ok',
      data: packPlan({ kind: 'unavailable', pack_name: 'Fabulously Optimized' }),
    });
    renderDialog();

    await waitFor(() => expect(screen.getByTestId('migration-pack-block')).toBeTruthy());
    expect(screen.getByTestId('migration-pack-block').textContent).toContain("Couldn't check");
    expect(screen.queryByTestId('migration-pack-restore')).toBeNull();
    await fireEvent.click(screen.getByTestId('migration-pack-retry'));
    await waitFor(() => expect(modsPlanMcMigration).toHaveBeenCalledTimes(2));
  });

  it('without a pack verdict the rows say what the pack does, not where to go', async () => {
    modsPlanMcMigration.mockResolvedValue({ status: 'ok', data: packPlan(null) });
    renderDialog();

    await waitFor(() => expect(screen.getByTestId('migration-stranded-section')).toBeTruthy());
    expect(screen.queryByTestId('migration-pack-block')).toBeNull();
    const section = screen.getByTestId('migration-stranded-section').textContent ?? '';
    expect(section).toContain('The modpack itself installs this version of the mod');
  });

  it('a pack already on its own platform gets no restore button', async () => {
    modsPlanMcMigration.mockResolvedValue({
      status: 'ok',
      data: packPlan({ ...DIFFERS, minecraft_differs: false, instance_mc: '26.2' }),
    });
    renderDialog();

    await waitFor(() => expect(screen.getByTestId('migration-pack-block')).toBeTruthy());
    expect(screen.queryByTestId('migration-pack-restore')).toBeNull();
    expect(screen.getByTestId('migration-pack-block').textContent).toContain(
      'itself ships these versions',
    );
  });

  it('restores through the confirm dialog, then reports and re-plans', async () => {
    modsPlanMcMigration.mockResolvedValue({ status: 'ok', data: packPlan(DIFFERS) });
    listInstances.mockResolvedValue({ status: 'ok', data: [INSTANCE] });
    modpackFetchToTemp.mockResolvedValue({ status: 'ok', data: 'C:/tmp/fo.mrpack' });
    modpackComputeUpdate.mockResolvedValue({ status: 'ok', data: RESTORE_DIFF });
    applyModpackUpdate.mockResolvedValue({ status: 'ok' });
    listWorldNames.mockResolvedValue({ status: 'ok', data: ['World', 'Creative'] });
    const onApplied = vi.fn();
    const changedBefore = instancesChanged.value;
    renderDialog(onApplied);

    await waitFor(() => expect(screen.getByTestId('migration-pack-restore')).toBeTruthy());
    await fireEvent.click(screen.getByTestId('migration-pack-restore'));

    // The installed version — not the newest — is what gets re-applied.
    await waitFor(() => expect(screen.getByTestId('pack-restore-dialog')).toBeTruthy());
    expect(modpackFetchToTemp).toHaveBeenCalledWith('modrinth', '1KVo5zza', 'cur');
    expect(modpackComputeUpdate).toHaveBeenCalledWith('inst-1', 'C:/tmp/fo.mrpack');

    await fireEvent.click(screen.getByTestId('pack-restore-confirm'));
    await waitFor(() => expect(applyModpackUpdate).toHaveBeenCalledTimes(1));
    expect(applyModpackUpdate.mock.calls[0]?.slice(0, 4)).toEqual([
      'Fabulously Optimized',
      'inst-1',
      'C:/tmp/fo.mrpack',
      'cur',
    ]);
    // The worlds move to another Minecraft version: backed up unless the user
    // unticks it, as on every other pack update.
    expect(applyModpackUpdate.mock.calls[0]?.[4]).toBe(true);
    await waitFor(() => expect(onApplied).toHaveBeenCalledTimes(1));
    // The instance record changed (Minecraft / loader): the page must refetch
    // the instance list, or the sidebar keeps the old version.
    expect(instancesChanged.value).toBe(changedBefore + 1);
    await waitFor(() => expect(modsPlanMcMigration).toHaveBeenCalledTimes(2));
    expect(screen.queryByTestId('pack-restore-dialog')).toBeNull();
  });

  it('restores without the world backup once the user unticks it', async () => {
    modsPlanMcMigration.mockResolvedValue({ status: 'ok', data: packPlan(DIFFERS) });
    listInstances.mockResolvedValue({ status: 'ok', data: [INSTANCE] });
    modpackFetchToTemp.mockResolvedValue({ status: 'ok', data: 'C:/tmp/fo.mrpack' });
    modpackComputeUpdate.mockResolvedValue({ status: 'ok', data: RESTORE_DIFF });
    applyModpackUpdate.mockResolvedValue({ status: 'ok' });
    listWorldNames.mockResolvedValue({ status: 'ok', data: ['World'] });
    renderDialog();

    await waitFor(() => expect(screen.getByTestId('migration-pack-restore')).toBeTruthy());
    await fireEvent.click(screen.getByTestId('migration-pack-restore'));
    const box = (await screen.findByTestId('restore-backup-worlds')) as HTMLInputElement;
    await fireEvent.click(box);
    await fireEvent.click(screen.getByTestId('pack-restore-confirm'));
    await waitFor(() => expect(applyModpackUpdate).toHaveBeenCalledTimes(1));
    expect(applyModpackUpdate.mock.calls[0]?.[4]).toBe(false);
  });

  it('names the loader, not Minecraft, when only the loader differs', async () => {
    // A NeoForge pack on 1.21.1 whose instance was switched to Fabric with the
    // pack left attached: Minecraft still matches.
    modsPlanMcMigration.mockResolvedValue({
      status: 'ok',
      data: packPlan({
        ...DIFFERS,
        current: { ...CURRENT, game_versions: ['1.21.1'], loaders: ['neoforge'] },
        instance_mc: '1.21.1',
        minecraft_differs: false,
        loader_differs: true,
      }),
    });
    renderDialog();

    await waitFor(() => expect(screen.getByTestId('migration-pack-block')).toBeTruthy());
    const block = screen.getByTestId('migration-pack-block').textContent ?? '';
    expect(block).toContain('built for NeoForge');
    expect(block).not.toContain('built for Minecraft');
    expect(block).toContain('different mod loader');
    expect(screen.getByTestId('migration-pack-restore').textContent).toContain('Restore NeoForge');
  });

  it('offers only the version switch when the platform matches but other versions exist', async () => {
    modsPlanMcMigration.mockResolvedValue({
      status: 'ok',
      data: packPlan({
        ...DIFFERS,
        minecraft_differs: false,
        instance_mc: '26.2',
        other_versions_for_instance: 3,
      }),
    });
    renderDialog();

    await waitFor(() => expect(screen.getByTestId('migration-pack-block')).toBeTruthy());
    expect(screen.queryByTestId('migration-pack-restore')).toBeNull();
    expect(screen.getByTestId('migration-pack-switch').textContent).toContain(
      '3 for Minecraft 26.2',
    );
    expect(screen.getByTestId('migration-pack-block').textContent).not.toContain(
      'no other version',
    );
  });

  it('a cancelled restore confirm closes and applies nothing', async () => {
    modsPlanMcMigration.mockResolvedValue({ status: 'ok', data: packPlan(DIFFERS) });
    listInstances.mockResolvedValue({ status: 'ok', data: [INSTANCE] });
    modpackFetchToTemp.mockResolvedValue({ status: 'ok', data: 'C:/tmp/fo.mrpack' });
    modpackComputeUpdate.mockResolvedValue({ status: 'ok', data: RESTORE_DIFF });
    renderDialog();

    await waitFor(() => expect(screen.getByTestId('migration-pack-restore')).toBeTruthy());
    await fireEvent.click(screen.getByTestId('migration-pack-restore'));
    await waitFor(() => expect(screen.getByTestId('pack-restore-dialog')).toBeTruthy());
    const restoreDialog = screen.getByTestId('pack-restore-dialog');
    const cancel = Array.from(restoreDialog.querySelectorAll('button')).find(
      (b) => b.textContent?.trim() === 'Cancel',
    ) as HTMLButtonElement;
    await fireEvent.click(cancel);

    await waitFor(() => expect(screen.queryByTestId('pack-restore-dialog')).toBeNull());
    expect(applyModpackUpdate).not.toHaveBeenCalled();
    expect((screen.getByTestId('migration-pack-restore') as HTMLButtonElement).disabled).toBe(
      false,
    );
  });

  it('a double click starts one restore, not two', async () => {
    modsPlanMcMigration.mockResolvedValue({ status: 'ok', data: packPlan(DIFFERS) });
    let resolveList: (v: unknown) => void = () => {};
    listInstances.mockReturnValue(
      new Promise((r) => {
        resolveList = r;
      }),
    );
    modpackFetchToTemp.mockResolvedValue({ status: 'ok', data: 'C:/tmp/fo.mrpack' });
    modpackComputeUpdate.mockResolvedValue({ status: 'ok', data: RESTORE_DIFF });
    renderDialog();

    await waitFor(() => expect(screen.getByTestId('migration-pack-restore')).toBeTruthy());
    await fireEvent.click(screen.getByTestId('migration-pack-restore'));
    await fireEvent.click(screen.getByTestId('migration-pack-restore'));
    resolveList({ status: 'ok', data: [INSTANCE] });

    await waitFor(() => expect(screen.getByTestId('pack-restore-dialog')).toBeTruthy());
    expect(listInstances).toHaveBeenCalledTimes(1);
    expect(modpackFetchToTemp).toHaveBeenCalledTimes(1);
  });

  it('while a restore writes, Apply is held back but the dialog still closes', async () => {
    modsPlanMcMigration.mockResolvedValue({ status: 'ok', data: packPlan(DIFFERS) });
    listInstances.mockResolvedValue({ status: 'ok', data: [INSTANCE] });
    modpackFetchToTemp.mockResolvedValue({ status: 'ok', data: 'C:/tmp/fo.mrpack' });
    modpackComputeUpdate.mockResolvedValue({ status: 'ok', data: RESTORE_DIFF });
    let finishApply: (v: unknown) => void = () => {};
    applyModpackUpdate.mockReturnValue(
      new Promise((r) => {
        finishApply = r;
      }),
    );
    const onClose = vi.fn();
    renderDialog(vi.fn(), onClose);

    await waitFor(() => expect(screen.getByTestId('migration-pack-restore')).toBeTruthy());
    // A stranded choice makes the dialog's own Apply available…
    await fireEvent.click(screen.getByTestId('migration-disposition-disable-p1'));
    await fireEvent.click(screen.getByTestId('migration-pack-restore'));
    await waitFor(() => expect(screen.getByTestId('pack-restore-confirm')).toBeTruthy());
    await fireEvent.click(screen.getByTestId('pack-restore-confirm'));
    await waitFor(() => expect(applyModpackUpdate).toHaveBeenCalledTimes(1));

    // …but not while the pack is being re-applied to the same instance.
    expect((screen.getByTestId('migration-apply-btn') as HTMLButtonElement).disabled).toBe(true);
    // Closing is never held hostage: a restore queued behind another long task
    // could otherwise lock the window until that task ends.
    await fireEvent.click(screen.getByRole('button', { name: 'Close' }));
    expect(onClose).toHaveBeenCalledTimes(1);

    finishApply({ status: 'ok' });
    await waitFor(() => expect(modsPlanMcMigration).toHaveBeenCalledTimes(2));
  });

  it('while the migration applies, the restore cannot start', async () => {
    modsPlanMcMigration.mockResolvedValue({ status: 'ok', data: packPlan(DIFFERS) });
    modsApplyMcMigration.mockReturnValue(new Promise(() => {}));
    renderDialog();

    await waitFor(() => expect(screen.getByTestId('migration-pack-restore')).toBeTruthy());
    await fireEvent.click(screen.getByTestId('migration-disposition-disable-p1'));
    await fireEvent.click(screen.getByTestId('migration-apply-btn'));
    await waitFor(() => expect(modsApplyMcMigration).toHaveBeenCalledTimes(1));

    expect((screen.getByTestId('migration-pack-restore') as HTMLButtonElement).disabled).toBe(true);
    await fireEvent.click(screen.getByTestId('migration-pack-restore'));
    expect(listInstances).not.toHaveBeenCalled();
  });

  it('names both Minecraft and the loader when both differ', async () => {
    modsPlanMcMigration.mockResolvedValue({
      status: 'ok',
      data: packPlan({ ...DIFFERS, instance_loader: 'quilt', loader_differs: true }),
    });
    renderDialog();

    await waitFor(() => expect(screen.getByTestId('migration-pack-block')).toBeTruthy());
    const block = screen.getByTestId('migration-pack-block').textContent ?? '';
    expect(block).toContain('built for Minecraft 26.2 · Fabric');
    expect(block).toContain('Minecraft 26.1.1 with a different mod loader');
    expect(screen.getByTestId('migration-pack-restore').textContent).toContain(
      'Restore Minecraft 26.2 · Fabric',
    );
  });

  it('does not name one loader when the pack version lists several', async () => {
    // The archive pins one of them; which, the list cannot say.
    modsPlanMcMigration.mockResolvedValue({
      status: 'ok',
      data: packPlan({
        ...DIFFERS,
        current: { ...CURRENT, game_versions: ['1.21.1'], loaders: ['fabric', 'quilt'] },
        instance_mc: '1.21.1',
        instance_loader: 'neoforge',
        minecraft_differs: false,
        loader_differs: true,
      }),
    });
    renderDialog();

    await waitFor(() => expect(screen.getByTestId('migration-pack-restore')).toBeTruthy());
    expect(screen.getByTestId('migration-pack-restore').textContent).toContain(
      "Restore the modpack's platform",
    );
    expect(screen.getByTestId('migration-pack-block').textContent).not.toContain('Fabric, Quilt');
  });

  it('claims no loader match for a source that lists no loaders', async () => {
    // ATLauncher: versions carry no loader, so the count is per Minecraft only.
    modsPlanMcMigration.mockResolvedValue({
      status: 'ok',
      data: packPlan({
        ...DIFFERS,
        current: { ...CURRENT, loaders: [] },
        alternatives_by_loader: false,
      }),
    });
    const onClose = vi.fn();
    renderDialog(vi.fn(), onClose);

    await waitFor(() => expect(screen.getByTestId('migration-pack-block')).toBeTruthy());
    const block = screen.getByTestId('migration-pack-block').textContent ?? '';
    expect(block).toContain('no other version for Minecraft 26.1.1.');
    expect(block).not.toContain('·');
  });

  it('deep-links without a loader filter when the source lists no loaders', async () => {
    modsPlanMcMigration.mockResolvedValue({
      status: 'ok',
      data: packPlan({
        ...DIFFERS,
        current: { ...CURRENT, loaders: [] },
        other_versions_for_instance: 1,
        alternatives_by_loader: false,
      }),
    });
    renderDialog();

    await waitFor(() => expect(screen.getByTestId('migration-pack-switch')).toBeTruthy());
    await fireEvent.click(screen.getByTestId('migration-pack-switch'));
    expect(modpacksNav.value).toEqual({
      openDrawerForInstance: 'inst-1',
      openSwitch: { mcFilter: '26.1.1' },
    });
  });

  it('names the loader in the count whenever any pack version lists one', async () => {
    // A CurseForge pack whose installed file is untagged but whose newer files
    // carry loader tags: the backend counted per loader, so the copy says so.
    modsPlanMcMigration.mockResolvedValue({
      status: 'ok',
      data: packPlan({ ...DIFFERS, current: { ...CURRENT, loaders: [] } }),
    });
    renderDialog();

    await waitFor(() => expect(screen.getByTestId('migration-pack-block')).toBeTruthy());
    expect(screen.getByTestId('migration-pack-block').textContent).toContain(
      'no other version for Minecraft 26.1.1 · Fabric',
    );
  });

  it('a dialog opened while the pack update still runs holds its writes and re-plans after', async () => {
    // Closing never waits for a restore, so the dialog can be reopened while
    // that task is queued or running: the registry, not the component, knows.
    modsPlanMcMigration.mockResolvedValue({ status: 'ok', data: packPlan(DIFFERS) });
    void start({
      id: 'pack-update-1',
      kind: 'pack-update',
      scope: { instanceId: 'inst-1' },
      title: 'Fabulously Optimized',
      phase: null,
      progress: null,
      lane: 'serial',
    });
    renderDialog();

    await waitFor(() => expect(screen.getByTestId('migration-pack-restore')).toBeTruthy());
    await fireEvent.click(screen.getByTestId('migration-disposition-disable-p1'));
    expect((screen.getByTestId('migration-pack-restore') as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByTestId('migration-apply-btn') as HTMLButtonElement).disabled).toBe(true);

    finish('pack-update-1', { state: 'ok' });
    await waitFor(() => expect(modsPlanMcMigration).toHaveBeenCalledTimes(2));
  });

  it('a restore that ends after the dialog closed still reports, but does not re-plan', async () => {
    modsPlanMcMigration.mockResolvedValue({ status: 'ok', data: packPlan(DIFFERS) });
    listInstances.mockResolvedValue({ status: 'ok', data: [INSTANCE] });
    modpackFetchToTemp.mockResolvedValue({ status: 'ok', data: 'C:/tmp/fo.mrpack' });
    modpackComputeUpdate.mockResolvedValue({ status: 'ok', data: RESTORE_DIFF });
    let finishApply: (v: unknown) => void = () => {};
    applyModpackUpdate.mockReturnValue(
      new Promise((r) => {
        finishApply = r;
      }),
    );
    const onApplied = vi.fn();
    const changedBefore = instancesChanged.value;
    const { unmount } = renderDialog(onApplied);

    await waitFor(() => expect(screen.getByTestId('migration-pack-restore')).toBeTruthy());
    await fireEvent.click(screen.getByTestId('migration-pack-restore'));
    await waitFor(() => expect(screen.getByTestId('pack-restore-confirm')).toBeTruthy());
    await fireEvent.click(screen.getByTestId('pack-restore-confirm'));
    await waitFor(() => expect(applyModpackUpdate).toHaveBeenCalledTimes(1));

    unmount();
    finishApply({ status: 'ok' });

    await waitFor(() => expect(onApplied).toHaveBeenCalledTimes(1));
    expect(instancesChanged.value).toBe(changedBefore + 1);
    await new Promise((r) => setTimeout(r, 20));
    expect(modsPlanMcMigration).toHaveBeenCalledTimes(1);
  });

  it('shows why a restore could not start', async () => {
    modsPlanMcMigration.mockResolvedValue({ status: 'ok', data: packPlan(DIFFERS) });
    listInstances.mockResolvedValue({ status: 'ok', data: [INSTANCE] });
    modpackFetchToTemp.mockResolvedValue({
      status: 'error',
      error: { kind: 'network', url: 'https://cdn.modrinth.com', details: 'offline' },
    });
    renderDialog();

    await waitFor(() => expect(screen.getByTestId('migration-pack-restore')).toBeTruthy());
    await fireEvent.click(screen.getByTestId('migration-pack-restore'));
    await waitFor(() => expect(screen.getByTestId('migration-pack-error')).toBeTruthy());
    expect(screen.queryByTestId('pack-restore-dialog')).toBeNull();
  });
});

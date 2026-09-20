import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import type { McMigrationPlan_Serialize, ModVersion_Serialize } from '$lib/ipc/bindings';

const modsPlanMcMigration = vi.fn();
const modsApplyMcMigration = vi.fn();

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsPlanMcMigration: (...a: unknown[]) => modsPlanMcMigration(...a),
    modsApplyMcMigration: (...a: unknown[]) => modsApplyMcMigration(...a),
  },
}));

import MigrationPlanDialog from '$lib/mods/MigrationPlanDialog.svelte';

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
//
// `as unknown as`: the red round runs against the OLD bindings, where
// `unjudged` is a number and `no_platform_build` does not exist. Task G10
// removes the cast once the bindings are regenerated.
const PLAN = {
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
} as unknown as McMigrationPlan_Serialize;

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
      stranded: [{ sha1: 's1', disposition: 'keep' }],
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
      stranded: [{ sha1: 's1', disposition: 'remove' }],
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

    await waitFor(() => expect(screen.getByTestId('migration-disposition-disable-n1')).toBeTruthy());
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

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
// row), one stranded, and two unjudged — every number distinct so an
// assertion that reads "3" can only mean fits, never stranded or unjudged.
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
  unjudged: 2,
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

  it('renders the unjudged count as its own line, distinct from — and not folded into — fits', async () => {
    modsPlanMcMigration.mockResolvedValue({ status: 'ok', data: PLAN });
    renderDialog();

    await waitFor(() => expect(screen.getByTestId('migration-unjudged')).toBeTruthy());
    // unjudged (2) is its own line...
    expect(screen.getByTestId('migration-unjudged').textContent).toContain('2');
    // ...while the summary line still reports the real fits count (3), not
    // 3 + 2 and not 2.
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

  it('keeps Apply disabled — with a visible reason — until every stranded row has a disposition, then enables it', async () => {
    modsPlanMcMigration.mockResolvedValue({ status: 'ok', data: PLAN });
    renderDialog();

    await waitFor(() => expect(screen.getByTestId('migration-apply-btn')).toBeTruthy());
    const applyBtn = screen.getByTestId('migration-apply-btn') as HTMLButtonElement;
    expect(applyBtn.disabled).toBe(true);
    expect(screen.getByTestId('migration-apply-disabled-reason').textContent).toMatch(/choose/i);

    await fireEvent.click(screen.getByTestId('migration-disposition-keep-s1'));

    expect(applyBtn.disabled).toBe(false);
    expect(screen.getByTestId('migration-apply-disabled-reason').textContent).toBe('');
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
});

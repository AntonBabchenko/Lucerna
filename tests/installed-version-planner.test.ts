/**
 * «Fix…» on a version conflict (spec §5.4, §6.5, D8). The Installed tab asks the two-sided planner
 * — the network only on that click — shows what each side would change, and applies only the side
 * the user clicks, through the existing switch flows (download first, the old jar after). A look
 * that failed says it failed; both sides empty is the honest dead end; and a plan made for mods
 * that have changed since is never offered — its «breaks nothing» may no longer be true.
 */
import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { formatError } from '$lib/ipc/format-error';

const h = vi.hoisted(() => {
  const mod = (sha1: string, projectId: string, name: string) => ({
    filename: `${name.toLowerCase()}.jar`,
    sha1,
    source: 'modrinth',
    project_id: projectId,
    version_id: `v-${sha1}`,
    name,
    version_number: '1.0',
    installed_at: '2026-01-01T00:00:00Z',
    enabled: true,
    enrich_attempted: false,
    requires: [],
  });
  return {
    rows: [mod('ind', 'PIND', 'Indium'), mod('sod', 'PSOD', 'Sodium')],
    instanceDependencyPreflight: vi.fn(),
    modsPlanVersionFix: vi.fn(),
    updateMod: vi.fn(),
    installModWithDeps: vi.fn(),
    pushSuccess: vi.fn(),
    pushWarning: vi.fn(),
    // The mod-toggle listener the view registers — fired to stand for a change made elsewhere.
    toggle: null as null | ((e: unknown) => void),
  };
});

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsListInstalled: vi.fn().mockResolvedValue({ status: 'ok', data: h.rows }),
    modsPackOriginSummary: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    modsEnrichPackMods: vi.fn().mockResolvedValue({ status: 'ok', data: 0 }),
    modsProjects: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsCheckUpdates: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsGetCurseforgeKeyStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'set' }),
    modsDependencyGraph: vi.fn().mockResolvedValue({ status: 'ok', data: { roots: [] } }),
    instanceDependencyPreflight: h.instanceDependencyPreflight,
    modsResolveDepNames: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    scanInstanceModCompat: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    checkInstanceModCompat: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsVersions: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    modsPlanVersionFix: h.modsPlanVersionFix,
  },
  events: {
    modInstalled: { listen: () => Promise.resolve(() => {}) },
    modUninstalled: { listen: () => Promise.resolve(() => {}) },
    modToggle: {
      listen: (cb: (e: unknown) => void) => {
        h.toggle = cb;
        return Promise.resolve(() => {});
      },
    },
    modsReconciled: { listen: () => Promise.resolve(() => {}) },
    gpuPrefApplied: { listen: () => Promise.resolve(() => {}) },
  },
}));
vi.mock('$lib/tasks/adapters/mod-install', () => ({
  updateMod: h.updateMod,
  installModWithDeps: h.installModWithDeps,
}));
vi.mock('$lib/toasts/toasts.svelte', () => ({
  pushSuccess: h.pushSuccess,
  pushWarning: h.pushWarning,
  pushActionToast: vi.fn(),
  pushInfo: vi.fn(),
}));

import InstalledModsView from '$lib/mods/installed/InstalledModsView.svelte';

// Indium 1.0 needs Sodium 0.5.x; Sodium 0.6.0 is installed.
const conflict = () => ({
  dependent_sha1: 'ind',
  dependent_name: 'Indium',
  dep_id: 'sodium',
  kind: 'version_out_of_range',
  installed_version: '0.6.0',
  needed: '0.5.x',
  needed_desc: {
    raw: '0.5.x',
    family: 'fabric_predicate',
    alternatives: [],
    unparseable: true,
    soft: false,
  },
  provider_project: { source: 'modrinth', project_id: 'PSOD', version_id: null },
  provider_sha1: 'sod',
  family: 'fabric_predicate',
});
const build = (projectId: string, n: string) => ({
  source: 'modrinth',
  project_id: projectId,
  version_id: `${projectId}-${n}`,
  name: `Build ${n}`,
  version_number: n,
  mc_versions: ['1.21.1'],
  loaders: ['fabric'],
  primary_file: {
    url: 'https://cdn/x.jar',
    filename: 'x.jar',
    sha1: 'f',
    size: 1,
    distribution_allowed: true,
  },
  deps: [],
  published_at: null,
});
const indium2 = build('PIND', '2.0');
const sodium0511 = build('PSOD', '0.5.11');
const ok = <T>(data: T) => ({ status: 'ok' as const, data });
const bothSides = () =>
  ok({
    update_dependent: { version: indium2, breaks: [] },
    change_provider: { version: sodium0511, direction: 'downgrade', breaks: [] },
  });

// A DISTINCT instance id per case: the pre-flight cache is a per-instance LRU.
const props = (instanceId: string) => ({
  instanceId,
  mcVersion: '1.21.1',
  loader: 'fabric' as const,
});
const panel = () => screen.findByTestId('preflight-panel');

beforeEach(() => {
  h.instanceDependencyPreflight.mockReset();
  h.instanceDependencyPreflight.mockImplementation(async () => ok({ violations: [conflict()] }));
  h.modsPlanVersionFix.mockReset();
  h.updateMod.mockReset();
  h.updateMod.mockResolvedValue(ok({}));
  h.pushSuccess.mockReset();
  h.pushWarning.mockReset();
});

describe('«Fix…» on a version conflict', () => {
  it('asks the planner only on the click, then updates the dependent the user picked', async () => {
    h.modsPlanVersionFix.mockResolvedValue(bothSides());
    render(InstalledModsView, { props: props('plan-dependent') });
    const p = await panel();
    expect(h.modsPlanVersionFix).not.toHaveBeenCalled();

    await fireEvent.click(within(p).getByRole('button', { name: 'Fix…' }));
    expect(h.modsPlanVersionFix).toHaveBeenCalledWith('plan-dependent', 'ind', 'sodium');
    const dependent = await within(p).findByTestId('preflight-plan-dependent');
    expect(dependent.textContent?.trim()).toBe('Update Indium to 2.0');
    expect(within(p).getByTestId('preflight-plan-provider').textContent?.trim()).toBe(
      'Roll Sodium back to 0.5.11',
    );

    await fireEvent.click(dependent);
    // The switch flow: one update command replacing Indium's jar, named after the mod.
    await waitFor(() =>
      expect(h.updateMod).toHaveBeenCalledWith('plan-dependent', 'Indium', 'ind', indium2, {}),
    );
    expect(h.updateMod).toHaveBeenCalledTimes(1);
  });

  it('changes the dependency instead when that is the side clicked', async () => {
    h.modsPlanVersionFix.mockResolvedValue(bothSides());
    render(InstalledModsView, { props: props('plan-provider') });
    const p = await panel();
    await fireEvent.click(within(p).getByRole('button', { name: 'Fix…' }));
    await fireEvent.click(await within(p).findByTestId('preflight-plan-provider'));
    // Sodium's own jar is the one replaced — never a second copy beside it.
    await waitFor(() =>
      expect(h.updateMod).toHaveBeenCalledWith('plan-provider', 'Build 0.5.11', 'sod', sodium0511, {
        allowOffPlatform: false,
      }),
    );
    expect(h.updateMod).toHaveBeenCalledTimes(1);
  });

  it('the row’s «Fix…» asks the same planner and hands focus to its offers', async () => {
    h.modsPlanVersionFix.mockResolvedValue(bothSides());
    render(InstalledModsView, { props: props('plan-row') });
    const line = await screen.findByTestId('row-problem');
    const fix = within(line).getByRole('button', { name: 'Fix…' });
    fix.focus();
    await fireEvent.click(fix);
    expect(h.modsPlanVersionFix).toHaveBeenCalledWith('plan-row', 'ind', 'sodium');
    const offer = await within(await panel()).findByTestId('preflight-plan-dependent');
    await waitFor(() => expect(document.activeElement).toBe(offer));
  });

  it('an answered conflict is not asked about again — «Fix…» takes the user to the answer', async () => {
    h.modsPlanVersionFix.mockResolvedValue(bothSides());
    render(InstalledModsView, { props: props('plan-answered') });
    const p = await panel();
    await fireEvent.click(within(p).getByRole('button', { name: 'Fix…' }));
    const offer = await within(p).findByTestId('preflight-plan-dependent');

    const fix = within(screen.getByTestId('row-problem')).getByRole('button', { name: 'Fix…' });
    fix.focus();
    await fireEvent.click(fix);
    await waitFor(() => expect(document.activeElement).toBe(offer));
    expect(h.modsPlanVersionFix).toHaveBeenCalledTimes(1);
  });

  it('says a look that failed failed, and why — never «no compatible version»', async () => {
    const error = { kind: 'mods_network', url: 'https://api.modrinth.com', details: 'offline' };
    h.modsPlanVersionFix.mockResolvedValue({ status: 'error', error });
    render(InstalledModsView, { props: props('plan-failed') });
    const p = await panel();
    await fireEvent.click(within(p).getByRole('button', { name: 'Fix…' }));
    expect(
      await within(p).findByText(`Couldn't look for a fix: ${formatError(error as never)}`),
    ).toBeTruthy();
    expect(within(p).queryByText('No compatible version')).toBeNull();
    // Still the way to try again.
    expect(within(p).getByRole('button', { name: 'Fix…' })).toBeTruthy();
  });

  it('both sides empty is the honest dead end', async () => {
    h.modsPlanVersionFix.mockResolvedValue(ok({ update_dependent: null, change_provider: null }));
    render(InstalledModsView, { props: props('plan-dead-end') });
    const p = await panel();
    await fireEvent.click(within(p).getByRole('button', { name: 'Fix…' }));
    expect(await within(p).findByText('No compatible version')).toBeTruthy();
    expect(within(p).getByText('Find alternative')).toBeTruthy();
    expect(within(p).queryByRole('button', { name: 'Fix…' })).toBeNull();
  });

  it('never offers a plan made for mods that changed while it was being made (D8)', async () => {
    let answer: (r: unknown) => void = () => {};
    h.modsPlanVersionFix.mockImplementation(() => new Promise((resolve) => (answer = resolve)));
    render(InstalledModsView, { props: props('plan-stale') });
    const p = await panel();
    await fireEvent.click(within(p).getByRole('button', { name: 'Fix…' }));
    const checks = h.instanceDependencyPreflight.mock.calls.length;

    // A mod is switched elsewhere: the pre-flight reads the folder again.
    h.toggle?.({ payload: { instance_id: 'plan-stale' } });
    await waitFor(
      () => expect(h.instanceDependencyPreflight.mock.calls.length).toBeGreaterThan(checks),
      { timeout: 2000 },
    );
    await waitFor(() => expect(within(p).getByRole('button', { name: 'Fix…' })).toBeTruthy());
    answer(bothSides());
    await new Promise((r) => setTimeout(r, 0));

    expect(within(p).queryByTestId('preflight-plan-dependent')).toBeNull();
    expect(within(p).queryByTestId('preflight-plan-provider')).toBeNull();
  });

  it('drops offers once the mods change — they answered for the mods as they were', async () => {
    h.modsPlanVersionFix.mockResolvedValue(bothSides());
    render(InstalledModsView, { props: props('plan-cleared') });
    const p = await panel();
    await fireEvent.click(within(p).getByRole('button', { name: 'Fix…' }));
    await within(p).findByTestId('preflight-plan-dependent');

    h.toggle?.({ payload: { instance_id: 'plan-cleared' } });
    await waitFor(() => expect(within(p).queryByTestId('preflight-plan-dependent')).toBeNull(), {
      timeout: 2000,
    });
    expect(within(p).getByRole('button', { name: 'Fix…' })).toBeTruthy();
  });

  // A mod write takes the SHARED claim: it is refused only while a long operation holds the
  // profile, never because the game runs (plan A9).
  it('a switch the profile refuses says it is busy — never that the game runs', async () => {
    h.modsPlanVersionFix.mockResolvedValue(bothSides());
    h.updateMod.mockResolvedValue({ status: 'error', error: { kind: 'instance_busy' } });
    render(InstalledModsView, { props: props('plan-busy') });
    const p = await panel();
    await fireEvent.click(within(p).getByRole('button', { name: 'Fix…' }));
    await fireEvent.click(await within(p).findByTestId('preflight-plan-dependent'));
    await waitFor(() =>
      expect(h.pushWarning).toHaveBeenCalledWith('Mod install failed', [
        'Another operation — such as a modpack update, a migration or a clone — is using this profile. Try again once it finishes.',
      ]),
    );
  });

  // A switch is an update (`mods_update_one`): the dependencies the new build brought in are said,
  // never installed silently (spec D9, review F7).
  it('a switch names the dependencies it installed', async () => {
    h.modsPlanVersionFix.mockResolvedValue(bothSides());
    h.updateMod.mockResolvedValue(
      ok({ primary_name: 'Build 2.0', installed_dependencies: ['Balm'], details: [] }),
    );
    render(InstalledModsView, { props: props('plan-deps') });
    const p = await panel();
    await fireEvent.click(within(p).getByRole('button', { name: 'Fix…' }));
    await fireEvent.click(await within(p).findByTestId('preflight-plan-dependent'));
    await waitFor(() =>
      expect(h.pushSuccess).toHaveBeenCalledWith('Installed Indium', ['+ installed: Balm']),
    );
  });
});

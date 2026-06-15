import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

const executeRepair = vi.fn();
const modsVersions = vi.fn();
const pushSuccess = vi.fn();
const pushWarning = vi.fn();

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    executeRepair: (...a: unknown[]) => executeRepair(...a),
    modsVersions: (...a: unknown[]) => modsVersions(...a),
  },
}));

vi.mock('$lib/toasts/toasts.svelte', () => ({
  pushSuccess: (...a: unknown[]) => pushSuccess(...a),
  pushWarning: (...a: unknown[]) => pushWarning(...a),
}));

import type { RepairPlan } from '$lib/ipc/bindings';
import BlockingModsRepairCard from '$lib/logs/BlockingModsRepairCard.svelte';

type BlockingPlan = Extract<RepairPlan, { kind: 'disable_blocking_mods' }>;
type BlockingMod = BlockingPlan['mods'][number];

// Mods may omit source/project_id in fixtures; planOf fills them so a mod is
// "replaceable" (source modrinth, project_id = mod_id) by default. Pass
// source: null explicitly to model a manually-added jar (fallback path).
type ModInput = Pick<BlockingMod, 'sha1' | 'mod_id' | 'name' | 'breaks'> &
  Partial<Pick<BlockingMod, 'source' | 'project_id'>>;

function planOf(mods: ModInput[]): BlockingPlan {
  return {
    kind: 'disable_blocking_mods',
    mods: mods.map((m): BlockingMod => ({
      sha1: m.sha1,
      mod_id: m.mod_id,
      name: m.name,
      breaks: m.breaks,
      source: 'source' in m ? (m.source ?? null) : 'modrinth',
      project_id: 'project_id' in m ? (m.project_id ?? null) : m.mod_id,
    })),
  };
}

function openDisclosure(sha1: string): void {
  const summary = screen.getByTestId(`blocking-disclosure-${sha1}`).querySelector('summary');
  if (summary) void fireEvent.click(summary);
}

afterEach(() => vi.clearAllMocks());

describe('BlockingModsRepairCard', () => {
  it('leads with guidance and lists each mod by its cited mod-id', () => {
    render(BlockingModsRepairCard, {
      props: {
        plan: planOf([
          { sha1: 'a', mod_id: 'alexsmobs', name: "Alex's Mobs", breaks: [] },
          { sha1: 'c', mod_id: 'citadel', name: 'Citadel', breaks: [] },
        ]),
        instanceId: 'inst-1',
        onClose: vi.fn(),
      },
    });
    // The card leads with the guidance block, not a prominent action.
    expect(screen.getByTestId('blocking-intro')).toBeTruthy();
    // Rows label mods by cited mod-id, not a filename-derived name.
    expect(screen.getByText('alexsmobs')).toBeTruthy();
    expect(screen.getByText('citadel')).toBeTruthy();
  });

  it('points users at the disconnect screen "Server has" column for the version', () => {
    render(BlockingModsRepairCard, {
      props: {
        plan: planOf([{ sha1: 'a', mod_id: 'alexsmobs', name: "Alex's Mobs", breaks: [] }]),
        instanceId: 'inst-1',
        onClose: vi.fn(),
      },
    });
    // The version-mismatch path is only actionable if the user knows where to
    // read the target version: the FML disconnect screen's "Server has" column.
    // Pinning this literal is the feature contract, not incidental coupling — a
    // reword that drops the pointer should fail here.
    expect(screen.getByTestId('blocking-intro').textContent).toContain('Server has');
  });

  it('keeps each Disable action behind a collapsed per-mod disclosure', () => {
    render(BlockingModsRepairCard, {
      props: {
        plan: planOf([{ sha1: 'a', mod_id: 'alexsmobs', name: "Alex's Mobs", breaks: [] }]),
        instanceId: 'inst-1',
        onClose: vi.fn(),
      },
    });
    const disclosure = screen.getByTestId('blocking-disclosure-a') as HTMLDetailsElement;
    expect(disclosure.tagName).toBe('DETAILS');
    expect(disclosure.open).toBe(false);
    // The Disable button lives inside that disclosure, not at the top level.
    const disableBtn = screen.getByTestId('blocking-disable-a');
    expect(disclosure.contains(disableBtn)).toBe(true);
  });

  it('shows a breaks warning inside the disclosure only for mods that break a kept dependent', () => {
    render(BlockingModsRepairCard, {
      props: {
        plan: planOf([
          { sha1: 'c', mod_id: 'citadel', name: 'Citadel', breaks: ["Alex's Mobs"] },
          { sha1: 'x', mod_id: 'standalone', name: 'Standalone', breaks: [] },
        ]),
        instanceId: 'inst-1',
        onClose: vi.fn(),
      },
    });
    const breaks = screen.getByTestId('blocking-breaks-c');
    expect((screen.getByTestId('blocking-disclosure-c') as HTMLElement).contains(breaks)).toBe(
      true,
    );
    expect(screen.queryByTestId('blocking-breaks-x')).toBeNull();
  });

  it('disables a mod via execute_repair and locks the row afterwards', async () => {
    executeRepair.mockResolvedValue({ status: 'ok', data: null });
    render(BlockingModsRepairCard, {
      props: {
        plan: planOf([{ sha1: 'a', mod_id: 'alexsmobs', name: "Alex's Mobs", breaks: [] }]),
        instanceId: 'inst-1',
        onClose: vi.fn(),
      },
    });
    openDisclosure('a');
    await fireEvent.click(screen.getByTestId('blocking-disable-a'));
    await waitFor(() =>
      expect(executeRepair).toHaveBeenCalledWith('inst-1', { kind: 'disable_mod', sha1: 'a' }),
    );
    await waitFor(() =>
      expect((screen.getByTestId('blocking-disable-a') as HTMLButtonElement).disabled).toBe(true),
    );
  });

  it('warns and leaves the row actionable when disable fails', async () => {
    executeRepair.mockResolvedValue({ status: 'error', error: 'net::ERR' });
    render(BlockingModsRepairCard, {
      props: {
        plan: planOf([{ sha1: 'a', mod_id: 'alexsmobs', name: "Alex's Mobs", breaks: [] }]),
        instanceId: 'inst-1',
        onClose: vi.fn(),
      },
    });
    openDisclosure('a');
    await fireEvent.click(screen.getByTestId('blocking-disable-a'));
    await waitFor(() => expect(pushWarning).toHaveBeenCalled());
    // The disable failed → the row stays actionable so the user can retry.
    expect((screen.getByTestId('blocking-disable-a') as HTMLButtonElement).disabled).toBe(false);
  });

  it('shows a done signal once every mod is disabled', async () => {
    executeRepair.mockResolvedValue({ status: 'ok', data: null });
    render(BlockingModsRepairCard, {
      props: {
        plan: planOf([{ sha1: 'a', mod_id: 'alexsmobs', name: "Alex's Mobs", breaks: [] }]),
        instanceId: 'inst-1',
        onClose: vi.fn(),
      },
    });
    expect(screen.queryByTestId('blocking-all-disabled')).toBeNull();
    openDisclosure('a');
    await fireEvent.click(screen.getByTestId('blocking-disable-a'));
    await waitFor(() => expect(screen.getByTestId('blocking-all-disabled')).toBeTruthy());
  });

  // ── Replace-version path (version-mismatch fix) ──────────────────────────

  it('offers Replace version for a mod with a known source', () => {
    render(BlockingModsRepairCard, {
      props: {
        plan: planOf([
          { sha1: 'a', mod_id: 'sophisticatedbackpacks', name: 'Sophisticated Backpacks', breaks: [] },
        ]),
        instanceId: 'inst-1',
        mcVersion: '1.20.1',
        loader: 'forge',
        onClose: vi.fn(),
      },
    });
    expect(screen.getByTestId('blocking-replace-a')).toBeTruthy();
  });

  it('hides Replace version for a sourceless (manual) mod, keeping Disable', () => {
    render(BlockingModsRepairCard, {
      props: {
        plan: planOf([
          { sha1: 'a', mod_id: 'manualmod', name: 'Manual Mod', breaks: [], source: null, project_id: null },
        ]),
        instanceId: 'inst-1',
        mcVersion: '1.20.1',
        loader: 'forge',
        onClose: vi.fn(),
      },
    });
    expect(screen.queryByTestId('blocking-replace-a')).toBeNull();
    expect(screen.getByTestId('blocking-disclosure-a')).toBeTruthy();
  });

  it('installs the picked version via executeRepair reinstall', async () => {
    modsVersions.mockResolvedValue({
      status: 'ok',
      data: [
        {
          source: 'modrinth',
          project_id: 'sophisticatedbackpacks',
          version_id: 'v-3245',
          name: 'SB 3.24.53',
          version_number: '3.24.53',
          mc_versions: ['1.20.1'],
          loaders: ['forge'],
          primary_file: {
            filename: 'sb.jar',
            url: 'https://example/sb.jar',
            sha1: 'aa',
            size: 1,
            distribution_allowed: true,
          },
          deps: [],
          published_at: null,
        },
      ],
    });
    executeRepair.mockResolvedValue({ status: 'ok', data: null });

    render(BlockingModsRepairCard, {
      props: {
        plan: planOf([
          { sha1: 'a', mod_id: 'sophisticatedbackpacks', name: 'Sophisticated Backpacks', breaks: [] },
        ]),
        instanceId: 'inst-1',
        mcVersion: '1.20.1',
        loader: 'forge',
        onClose: vi.fn(),
      },
    });

    await fireEvent.click(screen.getByTestId('blocking-replace-a'));
    await waitFor(() =>
      expect(modsVersions).toHaveBeenCalledWith(
        'modrinth',
        'sophisticatedbackpacks',
        '1.20.1',
        'forge',
      ),
    );

    // Drive the themed Select (custom combobox + listbox): open it, then commit
    // the option via mousedown (the control commits on onmousedown, not click).
    await waitFor(() => expect(screen.getByTestId('blocking-version-select-a')).toBeTruthy());
    await fireEvent.click(screen.getByRole('combobox'));
    await fireEvent.mouseDown(screen.getByRole('option', { name: '3.24.53' }));

    await waitFor(() =>
      expect((screen.getByTestId('blocking-install-version-a') as HTMLButtonElement).disabled).toBe(
        false,
      ),
    );
    await fireEvent.click(screen.getByTestId('blocking-install-version-a'));
    await waitFor(() =>
      expect(executeRepair).toHaveBeenCalledWith('inst-1', {
        kind: 'reinstall',
        old_sha1: 'a',
        target: { source: 'modrinth', project_id: 'sophisticatedbackpacks', version_id: 'v-3245' },
      }),
    );
  });
});

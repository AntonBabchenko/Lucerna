import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

const executeRepair = vi.fn();
const pushSuccess = vi.fn();
const pushWarning = vi.fn();

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    executeRepair: (...a: unknown[]) => executeRepair(...a),
  },
}));

vi.mock('$lib/toasts/toasts.svelte', () => ({
  pushSuccess: (...a: unknown[]) => pushSuccess(...a),
  pushWarning: (...a: unknown[]) => pushWarning(...a),
}));

import type { RepairPlan } from '$lib/ipc/bindings';
import BlockingModsRepairCard from '$lib/logs/BlockingModsRepairCard.svelte';

type BlockingPlan = Extract<RepairPlan, { kind: 'disable_blocking_mods' }>;

function planOf(mods: BlockingPlan['mods']): BlockingPlan {
  return { kind: 'disable_blocking_mods', mods };
}

afterEach(() => vi.clearAllMocks());

describe('BlockingModsRepairCard', () => {
  it('renders a Disable button per blocking mod', () => {
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
    expect(screen.getByTestId('blocking-disable-a')).toBeTruthy();
    expect(screen.getByTestId('blocking-disable-c')).toBeTruthy();
    // The row labels the mod by its cited mod-id, not a filename-derived name.
    expect(screen.getByText('alexsmobs')).toBeTruthy();
    expect(screen.getByText('citadel')).toBeTruthy();
  });

  it('shows a breaks warning only for mods that break a kept dependent', () => {
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
    expect(screen.getByTestId('blocking-breaks-c')).toBeTruthy();
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
    await fireEvent.click(screen.getByTestId('blocking-disable-a'));
    await waitFor(() => expect(screen.getByTestId('blocking-all-disabled')).toBeTruthy());
  });
});

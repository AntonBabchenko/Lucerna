import { fireEvent, render } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { RepairPlan } from '$lib/ipc/bindings';
import FixModRepairCard from '$lib/logs/FixModRepairCard.svelte';

function resolvablePlan(): Extract<RepairPlan, { kind: 'install_fix_mod' }> {
  return {
    kind: 'install_fix_mod',
    title: 'TFMG Lag Fix',
    source: 'curseforge',
    slug: 'tfmg-lag-fix',
    version_label: '1.0.3',
    install: { source: 'curseforge', project_id: '123', version_id: '456' },
  };
}

describe('FixModRepairCard', () => {
  it('install button sends the install_fix_mod choice', async () => {
    const onConfirm = vi.fn();
    const { getByTestId } = render(FixModRepairCard, {
      props: { plan: resolvablePlan(), onConfirm, onCancel: () => {} },
    });
    await fireEvent.click(getByTestId('fix-mod-install'));
    expect(onConfirm).toHaveBeenCalledWith({
      kind: 'install_fix_mod',
      source: 'curseforge',
      project_id: '123',
      version_id: '456',
    });
  });

  it('degraded plan (install=null) shows the open-project affordance, no install', () => {
    const { queryByTestId, getByTestId } = render(FixModRepairCard, {
      props: {
        plan: { ...resolvablePlan(), install: null, version_label: null },
        onConfirm: () => {},
        onCancel: () => {},
      },
    });
    expect(queryByTestId('fix-mod-install')).toBeNull();
    expect(getByTestId('fix-mod-open')).toBeTruthy();
  });
});

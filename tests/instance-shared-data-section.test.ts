import { fireEvent, render, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import type { InstanceWithStatus } from '$lib/ipc/bindings';

// The Settings tab's Shared data section (spec 2026-10-15 §A1): one button
// that opens the transfer dialog, locked while the game runs or a task holds
// the instance, with copy that says which.

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    transferScan: vi.fn(),
    runningInstances: vi.fn().mockResolvedValue([]),
  },
  events: {
    processSpawned: { listen: () => Promise.resolve(() => {}) },
    processExited: { listen: () => Promise.resolve(() => {}) },
  },
}));

import InstanceSharedDataSection from '$lib/instances/settings/InstanceSharedDataSection.svelte';

const inst = {
  id: 'inst-1',
  name: 'New pack',
  mc_version: '1.21.1',
  loader: 'fabric',
} as InstanceWithStatus;
const other = { ...inst, id: 'inst-2', name: 'Old pack' } as InstanceWithStatus;

function renderSection(over: { running?: boolean; busy?: boolean } = {}) {
  return render(InstanceSharedDataSection, {
    props: {
      instance: inst,
      instances: [inst, other],
      running: over.running ?? false,
      busy: over.busy ?? false,
    },
  });
}

beforeEach(() => {
  locale.set('en');
});

describe('InstanceSharedDataSection', () => {
  it('opens the transfer dialog when idle', async () => {
    renderSection();
    const button = screen.getByTestId('shared-data-copy') as HTMLButtonElement;
    expect(button.disabled).toBe(false);
    await fireEvent.click(button);
    expect(await screen.findByTestId('transfer-data-dialog')).toBeTruthy();
  });

  it('is locked while the game runs, and says so', () => {
    renderSection({ running: true });
    expect((screen.getByTestId('shared-data-copy') as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByTestId('shared-data-locked').textContent).toContain('Stop the game');
  });

  it('is locked while a task holds the instance, and says so', () => {
    renderSection({ busy: true });
    expect((screen.getByTestId('shared-data-copy') as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByTestId('shared-data-locked').textContent).toContain('current task');
  });
});

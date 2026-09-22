import { render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

vi.mock('$lib/ipc/bindings', async (importOriginal) => {
  const actual = await importOriginal<typeof import('$lib/ipc/bindings')>();
  return {
    ...actual,
    commands: {
      ...actual.commands,
      beginMicrosoftSignin: vi.fn().mockResolvedValue({ status: 'ok', data: {} }),
      getDataLocation: vi.fn().mockResolvedValue({
        status: 'ok',
        data: {
          effective: 'C:\\Users\\u\\AppData\\Local\\com.lucerna.app\\recovery\\4242',
          configured: 'D:\\LucernaData',
          fell_back: true,
          fallback: { kind: 'root_missing' },
          default_dir: 'C:\\Default',
          relocation: { kind: 'idle' },
        },
      }),
    },
  };
});

import Sidebar from '$lib/layout/Sidebar.svelte';
import { dataLocation } from '$lib/settings/data-location.svelte';

function baseProps() {
  return {
    accounts: [],
    activeAccount: null,
    instances: [],
    activeInstance: null,
    onSelectAccount: vi.fn(),
    onRemoveAccount: vi.fn(),
    onOpenCosmetics: vi.fn(),
    onAddOffline: vi.fn(),
    onSelectInstance: vi.fn(),
    onOpenManage: vi.fn(),
    onOpenMods: vi.fn(),
    onOpenLogs: vi.fn(),
    onOpenModpacks: vi.fn(),
    onOpenLauncherImport: vi.fn(),
    running: null,
    installing: false,
    onPlay: vi.fn(),
    onStop: vi.fn(),
    onInstall: vi.fn(),
  };
}

describe('Sidebar in a recovery session', () => {
  it('says the instances and accounts are in the unavailable folder — not that there are none', async () => {
    // With nothing seeded the lists ARE empty, and "No instances yet" reads as data loss: the
    // very thing the recovery session exists to avoid saying.
    await dataLocation.refresh();
    render(Sidebar, { props: { ...baseProps(), instancesLoaded: true } });
    expect(screen.queryByText('No instances yet.')).toBeNull();
    expect(screen.getByText(/Your instances are in the data folder/)).toBeTruthy();
    expect(screen.queryByText(/No accounts yet/)).toBeNull();
    expect(screen.getByText(/Your accounts are in the data folder/)).toBeTruthy();
  });
});

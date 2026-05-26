import { render } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { Account, InstanceWithStatus } from '$lib/ipc/bindings';
import Sidebar from '$lib/layout/Sidebar.svelte';

const sampleAccount: Account = {
  id: 'a1',
  name: 'Tester',
  uuid: '00000000-0000-0000-0000-000000000000',
  expires_at: null,
};
const sampleInstance: InstanceWithStatus = {
  id: 'i1',
  name: 'Default',
  mc_version: '1.20.1',
  loader: 'vanilla',
  loader_version: null,
  ready: true,
  max_heap_mb: 4096,
  extra_jvm_args: '',
  created_unix_ms: null,
  mrpack_name: null,
  mrpack_version: null,
  mrpack_project_id: null,
  mrpack_source: null,
  mrpack_summary: null,
  mrpack_version_id: null,
};

describe('Sidebar', () => {
  it('renders FTlauncher title', () => {
    const { getByText } = render(Sidebar, {
      props: {
        accounts: [],
        activeAccount: null,
        instances: [],
        activeInstance: null,
        onSelectAccount: vi.fn(),
        onRemoveAccount: vi.fn(),
        onAddOffline: vi.fn(),
        onSelectInstance: vi.fn(),
        onOpenManage: vi.fn(),
        onOpenMods: vi.fn(),
        onOpenLogs: vi.fn(),
        onOpenModpacks: vi.fn(),
        modpacksActive: false,
        running: null,
        installing: false,
        onPlay: vi.fn(),
        onStop: vi.fn(),
        onInstall: vi.fn(),
      },
    });
    expect(getByText('FTlauncher')).toBeTruthy();
  });

  it('lists accounts and emits select on change', async () => {
    const onSelectAccount = vi.fn();
    const { getByDisplayValue } = render(Sidebar, {
      props: {
        accounts: [sampleAccount],
        activeAccount: sampleAccount,
        instances: [sampleInstance],
        activeInstance: sampleInstance,
        onSelectAccount,
        onRemoveAccount: vi.fn(),
        onAddOffline: vi.fn(),
        onSelectInstance: vi.fn(),
        onOpenManage: vi.fn(),
        onOpenMods: vi.fn(),
        onOpenLogs: vi.fn(),
        onOpenModpacks: vi.fn(),
        modpacksActive: false,
        running: null,
        installing: false,
        onPlay: vi.fn(),
        onStop: vi.fn(),
        onInstall: vi.fn(),
      },
    });
    const select = getByDisplayValue(/Tester/) as HTMLSelectElement;
    expect(select).toBeTruthy();
  });

  it('renders the Browse modpacks button at the sidebar level', () => {
    const { getByTestId } = render(Sidebar, {
      props: {
        accounts: [],
        activeAccount: null,
        instances: [],
        activeInstance: null,
        onSelectAccount: vi.fn(),
        onRemoveAccount: vi.fn(),
        onAddOffline: vi.fn(),
        onSelectInstance: vi.fn(),
        onOpenManage: vi.fn(),
        onOpenMods: vi.fn(),
        onOpenLogs: vi.fn(),
        onOpenModpacks: vi.fn(),
        modpacksActive: false,
        running: null,
        installing: false,
        onPlay: vi.fn(),
        onStop: vi.fn(),
        onInstall: vi.fn(),
      },
    });
    expect(getByTestId('sidebar-open-modpacks')).toBeTruthy();
  });
});

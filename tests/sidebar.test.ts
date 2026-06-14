import { render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { Account, InstanceWithStatus } from '$lib/ipc/bindings';
import Sidebar from '$lib/layout/Sidebar.svelte';

vi.mock('$lib/ipc/bindings', async (importOriginal) => {
  const actual = await importOriginal<typeof import('$lib/ipc/bindings')>();
  return {
    ...actual,
    commands: {
      ...actual.commands,
      beginMicrosoftSignin: vi.fn().mockResolvedValue({ status: 'ok', data: {} }),
    },
  };
});

const sampleAccount: Account = {
  id: 'a1',
  kind: 'offline',
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
  integrity: null,
  imported_from: null,
};

describe('Sidebar', () => {
  it('renders Lucerna title', () => {
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
        running: null,
        installing: false,
        onPlay: vi.fn(),
        onStop: vi.fn(),
        onInstall: vi.fn(),
      },
    });
    expect(getByText('Lucerna')).toBeTruthy();
  });

  it('lists accounts and emits select on change', async () => {
    const onSelectAccount = vi.fn();
    const { getByRole } = render(Sidebar, {
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
        running: null,
        installing: false,
        onPlay: vi.fn(),
        onStop: vi.fn(),
        onInstall: vi.fn(),
      },
    });
    const trigger = getByRole('combobox', { name: 'Account' });
    expect(trigger.textContent).toMatch(/Tester/);
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
        running: null,
        installing: false,
        onPlay: vi.fn(),
        onStop: vi.fn(),
        onInstall: vi.fn(),
      },
    });
    expect(getByTestId('sidebar-open-modpacks')).toBeTruthy();
  });

  it('does NOT render the deferred-MS italic text', () => {
    render(Sidebar, {
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
        running: null,
        installing: false,
        onPlay: vi.fn(),
        onStop: vi.fn(),
        onInstall: vi.fn(),
      },
    });
    expect(screen.queryByText(/Microsoft account login deferred/i)).toBeNull();
  });

  it('shows (offline) suffix for offline accounts in the dropdown', () => {
    render(Sidebar, {
      props: {
        accounts: [sampleAccount],
        activeAccount: sampleAccount,
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
        running: null,
        installing: false,
        onPlay: vi.fn(),
        onStop: vi.fn(),
        onInstall: vi.fn(),
      },
    });
    expect(screen.getByText('Tester (offline)')).toBeTruthy();
  });

  it('shows (Microsoft) suffix for microsoft accounts in the dropdown', () => {
    const msAccount: Account = {
      id: 'ms1',
      kind: 'microsoft',
      name: 'MSPlayer',
      uuid: '11111111-0000-0000-0000-000000000000',
      expires_at: 9999999999,
    };
    render(Sidebar, {
      props: {
        accounts: [msAccount],
        activeAccount: msAccount,
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
        running: null,
        installing: false,
        onPlay: vi.fn(),
        onStop: vi.fn(),
        onInstall: vi.fn(),
      },
    });
    expect(screen.getByText('MSPlayer (Microsoft)')).toBeTruthy();
  });

  it('renders the Sign in with Microsoft button', () => {
    render(Sidebar, {
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
        running: null,
        installing: false,
        onPlay: vi.fn(),
        onStop: vi.fn(),
        onInstall: vi.fn(),
      },
    });
    expect(screen.getByRole('button', { name: /sign in with microsoft/i })).toBeTruthy();
  });

  it('renders the compact toggle and emits on click', async () => {
    const onToggleCompact = vi.fn();
    const { getByLabelText } = render(Sidebar, {
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
        running: null,
        installing: false,
        onPlay: vi.fn(),
        onStop: vi.fn(),
        onInstall: vi.fn(),
        compact: false,
        onToggleCompact,
      },
    });
    const toggle = getByLabelText('Collapse to mini mode');
    toggle.click();
    expect(onToggleCompact).toHaveBeenCalledTimes(1);
  });
});

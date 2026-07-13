import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { Account, InstanceWithStatus } from '$lib/ipc/bindings';
import Sidebar from '$lib/layout/Sidebar.svelte';

function offlineAccount(over: Partial<Account> = {}): Account {
  return {
    id: 'of-1',
    kind: 'offline',
    name: 'Steve',
    uuid: '00000000-0000-0000-0000-000000000001',
    expires_at: null,
    ...over,
  };
}

function microsoftAccount(over: Partial<Account> = {}): Account {
  return {
    id: 'ms-1',
    kind: 'microsoft',
    name: 'Alex',
    uuid: '00000000-0000-0000-0000-000000000002',
    expires_at: null,
    ...over,
  };
}

function instance(over: Partial<InstanceWithStatus> = {}): InstanceWithStatus {
  return {
    id: 'inst-1',
    name: 'Default',
    mc_version: '1.20.4',
    loader: 'vanilla',
    loader_version: null,
    max_heap_mb: 2048,
    min_heap_mb: null,
    extra_jvm_args: '',
    created_unix_ms: null,
    ready: true,
    has_icon: false,
    mrpack_name: null,
    mrpack_version: null,
    mrpack_project_id: null,
    mrpack_source: null,
    mrpack_summary: null,
    mrpack_version_id: null,
    integrity: null,
    imported_from: null,
    created_from_server: null,
    ...over,
  };
}

function handlers() {
  return {
    onSelectAccount: () => {},
    onRemoveAccount: () => {},
    onOpenCosmetics: vi.fn(),
    onOpenSkinEditor: vi.fn(),
    onAddOffline: () => {},
    onSelectInstance: () => {},
    onOpenManage: () => {},
    onOpenMods: () => {},
    onOpenLogs: () => {},
    onOpenModpacks: () => {},
    onOpenLauncherImport: () => {},
    onPlay: () => {},
    onStop: () => {},
    onInstall: () => {},
  };
}

function baseProps(h: ReturnType<typeof handlers>) {
  return {
    ...h,
    accounts: [offlineAccount()],
    activeAccount: offlineAccount(),
    instances: [instance()],
    activeInstance: instance(),
    running: null,
    installing: false,
  };
}

describe('Sidebar — adaptive skin entry button', () => {
  it('offline active account: shows "Skin editor" and opens the editor with that account', async () => {
    const h = handlers();
    render(Sidebar, { props: baseProps(h) });
    const btn = screen.getByRole('button', { name: /skin editor/i });
    await fireEvent.click(btn);
    expect(h.onOpenSkinEditor).toHaveBeenCalledWith(offlineAccount());
    expect(h.onOpenCosmetics).not.toHaveBeenCalled();
  });

  it('microsoft active account: shows "Skin and cape" and opens cosmetics', async () => {
    const h = handlers();
    render(Sidebar, {
      props: { ...baseProps(h), accounts: [microsoftAccount()], activeAccount: microsoftAccount() },
    });
    const btn = screen.getByRole('button', { name: /skin and cape/i });
    await fireEvent.click(btn);
    expect(h.onOpenCosmetics).toHaveBeenCalledWith(microsoftAccount());
    expect(h.onOpenSkinEditor).not.toHaveBeenCalled();
  });

  it('no accounts: shows "Skin editor" and opens the editor with null', async () => {
    const h = handlers();
    render(Sidebar, { props: { ...baseProps(h), accounts: [], activeAccount: null } });
    const btn = screen.getByRole('button', { name: /skin editor/i });
    await fireEvent.click(btn);
    expect(h.onOpenSkinEditor).toHaveBeenCalledWith(null);
  });
});

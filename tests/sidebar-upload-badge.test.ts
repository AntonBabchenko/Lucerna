import { render } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
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

let mockAnyUploading = false;

vi.mock('$lib/servers/server-state.svelte', () => ({
  serverState: {
    get serversNavStatus() {
      return 'idle';
    },
    get anyUploading() {
      return mockAnyUploading;
    },
  },
}));

function baseProps() {
  return {
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
    onOpenLauncherImport: vi.fn(),
    running: null,
    installing: false,
    onPlay: vi.fn(),
    onStop: vi.fn(),
    onInstall: vi.fn(),
  };
}

describe('Sidebar upload badge', () => {
  it('shows the upload badge when anyUploading is true', () => {
    mockAnyUploading = true;
    const { getByTestId } = render(Sidebar, { props: baseProps() });
    expect(getByTestId('sidebar-servers-upload-badge')).toBeTruthy();
  });

  it('does not show the upload badge when anyUploading is false', () => {
    mockAnyUploading = false;
    const { queryByTestId } = render(Sidebar, { props: baseProps() });
    expect(queryByTestId('sidebar-servers-upload-badge')).toBeNull();
  });
});

import { render, screen } from '@testing-library/svelte';
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

describe('Sidebar — instances loading state', () => {
  it('shows a spinner, not the empty state, before the first list load settles', () => {
    render(Sidebar, { props: { ...baseProps(), instancesLoaded: false } });
    expect(screen.getByTestId('sidebar-instances-loading')).toBeTruthy();
    expect(screen.queryByText('No instances yet.')).toBeNull();
    expect(screen.queryByText('+ Create')).toBeNull();
  });

  it('shows the empty state once the list has loaded empty', () => {
    render(Sidebar, { props: { ...baseProps(), instancesLoaded: true } });
    expect(screen.queryByTestId('sidebar-instances-loading')).toBeNull();
    expect(screen.getByText('No instances yet.')).toBeTruthy();
    expect(screen.getByText('+ Create')).toBeTruthy();
  });

  it('defaults to loaded so existing mounts keep the old semantics', () => {
    render(Sidebar, { props: baseProps() });
    expect(screen.queryByTestId('sidebar-instances-loading')).toBeNull();
    expect(screen.getByText('No instances yet.')).toBeTruthy();
  });
});

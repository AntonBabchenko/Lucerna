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

function instance(over: Partial<InstanceWithStatus> = {}): InstanceWithStatus {
  return {
    id: 'inst-1',
    name: 'Default',
    mc_version: '1.20.4',
    loader: 'vanilla',
    loader_version: null,
    max_heap_mb: 2048,
    extra_jvm_args: '',
    created_unix_ms: null,
    ready: false,
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

const noopHandlers = {
  onSelectAccount: () => {},
  onRemoveAccount: () => {},
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

const baseProps = {
  ...noopHandlers,
  accounts: [offlineAccount()],
  activeAccount: offlineAccount(),
  instances: [instance()],
  activeInstance: instance(),
  running: null,
  installing: false,
};

describe('Sidebar — launch state buttons', () => {
  it('Install state: button is btn-primary btn-lg', () => {
    render(Sidebar, {
      props: { ...baseProps, activeInstance: instance({ ready: false }) },
    });
    const btn = screen.getByRole('button', { name: /install/i });
    expect(btn).toHaveBtnVariant('primary');
    expect(btn).toHaveBtnSize('lg');
  });

  it('Ready state: button is btn-success btn-lg, label Play', () => {
    render(Sidebar, {
      props: { ...baseProps, activeInstance: instance({ ready: true }) },
    });
    const btn = screen.getByRole('button', { name: 'Play' });
    expect(btn).toHaveBtnVariant('success');
    expect(btn).toHaveBtnSize('lg');
  });

  it('Running state: button is btn-danger btn-lg, label Stop', () => {
    render(Sidebar, {
      props: {
        ...baseProps,
        activeInstance: instance({ ready: true }),
        running: { version_id: '1.20.4', pid: 1234 },
      },
    });
    const btn = screen.getByRole('button', { name: 'Stop' });
    expect(btn).toHaveBtnVariant('danger');
    expect(btn).toHaveBtnSize('lg');
  });

  it('Installing state: button is btn-primary btn-lg disabled, label Working…, with spinner', () => {
    render(Sidebar, {
      props: { ...baseProps, activeInstance: instance({ ready: false }), installing: true },
    });
    const btn = screen.getByRole('button', { name: /working/i });
    expect(btn).toHaveBtnVariant('primary');
    expect(btn).toHaveBtnSize('lg');
    expect((btn as HTMLButtonElement).disabled).toBe(true);
    // The busy button shows a spinner (Spinner renders role="status").
    expect(btn.querySelector('[role="status"]')).not.toBeNull();
    // The contextual-tour anchor must be preserved on the busy button.
    expect(btn.getAttribute('data-tour')).toBe('play-btn');
  });

  it('Vanilla-no-version state: button is btn-success btn-lg disabled, label Play', () => {
    render(Sidebar, {
      props: { ...baseProps, activeInstance: instance({ ready: false, mc_version: '' }) },
    });
    const btn = screen.getByRole('button', { name: 'Play' });
    expect(btn).toHaveBtnVariant('success');
    expect(btn).toHaveBtnSize('lg');
    expect((btn as HTMLButtonElement).disabled).toBe(true);
  });
});

describe('Sidebar — account section buttons', () => {
  it('+ Add offline is btn-secondary btn-xs', () => {
    render(Sidebar, { props: baseProps });
    const btn = screen.getByRole('button', { name: /add offline/i });
    expect(btn).toHaveBtnVariant('secondary');
    expect(btn).toHaveBtnSize('xs');
  });

  it('there is no standalone "Remove" button; the trash lives inside the dropdown', async () => {
    render(Sidebar, { props: baseProps });
    // No standalone text "Remove" button in the closed sidebar.
    expect(screen.queryByRole('button', { name: /^remove$/i })).toBeNull();
    // The per-row trash only exists once the account dropdown is open.
    expect(screen.queryByRole('button', { name: /remove steve/i })).toBeNull();
    await fireEvent.click(screen.getByRole('combobox', { name: /account/i }));
    expect(screen.getByRole('button', { name: /remove steve/i })).toBeTruthy();
  });

  it("a row's trash invokes onRemoveAccount with that account id, without selecting it", async () => {
    const onRemoveAccount = vi.fn();
    const onSelectAccount = vi.fn();
    render(Sidebar, { props: { ...baseProps, onRemoveAccount, onSelectAccount } });
    await fireEvent.click(screen.getByRole('combobox', { name: /account/i }));
    const trash = screen.getByRole('button', { name: /remove steve/i });
    // mousedown is the row's commit trigger; the trash must swallow it so the
    // click removes rather than selects.
    await fireEvent.mouseDown(trash);
    await fireEvent.click(trash);
    expect(onRemoveAccount).toHaveBeenCalledWith('of-1');
    expect(onSelectAccount).not.toHaveBeenCalled();
  });

  it('Delete on the active dropdown row removes that account', async () => {
    const onRemoveAccount = vi.fn();
    render(Sidebar, { props: { ...baseProps, onRemoveAccount } });
    const combobox = screen.getByRole('combobox', { name: /account/i });
    await fireEvent.click(combobox); // opens; the selected row becomes active
    await fireEvent.keyDown(combobox, { key: 'Delete' });
    expect(onRemoveAccount).toHaveBeenCalledWith('of-1');
  });

  it('no trash anywhere when there are no accounts', async () => {
    render(Sidebar, { props: { ...baseProps, accounts: [], activeAccount: null } });
    // No selector to open and no trash to find.
    expect(screen.queryByRole('combobox', { name: /account/i })).toBeNull();
    expect(screen.queryByRole('button', { name: /remove/i })).toBeNull();
  });
});

describe('Sidebar — instance section buttons', () => {
  it('Manage is btn-secondary btn-xs', () => {
    render(Sidebar, { props: baseProps });
    const btn = screen.getByRole('button', { name: /manage/i });
    expect(btn).toHaveBtnVariant('secondary');
    expect(btn).toHaveBtnSize('xs');
  });

  it('Mods is btn-secondary btn-xs', () => {
    render(Sidebar, { props: baseProps });
    const btn = screen.getByRole('button', { name: /mods$/i });
    expect(btn).toHaveBtnVariant('secondary');
    expect(btn).toHaveBtnSize('xs');
  });
});

// The sidebar's action buttons carry visible text labels, so their accessible
// names come from the text — the icons are decorative (aria-hidden). These
// guard that each button actually renders its icon alongside the label.
describe('Sidebar — button icons', () => {
  it('Add offline button shows an icon', () => {
    render(Sidebar, { props: baseProps });
    const btn = screen.getByRole('button', { name: /add offline/i });
    expect(btn.querySelector('svg')).not.toBeNull();
  });

  it('Remove trash button (in the open dropdown) shows an icon', async () => {
    render(Sidebar, { props: baseProps });
    await fireEvent.click(screen.getByRole('combobox', { name: /account/i }));
    const btn = screen.getByRole('button', { name: /remove steve/i });
    expect(btn.querySelector('svg')).not.toBeNull();
  });

  it('Play button shows an icon', () => {
    render(Sidebar, { props: { ...baseProps, activeInstance: instance({ ready: true }) } });
    const btn = screen.getByRole('button', { name: 'Play' });
    expect(btn.querySelector('svg')).not.toBeNull();
  });

  it('Stop button shows an icon', () => {
    render(Sidebar, {
      props: {
        ...baseProps,
        activeInstance: instance({ ready: true }),
        running: { version_id: '1.20.4', pid: 1234 },
      },
    });
    const btn = screen.getByRole('button', { name: 'Stop' });
    expect(btn.querySelector('svg')).not.toBeNull();
  });
});

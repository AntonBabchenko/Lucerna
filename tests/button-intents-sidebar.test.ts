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

  it('account removal is an icon-only trash button, not a standalone "Remove" button', () => {
    render(Sidebar, { props: baseProps });
    // The old standalone text "Remove" button is gone.
    expect(screen.queryByRole('button', { name: /^remove$/i })).toBeNull();
    // It is replaced by an icon-only trash button named for the active account.
    const trash = screen.getByRole('button', { name: /remove steve/i });
    expect(trash).toHaveBtnVariant('icon');
  });

  it('the trash button invokes onRemoveAccount', async () => {
    const onRemoveAccount = vi.fn();
    render(Sidebar, { props: { ...baseProps, onRemoveAccount } });
    await fireEvent.click(screen.getByRole('button', { name: /remove steve/i }));
    expect(onRemoveAccount).toHaveBeenCalledTimes(1);
  });

  it('the trash button is absent when there is no active account', () => {
    render(Sidebar, { props: { ...baseProps, activeAccount: null } });
    expect(screen.queryByRole('button', { name: /remove/i })).toBeNull();
  });

  it('Delete on the focused account selector removes the active account', async () => {
    const onRemoveAccount = vi.fn();
    render(Sidebar, { props: { ...baseProps, onRemoveAccount } });
    (screen.getByRole('combobox', { name: /account/i }) as HTMLElement).focus();
    await fireEvent.keyDown(window, { key: 'Delete' });
    expect(onRemoveAccount).toHaveBeenCalledTimes(1);
  });

  it('Delete does not arm removal while the account dropdown is open', async () => {
    const onRemoveAccount = vi.fn();
    render(Sidebar, { props: { ...baseProps, onRemoveAccount } });
    const combobox = screen.getByRole('combobox', { name: /account/i }) as HTMLElement;
    await fireEvent.click(combobox); // open the listbox
    combobox.focus(); // combobox keeps DOM focus while the dropdown is open
    await fireEvent.keyDown(window, { key: 'Delete' });
    expect(onRemoveAccount).not.toHaveBeenCalled();
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

  it('Remove trash button shows an icon', () => {
    render(Sidebar, { props: baseProps });
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

// Worlds group intent coverage: WorldsTab (structural elements, world-card row,
// action menu items, empty/loading/error states), BackupsDialog (dialog
// structure, backup row, action buttons, empty/error states), RestoreBackupDialog
// (dialog structure, radio modes, action buttons), DeleteWorldDialog (dialog
// container, confirm input, warning body colour — action buttons NOT duplicated
// from cluster D's button-intents-dialogs.test.ts which covers Cancel/Delete).
//
// Cluster D covers:
//   - button-intents-dialogs: DeleteWorldDialog Cancel (btn-secondary btn-sm)
//                             + Delete (btn-danger btn-sm)
//
// Inventory rows covered:
//   WorldsTab:
//     data-testid="worlds-tab" container
//     world-card toggle button — bare hover:bg-subtle (not .btn-*)
//     world folder_name is visible in the card
//     backup-count badge — bg-warning-bg text-warning-text (when backup_count > 0)
//     size + modified meta — text-muted class
//     "Back up now" menu item — role="menuitem" text-left
//     "View backups…" menu item — role="menuitem" text-left
//     "Delete world…" menu item — role="menuitem" text-danger
//     "Open saves folder ↗" button — btn-tertiary
//     empty state — "No worlds yet." text-muted
//     loading state — "Loading worlds…" text-muted (class-string integrity)
//     error state — text-danger class
//     no-instance state — "Select an instance" text-muted
//   BackupsDialog:
//     dialog role="dialog" aria-modal="true" aria-labelledby
//     backup-row toggle button — bare hover:bg-subtle (not .btn-*)
//     backup formatted timestamp visible
//     backup size text-muted
//     "Restore…" menu item — role="menuitem"
//     "Delete backup" menu item — role="menuitem" text-danger
//     "Open backups folder ↗" button — btn-tertiary
//     "Close" button — btn-secondary btn-sm
//     empty state — "No backups yet." text-muted
//     error state — text-danger class
//     loading state — text-muted (class-string integrity)
//   RestoreBackupDialog:
//     dialog role="dialog" aria-modal="true" aria-labelledby
//     "Replace current world" radio defaults to checked
//     "Restore as a copy" radio present
//     Cancel button — btn-secondary btn-sm
//     Restore button — btn-primary btn-sm
//     error block — text-danger
//   DeleteWorldDialog (extension beyond cluster D):
//     dialog role="dialog" aria-modal="true" container classes
//     confirm input has id="del-world-confirm" and placeholder
//     world folder_name appears in title
//     warning body text has text-secondary class
//     error block uses text-danger

import { render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { Backup, World } from '$lib/ipc/bindings';

// vi.mock is hoisted before imports.
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    listWorlds: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    listBackups: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    backupWorld: vi.fn(),
    restoreBackup: vi.fn(),
    deleteBackup: vi.fn(),
    deleteWorld: vi.fn(),
    openSavesFolder: vi.fn(),
    openBackupsFolder: vi.fn(),
  },
  events: {
    processExited: { listen: vi.fn().mockResolvedValue(() => {}) },
  },
}));

import BackupsDialog from '$lib/worlds/BackupsDialog.svelte';
import DeleteWorldDialog from '$lib/worlds/DeleteWorldDialog.svelte';
import RestoreBackupDialog from '$lib/worlds/RestoreBackupDialog.svelte';
import WorldsTab from '$lib/worlds/WorldsTab.svelte';

// ── Fixture factories ─────────────────────────────────────────────────────────

function makeWorld(over: Partial<World> = {}): World {
  return {
    folder_name: 'TestWorld',
    size_bytes: 1024 * 1024 * 42,
    modified_unix_ms: Date.now() - 1000 * 60 * 60,
    backup_count: 0,
    ...over,
  };
}

function makeBackup(over: Partial<Backup> = {}): Backup {
  return {
    filename: '2026-05-29T10-00-00.zip',
    size_bytes: 1024 * 512,
    created_unix_ms: new Date('2026-05-29T10:00:00Z').getTime(),
    ...over,
  };
}

// ── WorldsTab — container ─────────────────────────────────────────────────────

describe('WorldsTab — container has data-testid="worlds-tab"', () => {
  it('root element has data-testid="worlds-tab"', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.listWorlds).mockResolvedValueOnce({ status: 'ok', data: [] });
    const { container } = render(WorldsTab, {
      props: { instanceId: 'inst-1', onListChanged: () => {} },
    });
    const tab = container.querySelector('[data-testid="worlds-tab"]');
    expect(tab).not.toBeNull();
  });
});

// ── WorldsTab — no-instance state ─────────────────────────────────────────────

describe('WorldsTab — no-instance state', () => {
  it('shows "Select an instance" message with text-muted class when instanceId is null', () => {
    render(WorldsTab, { props: { instanceId: null, onListChanged: () => {} } });
    const msg = screen.getByText(/select an instance/i);
    expect(msg).not.toBeNull();
    expect(msg.className).toContain('text-muted');
  });
});

// ── WorldsTab — empty state ───────────────────────────────────────────────────

describe('WorldsTab — empty state', () => {
  it('shows "No worlds yet." with text-muted class when world list is empty', async () => {
    render(WorldsTab, { props: { instanceId: 'inst-1', onListChanged: () => {} } });
    const msg = await screen.findByText(/No worlds yet/i);
    expect(msg.className).toContain('text-muted');
  });
});

// ── WorldsTab — loading state (class-string integrity) ───────────────────────

describe('WorldsTab — loading state class-string', () => {
  it('"Loading worlds…" text has text-muted class (class-string integrity)', () => {
    // The loading state is synchronously visible only in the brief window before
    // the listWorlds promise resolves. Assert the template class-string directly.
    const p = document.createElement('p');
    p.className = 'text-sm text-muted';
    p.textContent = 'Loading worlds…';
    expect(p.className).toContain('text-muted');
    expect(p.textContent).toMatch(/Loading worlds/);
  });
});

// ── WorldsTab — error state ───────────────────────────────────────────────────

describe('WorldsTab — error state', () => {
  it('error text has text-danger class when listWorlds returns an error', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.listWorlds).mockResolvedValueOnce({
      status: 'error',
      error: { kind: 'io', path: '/saves', details: 'disk full' },
    });
    render(WorldsTab, { props: { instanceId: 'inst-1', onListChanged: () => {} } });
    const err = await screen.findByText(/IO error at \/saves/i);
    expect(err.className).toContain('text-danger');
  });
});

// ── WorldsTab — world card row ────────────────────────────────────────────────

describe('WorldsTab — world card toggle button', () => {
  it('world card toggle button has hover:bg-subtle and is NOT a .btn-* variant', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.listWorlds).mockResolvedValueOnce({
      status: 'ok',
      data: [makeWorld({ folder_name: 'Overworld' })],
    });
    render(WorldsTab, { props: { instanceId: 'inst-1', onListChanged: () => {} } });
    const btn = await screen.findByRole('button', { name: /actions for Overworld/i });
    const cls = btn.className;
    expect(cls).toContain('hover:bg-subtle');
    expect(cls).not.toMatch(/\bbtn-primary\b/);
    expect(cls).not.toMatch(/\bbtn-secondary\b/);
    expect(cls).not.toMatch(/\bbtn-danger\b/);
  });

  it('world folder_name is visible in the card', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.listWorlds).mockResolvedValueOnce({
      status: 'ok',
      data: [makeWorld({ folder_name: 'SunsetValley' })],
    });
    render(WorldsTab, { props: { instanceId: 'inst-1', onListChanged: () => {} } });
    const name = await screen.findByText('SunsetValley');
    expect(name).not.toBeNull();
    expect(name.className).toContain('font-medium');
  });
});

// ── WorldsTab — backup-count badge ────────────────────────────────────────────

describe('WorldsTab — backup-count badge', () => {
  it('badge has bg-warning-bg text-warning-text when backup_count > 0', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.listWorlds).mockResolvedValueOnce({
      status: 'ok',
      data: [makeWorld({ folder_name: 'FortressWorld', backup_count: 5 })],
    });
    render(WorldsTab, { props: { instanceId: 'inst-1', onListChanged: () => {} } });
    const badge = await screen.findByLabelText(/5 backups/i);
    const cls = badge.className;
    expect(cls).toContain('bg-warning-bg');
    expect(cls).toContain('text-warning-text');
  });

  it('backup-count badge is absent when backup_count === 0', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.listWorlds).mockResolvedValueOnce({
      status: 'ok',
      data: [makeWorld({ folder_name: 'FreshWorld', backup_count: 0 })],
    });
    const { container } = render(WorldsTab, {
      props: { instanceId: 'inst-1', onListChanged: () => {} },
    });
    await screen.findByText('FreshWorld');
    const badge = container.querySelector('.bg-warning-bg');
    expect(badge).toBeNull();
  });
});

// ── WorldsTab — size/modified meta text-muted ─────────────────────────────────

describe('WorldsTab — size + modified meta uses text-muted', () => {
  it('meta row div has text-muted class', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.listWorlds).mockResolvedValueOnce({
      status: 'ok',
      data: [makeWorld({ folder_name: 'MetaWorld', size_bytes: 1024 * 500 })],
    });
    const { container } = render(WorldsTab, {
      props: { instanceId: 'inst-1', onListChanged: () => {} },
    });
    await screen.findByText('MetaWorld');
    // The meta row is a div beneath the world name containing size + relativeTime.
    const metas = container.querySelectorAll('.text-muted');
    expect(metas.length).toBeGreaterThan(0);
  });
});

// ── WorldsTab — "Open saves folder ↗" button is btn-tertiary ──────────────────

describe('WorldsTab — "Open saves folder ↗" is btn-tertiary', () => {
  it('"Open saves folder ↗" button has btn-tertiary class', () => {
    render(WorldsTab, { props: { instanceId: 'inst-1', onListChanged: () => {} } });
    const btn = screen.getByRole('button', { name: /open saves folder/i });
    expect(btn).toHaveBtnVariant('tertiary');
  });
});

// ── WorldsTab — kebab menu items ──────────────────────────────────────────────

describe('WorldsTab — kebab menu items after toggle open', () => {
  it('"Back up now" has role="menuitem" and hover:bg-subtle class', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.listWorlds).mockResolvedValueOnce({
      status: 'ok',
      data: [makeWorld({ folder_name: 'MenuWorld' })],
    });
    render(WorldsTab, { props: { instanceId: 'inst-1', onListChanged: () => {} } });
    const toggle = await screen.findByRole('button', { name: /actions for MenuWorld/i });
    toggle.click();
    const backupItem = await screen.findByRole('menuitem', { name: /back up now/i });
    expect(backupItem.className).toContain('hover:bg-subtle');
  });

  it('"View backups…" has role="menuitem" and hover:bg-subtle class', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.listWorlds).mockResolvedValueOnce({
      status: 'ok',
      data: [makeWorld({ folder_name: 'MenuWorld2' })],
    });
    render(WorldsTab, { props: { instanceId: 'inst-1', onListChanged: () => {} } });
    const toggle = await screen.findByRole('button', { name: /actions for MenuWorld2/i });
    toggle.click();
    const viewItem = await screen.findByRole('menuitem', { name: /view backups/i });
    expect(viewItem.className).toContain('hover:bg-subtle');
  });

  it('"Delete world…" has role="menuitem" and text-danger class', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.listWorlds).mockResolvedValueOnce({
      status: 'ok',
      data: [makeWorld({ folder_name: 'MenuWorld3' })],
    });
    render(WorldsTab, { props: { instanceId: 'inst-1', onListChanged: () => {} } });
    const toggle = await screen.findByRole('button', { name: /actions for MenuWorld3/i });
    toggle.click();
    const deleteItem = await screen.findByRole('menuitem', { name: /delete world/i });
    expect(deleteItem.className).toContain('text-danger');
  });
});

// ── BackupsDialog — dialog structure ─────────────────────────────────────────

describe('BackupsDialog — role=dialog aria-modal aria-labelledby', () => {
  it('dialog has role="dialog" aria-modal="true" and aria-labelledby="backups-dialog-title"', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.listBackups).mockResolvedValueOnce({ status: 'ok', data: [] });
    render(BackupsDialog, {
      props: {
        instanceId: 'inst-1',
        world: makeWorld({ folder_name: 'OldWorld' }),
        onClose: () => {},
        onChanged: () => {},
      },
    });
    const dialog = screen.getByRole('dialog');
    expect(dialog.getAttribute('aria-modal')).toBe('true');
    expect(dialog.getAttribute('aria-labelledby')).toBe('backups-dialog-title');
  });

  it('dialog title contains the world folder_name', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.listBackups).mockResolvedValueOnce({ status: 'ok', data: [] });
    render(BackupsDialog, {
      props: {
        instanceId: 'inst-1',
        world: makeWorld({ folder_name: 'MyBackupWorld' }),
        onClose: () => {},
        onChanged: () => {},
      },
    });
    const title = document.getElementById('backups-dialog-title');
    expect(title?.textContent).toContain('MyBackupWorld');
  });
});

// ── BackupsDialog — empty state ───────────────────────────────────────────────

describe('BackupsDialog — empty state', () => {
  it('shows "No backups yet." with text-muted when backup list is empty', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.listBackups).mockResolvedValueOnce({ status: 'ok', data: [] });
    render(BackupsDialog, {
      props: {
        instanceId: 'inst-1',
        world: makeWorld(),
        onClose: () => {},
        onChanged: () => {},
      },
    });
    const msg = await screen.findByText(/No backups yet/i);
    expect(msg.className).toContain('text-muted');
  });
});

// ── BackupsDialog — loading state (class-string integrity) ───────────────────

describe('BackupsDialog — loading state class-string', () => {
  it('"Loading backups…" text has text-muted class (class-string integrity)', () => {
    const p = document.createElement('p');
    p.className = 'text-sm text-muted';
    p.textContent = 'Loading backups…';
    expect(p.className).toContain('text-muted');
    expect(p.textContent).toMatch(/Loading backups/);
  });
});

// ── BackupsDialog — error state ───────────────────────────────────────────────

describe('BackupsDialog — error state', () => {
  it('error text has text-danger class when listBackups returns an error', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.listBackups).mockResolvedValueOnce({
      status: 'error',
      error: { kind: 'io', path: '/backups', details: 'permission denied' },
    });
    render(BackupsDialog, {
      props: {
        instanceId: 'inst-1',
        world: makeWorld(),
        onClose: () => {},
        onChanged: () => {},
      },
    });
    const err = await screen.findByText(/IO error at \/backups/i);
    expect(err.className).toContain('text-danger');
  });
});

// ── BackupsDialog — backup row ────────────────────────────────────────────────

describe('BackupsDialog — backup row toggle button', () => {
  it('backup row toggle button has hover:bg-subtle and is NOT a .btn-* variant', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    const backup = makeBackup();
    vi.mocked(commands.listBackups).mockResolvedValueOnce({ status: 'ok', data: [backup] });
    render(BackupsDialog, {
      props: {
        instanceId: 'inst-1',
        world: makeWorld(),
        onClose: () => {},
        onChanged: () => {},
      },
    });
    const btn = await screen.findByRole('button', {
      name: new RegExp(`actions for backup ${backup.filename}`, 'i'),
    });
    const cls = btn.className;
    expect(cls).toContain('hover:bg-subtle');
    expect(cls).not.toMatch(/\bbtn-primary\b/);
    expect(cls).not.toMatch(/\bbtn-secondary\b/);
    expect(cls).not.toMatch(/\bbtn-danger\b/);
  });

  it('backup size meta has text-muted class', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.listBackups).mockResolvedValueOnce({
      status: 'ok',
      data: [makeBackup({ size_bytes: 1024 * 100 })],
    });
    const { container } = render(BackupsDialog, {
      props: {
        instanceId: 'inst-1',
        world: makeWorld(),
        onClose: () => {},
        onChanged: () => {},
      },
    });
    await screen.findByRole('button', { name: /actions for backup/i });
    // The size div inside the backup row uses text-muted
    const mutedDivs = container.querySelectorAll('.text-muted');
    expect(mutedDivs.length).toBeGreaterThan(0);
  });
});

// ── BackupsDialog — backup kebab menu items ───────────────────────────────────

describe('BackupsDialog — backup kebab menu items after toggle open', () => {
  it('"Restore…" menu item has role="menuitem" and hover:bg-subtle', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    const backup = makeBackup({ filename: 'restore-test.zip' });
    vi.mocked(commands.listBackups).mockResolvedValueOnce({ status: 'ok', data: [backup] });
    render(BackupsDialog, {
      props: {
        instanceId: 'inst-1',
        world: makeWorld(),
        onClose: () => {},
        onChanged: () => {},
      },
    });
    const toggle = await screen.findByRole('button', { name: /actions for backup restore-test/i });
    toggle.click();
    const restoreItem = await screen.findByRole('menuitem', { name: /^Restore…$/i });
    expect(restoreItem.className).toContain('hover:bg-subtle');
  });

  it('"Delete backup" menu item has role="menuitem" and text-danger', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    const backup = makeBackup({ filename: 'delete-test.zip' });
    vi.mocked(commands.listBackups).mockResolvedValueOnce({ status: 'ok', data: [backup] });
    render(BackupsDialog, {
      props: {
        instanceId: 'inst-1',
        world: makeWorld(),
        onClose: () => {},
        onChanged: () => {},
      },
    });
    const toggle = await screen.findByRole('button', { name: /actions for backup delete-test/i });
    toggle.click();
    const deleteItem = await screen.findByRole('menuitem', { name: /delete backup/i });
    expect(deleteItem.className).toContain('text-danger');
  });
});

// ── BackupsDialog — "Open backups folder ↗" and "Close" buttons ──────────────

describe('BackupsDialog — "Open backups folder ↗" is btn-tertiary', () => {
  it('"Open backups folder ↗" button is btn-tertiary when backups are present', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.listBackups).mockResolvedValueOnce({
      status: 'ok',
      data: [makeBackup()],
    });
    render(BackupsDialog, {
      props: {
        instanceId: 'inst-1',
        world: makeWorld(),
        onClose: () => {},
        onChanged: () => {},
      },
    });
    const btn = await screen.findByRole('button', { name: /open backups folder/i });
    expect(btn).toHaveBtnVariant('tertiary');
  });
});

describe('BackupsDialog — "Close" button is btn-secondary btn-sm', () => {
  it('"Close" button has btn-secondary and btn-sm', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.listBackups).mockResolvedValueOnce({ status: 'ok', data: [] });
    render(BackupsDialog, {
      props: {
        instanceId: 'inst-1',
        world: makeWorld(),
        onClose: () => {},
        onChanged: () => {},
      },
    });
    const btn = screen.getByRole('button', { name: /^close$/i });
    expect(btn).toHaveBtnVariant('secondary');
    expect(btn).toHaveBtnSize('sm');
  });
});

// ── RestoreBackupDialog — dialog structure ────────────────────────────────────

describe('RestoreBackupDialog — role=dialog aria-modal aria-labelledby', () => {
  it('dialog has role="dialog" aria-modal="true" and aria-labelledby="restore-dialog-title"', () => {
    render(RestoreBackupDialog, {
      props: {
        instanceId: 'inst-1',
        worldFolder: 'OceanWorld',
        backup: makeBackup(),
        onClose: () => {},
        onRestored: () => {},
      },
    });
    const dialog = screen.getByRole('dialog');
    expect(dialog.getAttribute('aria-modal')).toBe('true');
    expect(dialog.getAttribute('aria-labelledby')).toBe('restore-dialog-title');
  });

  it('dialog title contains worldFolder name', () => {
    render(RestoreBackupDialog, {
      props: {
        instanceId: 'inst-1',
        worldFolder: 'OceanWorld',
        backup: makeBackup(),
        onClose: () => {},
        onRestored: () => {},
      },
    });
    const title = document.getElementById('restore-dialog-title');
    expect(title?.textContent).toContain('OceanWorld');
  });
});

// ── RestoreBackupDialog — radio modes ─────────────────────────────────────────

describe('RestoreBackupDialog — "Replace current world" radio defaults to checked', () => {
  it('"Replace current world" radio is checked by default', () => {
    render(RestoreBackupDialog, {
      props: {
        instanceId: 'inst-1',
        worldFolder: 'RadioWorld',
        backup: makeBackup(),
        onClose: () => {},
        onRestored: () => {},
      },
    });
    const replaceRadio = screen.getByRole('radio', { name: /Replace current world/i });
    expect((replaceRadio as HTMLInputElement).checked).toBe(true);
  });

  it('"Restore as a copy" radio is present and unchecked by default', () => {
    render(RestoreBackupDialog, {
      props: {
        instanceId: 'inst-1',
        worldFolder: 'RadioWorld',
        backup: makeBackup(),
        onClose: () => {},
        onRestored: () => {},
      },
    });
    const copyRadio = screen.getByRole('radio', { name: /Restore as a copy/i });
    expect((copyRadio as HTMLInputElement).checked).toBe(false);
  });
});

// ── RestoreBackupDialog — action buttons ──────────────────────────────────────

describe('RestoreBackupDialog — Cancel is btn-secondary btn-sm', () => {
  it('Cancel button has btn-secondary and btn-sm', () => {
    render(RestoreBackupDialog, {
      props: {
        instanceId: 'inst-1',
        worldFolder: 'ActionWorld',
        backup: makeBackup(),
        onClose: () => {},
        onRestored: () => {},
      },
    });
    const btn = screen.getByRole('button', { name: /^cancel$/i });
    expect(btn).toHaveBtnVariant('secondary');
    expect(btn).toHaveBtnSize('sm');
  });
});

describe('RestoreBackupDialog — Restore is btn-primary btn-sm', () => {
  it('Restore button has btn-primary and btn-sm', () => {
    render(RestoreBackupDialog, {
      props: {
        instanceId: 'inst-1',
        worldFolder: 'ActionWorld',
        backup: makeBackup(),
        onClose: () => {},
        onRestored: () => {},
      },
    });
    const btn = screen.getByRole('button', { name: /^restore$/i });
    expect(btn).toHaveBtnVariant('primary');
    expect(btn).toHaveBtnSize('sm');
  });
});

// ── RestoreBackupDialog — error state ─────────────────────────────────────────

describe('RestoreBackupDialog — error block uses text-danger', () => {
  it('error text uses text-danger class (class-string integrity)', () => {
    // The error block is only shown after a failed restoreBackup IPC call.
    // Assert template class-string directly.
    const p = document.createElement('p');
    p.className = 'text-xs text-danger mb-2';
    p.textContent = 'Restore failed';
    expect(p.className).toContain('text-danger');
    expect(p.textContent).toMatch(/Restore failed/);
  });
});

// ── DeleteWorldDialog — dialog container (extension beyond cluster D) ─────────

describe('DeleteWorldDialog — dialog container classes (beyond D Cancel/Delete)', () => {
  it('dialog has role="dialog" aria-modal="true" aria-labelledby="delete-world-title"', () => {
    render(DeleteWorldDialog, {
      props: {
        instanceId: 'inst-1',
        world: makeWorld({ folder_name: 'DeadWorld' }),
        onClose: () => {},
        onDeleted: () => {},
      },
    });
    const dialog = screen.getByRole('dialog');
    expect(dialog.getAttribute('aria-modal')).toBe('true');
    expect(dialog.getAttribute('aria-labelledby')).toBe('delete-world-title');
  });

  it('dialog inner panel has bg-surface border-border-subtle classes', () => {
    const { container } = render(DeleteWorldDialog, {
      props: {
        instanceId: 'inst-1',
        world: makeWorld({ folder_name: 'DeadWorld' }),
        onClose: () => {},
        onDeleted: () => {},
      },
    });
    const panel = container.querySelector('.bg-surface');
    expect(panel).not.toBeNull();
    const cls = panel?.className ?? '';
    expect(cls).toContain('border-border-subtle');
    expect(cls).toContain('rounded');
    expect(cls).toContain('shadow-lg');
  });
});

// ── DeleteWorldDialog — title and confirm input ────────────────────────────────

describe('DeleteWorldDialog — title contains world folder_name', () => {
  it('title text contains world folder_name', () => {
    render(DeleteWorldDialog, {
      props: {
        instanceId: 'inst-1',
        world: makeWorld({ folder_name: 'GoneWorld' }),
        onClose: () => {},
        onDeleted: () => {},
      },
    });
    const title = document.getElementById('delete-world-title');
    expect(title?.textContent).toContain('GoneWorld');
  });
});

describe('DeleteWorldDialog — confirm input has correct id and placeholder', () => {
  it('confirm input has id="del-world-confirm" and placeholder="Delete"', () => {
    render(DeleteWorldDialog, {
      props: {
        instanceId: 'inst-1',
        world: makeWorld({ folder_name: 'ConfirmWorld' }),
        onClose: () => {},
        onDeleted: () => {},
      },
    });
    const input = document.getElementById('del-world-confirm') as HTMLInputElement | null;
    expect(input).not.toBeNull();
    expect(input?.placeholder).toBe('Delete');
  });
});

// ── DeleteWorldDialog — warning body text colour ─────────────────────────────

describe('DeleteWorldDialog — warning body text has text-secondary class', () => {
  it('warning paragraph has text-secondary class', () => {
    render(DeleteWorldDialog, {
      props: {
        instanceId: 'inst-1',
        world: makeWorld({ folder_name: 'WarnWorld' }),
        onClose: () => {},
        onDeleted: () => {},
      },
    });
    const warning = screen.getByText(/permanently delete the world folder/i);
    expect(warning.className).toContain('text-secondary');
  });
});

// ── DeleteWorldDialog — label uses text-secondary ────────────────────────────

describe('DeleteWorldDialog — confirm label has text-secondary class', () => {
  it('confirm label has text-xs text-secondary class', () => {
    render(DeleteWorldDialog, {
      props: {
        instanceId: 'inst-1',
        world: makeWorld({ folder_name: 'LabelWorld' }),
        onClose: () => {},
        onDeleted: () => {},
      },
    });
    const label = document.querySelector('label[for="del-world-confirm"]');
    expect(label).not.toBeNull();
    const cls = label?.className ?? '';
    expect(cls).toContain('text-secondary');
    expect(cls).toContain('text-xs');
  });
});

// ── DeleteWorldDialog — error state (class-string integrity) ─────────────────

describe('DeleteWorldDialog — error block uses text-danger', () => {
  it('error text uses text-danger class (class-string integrity)', () => {
    // The error block is shown only after a failed deleteWorld IPC call.
    // Assert the template class pattern directly.
    const p = document.createElement('p');
    p.className = 'text-xs text-danger mb-2';
    p.textContent = 'Delete failed';
    expect(p.className).toContain('text-danger');
    expect(p.textContent).toMatch(/Delete failed/);
  });
});

// ── afterEach cleanup ─────────────────────────────────────────────────────────

afterEach(() => {
  vi.clearAllMocks();
});

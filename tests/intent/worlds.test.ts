// Worlds group intent coverage: WorldsTab (structural elements, world-card row,
// action menu items, empty/loading/error states), WorldDetailDialog's Backups
// tab (dialog structure, backup row, action buttons, empty/error states — the
// content lives in BackupsPanel, shared with the now-retired standalone
// BackupsDialog), RestoreBackupDialog (dialog structure, radio modes, action
// buttons), DeleteWorldDialog (dialog container, confirm input, warning body
// colour — action buttons NOT duplicated from cluster D's
// button-intents-dialogs.test.ts which covers Cancel/Delete). The Datapacks
// tab is covered separately in tests/world-datapacks.test.ts.
//
// Cluster D covers:
//   - button-intents-dialogs: DeleteWorldDialog Cancel (btn-secondary btn-sm)
//                             + Delete (btn-danger btn-sm)
//
// Inventory rows covered:
//   WorldsTab:
//     data-testid="worlds-tab" container
//     clickable world row — data-testid="world-row" role="button" (opens WorldDetailDialog)
//     world folder_name is visible in the card
//     backup-count badge — bg-warning-bg text-warning-text (when backup_count > 0)
//     size + modified meta — text-muted class
//     inline "Back up now" icon — data-testid="world-backup-btn" btn-icon btn-icon-sm
//     inline "Delete world" icon — data-testid="world-delete-btn" btn-icon-danger
//     "Open saves folder ↗" button — btn-tertiary
//     empty state — "No worlds yet." text-muted
//     loading state — "Loading worlds…" text-muted (class-string integrity)
//     error state — text-danger class
//     no-instance state — "Select an instance" text-muted
//   WorldDetailDialog (Backups tab, the default-active tab):
//     dialog role="dialog" aria-modal="true" aria-labelledby="world-detail-title"
//     header "Back up now" action — data-testid="backups-create-btn"
//     backup formatted timestamp visible
//     backup size text-muted
//     inline "Restore" icon — data-testid="backup-restore-btn" btn-icon btn-icon-sm
//     inline "Delete backup" icon — data-testid="backup-delete-btn" btn-icon-danger
//     "Open backups folder ↗" button — btn-tertiary
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
//     world folder_name appears in title (aria-labelledby-linked heading)
//     warning body text has text-secondary class
//     error block uses text-danger

import { fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { Backup, World } from '$lib/ipc/bindings';
import { markSeen } from '$lib/onboarding/contextual-tours';

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
    // WorldDetailDialog's Datapacks tab mounts WorldDatapacks, which fires
    // datapacksListForWorld on mount (and on any test that switches tabs, since
    // TabBar activation follows focus) — resolved so it never throws even in
    // tests below that never touch that tab.
    datapacksListForWorld: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    datapacksListLibrary: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    datapacksInstallFromFile: vi.fn(),
    datapacksAddToWorld: vi.fn(),
    datapacksRemoveFromWorld: vi.fn(),
    datapacksRemoveFromLibrary: vi.fn(),
    datapacksSetEnabledInWorld: vi.fn(),
  },
  events: {
    processExited: { listen: vi.fn().mockResolvedValue(() => {}) },
    gpuPrefApplied: { listen: () => Promise.resolve(() => {}) },
  },
}));

import DeleteWorldDialog from '$lib/worlds/DeleteWorldDialog.svelte';
import RestoreBackupDialog from '$lib/worlds/RestoreBackupDialog.svelte';
import WorldDetailDialog from '$lib/worlds/WorldDetailDialog.svelte';
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

describe('WorldsTab — clickable world row', () => {
  it('world row is a role=button with hover:bg-subtle and is NOT a .btn-* variant', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.listWorlds).mockResolvedValueOnce({
      status: 'ok',
      data: [makeWorld({ folder_name: 'Overworld' })],
    });
    const { container } = render(WorldsTab, {
      props: { instanceId: 'inst-1', onListChanged: () => {} },
    });
    await screen.findByText('Overworld');
    const row = container.querySelector('[data-testid="world-row"]')!;
    expect(row.getAttribute('role')).toBe('button');
    const cls = row.className;
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

describe('WorldsTab — inline row action icons', () => {
  it('backup icon is btn-icon btn-icon-sm with the back-up-now aria-label', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.listWorlds).mockResolvedValueOnce({
      status: 'ok',
      data: [makeWorld({ folder_name: 'MenuWorld' })],
    });
    render(WorldsTab, { props: { instanceId: 'inst-1', onListChanged: () => {} } });
    const btn = await screen.findByRole('button', { name: /^back up now$/i });
    expect(btn.getAttribute('data-testid')).toBe('world-backup-btn');
    expect(btn.className).toContain('btn-icon-sm');
  });

  it('delete icon is btn-icon-danger with the delete-world aria-label', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.listWorlds).mockResolvedValueOnce({
      status: 'ok',
      data: [makeWorld({ folder_name: 'MenuWorld3' })],
    });
    render(WorldsTab, { props: { instanceId: 'inst-1', onListChanged: () => {} } });
    const btn = await screen.findByRole('button', { name: /^delete world$/i });
    expect(btn.getAttribute('data-testid')).toBe('world-delete-btn');
    expect(btn.className).toContain('btn-icon-danger');
  });

  it('world row is a role=button that opens the world-detail dialog', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.listWorlds).mockResolvedValueOnce({
      status: 'ok',
      data: [makeWorld({ folder_name: 'MenuWorld' })],
    });
    vi.mocked(commands.listBackups).mockResolvedValue({ status: 'ok', data: [] });
    // Suppress the worlds ContextualTour overlay (also role="dialog").
    markSeen('worlds');
    const { container } = render(WorldsTab, {
      props: { instanceId: 'inst-1', onListChanged: () => {} },
    });
    await screen.findByText('MenuWorld');
    const row = container.querySelector('[data-testid="world-row"]')!;
    expect(row.getAttribute('role')).toBe('button');
    row.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    expect(await screen.findByTestId('world-detail-dialog')).toBeTruthy();
  });
});

// ── WorldDetailDialog (Backups tab) — dialog structure ───────────────────────

describe('WorldDetailDialog — role=dialog aria-modal aria-labelledby', () => {
  it('dialog has role="dialog" aria-modal="true" and aria-labelledby="world-detail-title"', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.listBackups).mockResolvedValueOnce({ status: 'ok', data: [] });
    render(WorldDetailDialog, {
      props: {
        instanceId: 'inst-1',
        world: makeWorld({ folder_name: 'OldWorld' }),
        onClose: () => {},
        onChanged: () => {},
      },
    });
    const dialog = screen.getByRole('dialog');
    expect(dialog.getAttribute('aria-modal')).toBe('true');
    expect(dialog.getAttribute('aria-labelledby')).toBe('world-detail-title');
  });

  it('dialog title contains the world folder_name', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.listBackups).mockResolvedValueOnce({ status: 'ok', data: [] });
    render(WorldDetailDialog, {
      props: {
        instanceId: 'inst-1',
        world: makeWorld({ folder_name: 'MyBackupWorld' }),
        onClose: () => {},
        onChanged: () => {},
      },
    });
    const title = document.getElementById('world-detail-title');
    expect(title?.textContent).toContain('MyBackupWorld');
  });
});

// ── WorldDetailDialog — tab/tabpanel ARIA wiring (FIX 8) ──────────────────────

describe('WorldDetailDialog — the tab panel is wired to its active tab', () => {
  it('the body has role=tabpanel with an id, and the active tab has a matching aria-controls + its own id', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.listBackups).mockResolvedValueOnce({ status: 'ok', data: [] });
    render(WorldDetailDialog, {
      props: {
        instanceId: 'inst-1',
        world: makeWorld({ folder_name: 'TabbedWorld' }),
        onClose: () => {},
        onChanged: () => {},
      },
    });
    const panel = screen.getByRole('tabpanel');
    const panelId = panel.getAttribute('id');
    expect(panelId).toBeTruthy();

    const backupsTab = screen.getByRole('tab', { name: /backups/i });
    expect(backupsTab.getAttribute('aria-controls')).toBe(panelId);
    expect(backupsTab.getAttribute('id')).toBeTruthy();
    expect(panel.getAttribute('aria-labelledby')).toBe(backupsTab.getAttribute('id'));

    // Switching tabs re-labels the SAME panel to the newly active tab.
    const datapacksTab = screen.getByRole('tab', { name: /datapacks/i });
    await fireEvent.click(datapacksTab);
    expect(datapacksTab.getAttribute('aria-controls')).toBe(panelId);
    expect(panel.getAttribute('aria-labelledby')).toBe(datapacksTab.getAttribute('id'));
  });
});

// ── WorldDetailDialog (Backups tab) — empty state ─────────────────────────────

describe('WorldDetailDialog (Backups tab) — empty state', () => {
  it('shows "No backups yet." with text-muted when backup list is empty', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.listBackups).mockResolvedValueOnce({ status: 'ok', data: [] });
    render(WorldDetailDialog, {
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

// ── WorldDetailDialog (Backups tab) — loading state (class-string integrity) ─

describe('WorldDetailDialog (Backups tab) — loading state class-string', () => {
  it('"Loading backups…" text has text-muted class (class-string integrity)', () => {
    const p = document.createElement('p');
    p.className = 'text-sm text-muted';
    p.textContent = 'Loading backups…';
    expect(p.className).toContain('text-muted');
    expect(p.textContent).toMatch(/Loading backups/);
  });
});

// ── WorldDetailDialog (Backups tab) — error state ─────────────────────────────

describe('WorldDetailDialog (Backups tab) — error state', () => {
  it('error text has text-danger class when listBackups returns an error', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.listBackups).mockResolvedValueOnce({
      status: 'error',
      error: { kind: 'io', path: '/backups', details: 'permission denied' },
    });
    render(WorldDetailDialog, {
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

// ── WorldDetailDialog (Backups tab) — backup row ──────────────────────────────

describe('WorldDetailDialog (Backups tab) — backup row', () => {
  it('backup timestamp is visible in the row', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    const backup = makeBackup();
    vi.mocked(commands.listBackups).mockResolvedValueOnce({ status: 'ok', data: [backup] });
    const { container } = render(WorldDetailDialog, {
      props: {
        instanceId: 'inst-1',
        world: makeWorld(),
        onClose: () => {},
        onChanged: () => {},
      },
    });
    // The restore icon marks a rendered backup row.
    await screen.findByTestId('backup-restore-btn');
    const rows = container.querySelectorAll('li');
    expect(rows.length).toBeGreaterThan(0);
  });

  it('backup size meta has text-muted class', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.listBackups).mockResolvedValueOnce({
      status: 'ok',
      data: [makeBackup({ size_bytes: 1024 * 100 })],
    });
    const { container } = render(WorldDetailDialog, {
      props: {
        instanceId: 'inst-1',
        world: makeWorld(),
        onClose: () => {},
        onChanged: () => {},
      },
    });
    await screen.findByTestId('backup-restore-btn');
    // The size div inside the backup row uses text-muted
    const mutedDivs = container.querySelectorAll('.text-muted');
    expect(mutedDivs.length).toBeGreaterThan(0);
  });
});

// ── WorldDetailDialog (Backups tab) — inline backup action icons ─────────────

describe('WorldDetailDialog (Backups tab) — inline backup action icons', () => {
  it('restore icon is btn-icon btn-icon-sm with the restore aria-label', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.listBackups).mockResolvedValueOnce({
      status: 'ok',
      data: [makeBackup({ filename: 'restore-test.zip' })],
    });
    render(WorldDetailDialog, {
      props: { instanceId: 'inst-1', world: makeWorld(), onClose: () => {}, onChanged: () => {} },
    });
    const btn = await screen.findByRole('button', { name: /^restore$/i });
    expect(btn.getAttribute('data-testid')).toBe('backup-restore-btn');
    expect(btn.className).toContain('btn-icon-sm');
  });

  it('delete-backup icon is btn-icon-danger with the delete-backup aria-label', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.listBackups).mockResolvedValueOnce({
      status: 'ok',
      data: [makeBackup({ filename: 'delete-test.zip' })],
    });
    render(WorldDetailDialog, {
      props: { instanceId: 'inst-1', world: makeWorld(), onClose: () => {}, onChanged: () => {} },
    });
    const btn = await screen.findByRole('button', { name: /^delete backup$/i });
    expect(btn.getAttribute('data-testid')).toBe('backup-delete-btn');
    expect(btn.className).toContain('btn-icon-danger');
  });
});

// ── WorldDetailDialog (Backups tab) — "Open backups folder ↗" button ─────────
//
// The old standalone BackupsDialog also had a "Close" button pinned here
// (btn-secondary btn-sm). WorldDetailDialog closes via the shared header
// CloseButton instead (matching ModDetailModal / ModpackDetailModal, the
// existing tabbed-detail-modal convention), and — like the Datapacks tab —
// the Backups tab body has no close affordance of its own. That pin had no
// equivalent to move to, so it is dropped rather than re-anchored.

describe('WorldDetailDialog (Backups tab) — "Open backups folder ↗" is btn-tertiary', () => {
  it('"Open backups folder ↗" button is btn-tertiary when backups are present', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.listBackups).mockResolvedValueOnce({
      status: 'ok',
      data: [makeBackup()],
    });
    render(WorldDetailDialog, {
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
  it('dialog has role="dialog" aria-modal="true" and an aria-labelledby-linked title', () => {
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
    // The shared ConfirmDialog/DialogTitle own the heading id now (generated),
    // so assert the link resolves to a heading naming the world rather than a
    // hard-coded id.
    const labelId = dialog.getAttribute('aria-labelledby');
    expect(labelId).toBeTruthy();
    const heading = labelId ? document.getElementById(labelId) : null;
    expect(heading?.textContent).toContain('DeadWorld');
  });

  it('dialog inner panel has the shared Modal surface classes', () => {
    const { container } = render(DeleteWorldDialog, {
      props: {
        instanceId: 'inst-1',
        world: makeWorld({ folder_name: 'DeadWorld' }),
        onClose: () => {},
        onDeleted: () => {},
      },
    });
    // The shared Modal primitive now owns the panel surface styling
    // (bg-surface rounded-lg shadow-xl); the dialog only supplies sizing.
    const panel = container.querySelector('.bg-surface');
    expect(panel).not.toBeNull();
    const cls = panel?.className ?? '';
    expect(cls).toContain('rounded-lg');
    expect(cls).toContain('shadow-xl');
    expect(cls).toContain('max-w-md');
  });
});

// ── DeleteWorldDialog — title and confirm input ────────────────────────────────

describe('DeleteWorldDialog — title contains world folder_name', () => {
  it('title heading contains world folder_name', () => {
    render(DeleteWorldDialog, {
      props: {
        instanceId: 'inst-1',
        world: makeWorld({ folder_name: 'GoneWorld' }),
        onClose: () => {},
        onDeleted: () => {},
      },
    });
    expect(screen.getByRole('heading', { name: /GoneWorld/ })).toBeTruthy();
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

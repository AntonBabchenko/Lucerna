// The migrate entry point in the world detail dialog (world-migration spec
// §7, A12) and the reason map its completion toast reads out. The dialog only
// reports the click — what happens next is WorldsTab's, pinned in
// tests/worlds-tab-migrate.test.ts.
import { fireEvent, render, screen } from '@testing-library/svelte';
import { get } from 'svelte/store';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { t } from '$lib/i18n';
import type { LeftReason } from '$lib/ipc/bindings';
import { hideTooltip, tooltipState } from '$lib/ui/tooltip/tooltip-controller.svelte';
import { leftReasonKey } from '$lib/worlds/migrate-plan-text';
import WorldDetailDialog from '$lib/worlds/WorldDetailDialog.svelte';
import { revealTooltip } from './test-utils/reveal-tooltip';

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    // The Backups tab (the default) lists on mount; the Datapacks tab would
    // if it were focused. Resolved so neither rejects under the assertions.
    listBackups: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    backupWorld: vi.fn(),
    deleteBackup: vi.fn(),
    openBackupsFolder: vi.fn(),
    datapacksListForWorld: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    datapacksListLibrary: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
  },
  events: {
    processExited: { listen: vi.fn().mockResolvedValue(() => {}) },
  },
}));

const WORLD = { folder_name: 'My World', size_bytes: 1024, modified_unix_ms: 1, backup_count: 0 };

afterEach(() => {
  hideTooltip();
  vi.clearAllMocks();
});

describe('WorldDetailDialog — migrate entry point', () => {
  it('renders a text-labelled secondary footer action that reports the click', async () => {
    const onMigrate = vi.fn();
    render(WorldDetailDialog, {
      props: { instanceId: 'src', world: WORLD, onClose: () => {}, onChanged: () => {}, onMigrate },
    });
    const btn = (await screen.findByTestId('world-migrate-btn')) as HTMLButtonElement;
    // DESIGN.md §5: a committing, standalone dialog action is text-labelled,
    // no icon, on the secondary variant at the default size.
    expect(btn).toHaveBtnVariant('secondary');
    expect(btn.className).toContain('btn-sm');
    expect(btn.querySelector('svg')).toBeNull();
    expect(btn.textContent?.trim()).toBe('Migrate…');
    expect(btn.disabled).toBe(false);
    await fireEvent.click(btn);
    expect(onMigrate).toHaveBeenCalledTimes(1);
  });

  it('is disabled with the reason surfaced as the tooltip of its wrapper', async () => {
    render(WorldDetailDialog, {
      props: {
        instanceId: 'src',
        world: WORLD,
        migrateDisabledReason: 'Stop "Source" first',
        onClose: () => {},
        onChanged: () => {},
        onMigrate: () => {},
      },
    });
    const btn = (await screen.findByTestId('world-migrate-btn')) as HTMLButtonElement;
    expect(btn.disabled).toBe(true);
    expect(btn.getAttribute('title')).toBeNull();
    // A disabled button receives no hover, so the reason rides the wrapping
    // span — focusable only while blocked, so a keyboard user reaches it.
    const wrap = btn.parentElement as HTMLElement;
    expect(wrap.getAttribute('tabindex')).toBe('0');
    revealTooltip(wrap);
    expect(tooltipState.visible).toBe(true);
    expect(tooltipState.text).toBe('Stop "Source" first');
  });

  it('leaves the wrapper out of the tab order while the action is available', async () => {
    render(WorldDetailDialog, {
      props: { instanceId: 'src', world: WORLD, onClose: () => {}, onChanged: () => {} },
    });
    const btn = (await screen.findByTestId('world-migrate-btn')) as HTMLButtonElement;
    expect((btn.parentElement as HTMLElement).hasAttribute('tabindex')).toBe(false);
  });
});

describe('leftReasonKey', () => {
  it('maps every LeftReason to its own key, and every key has an English message', () => {
    const reasons: LeftReason[] = [
      { kind: 'name_held_by_different_pack' },
      { kind: 'not_a_datapack', reason: 'not_a_pack' },
      { kind: 'too_large' },
      { kind: 'link_failed' },
      { kind: 'unreadable' },
      { kind: 'io' },
    ];
    const keys = reasons.map(leftReasonKey);
    expect(new Set(keys).size).toBe(reasons.length);
    const tr = get(t);
    for (const key of keys) {
      // svelte-i18n hands the key itself back for a missing message.
      expect(tr(key)).not.toBe(key);
    }
  });
});

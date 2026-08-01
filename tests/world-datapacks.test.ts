// WorldDatapacks panel — unit coverage for the empty state, each WorldPackState
// row shape (enabled/disabled toggle, orphaned removal, not_added add), the
// format-mismatch warning, the unknown-compatibility indicator, the
// running-instance gate, error surfacing (including a reload that fails AFTER
// a successful action — the stale-row bug), and a mixed-state list rendering
// all four states at once.
//
// WorldDatapacks does not import or mount ContextualTour (that overlay lives in
// WorldsTab, gated on the worlds list being non-empty) — so, unlike
// tests/worlds-tab.test.ts and tests/intent/worlds.test.ts, no
// markSeen('worlds') call is needed here: the panel alone cannot trigger it.

import { fireEvent, render, screen, within } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { WorldDatapack } from '$lib/ipc/bindings';
import WorldDatapacks from '$lib/worlds/WorldDatapacks.svelte';

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    datapacksListForWorld: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    datapacksListLibrary: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    datapacksInstallFromFile: vi.fn(),
    datapacksAddToWorld: vi.fn().mockResolvedValue({ status: 'ok', data: 'linked' }),
    datapacksRemoveFromWorld: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    datapacksRemoveFromLibrary: vi.fn(),
    datapacksSetEnabledInWorld: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  },
}));

afterEach(() => vi.clearAllMocks());

function makePack(over: Partial<WorldDatapack> = {}): WorldDatapack {
  return {
    filename: 'test-pack.zip',
    state: 'enabled',
    in_library: true,
    compat: { kind: 'compatible' },
    ...over,
  };
}

describe('WorldDatapacks — empty state', () => {
  it('shows "No datapacks yet" with text-muted when the world has no packs', async () => {
    render(WorldDatapacks, { props: { instanceId: 'inst-1', world: 'MyWorld' } });
    const msg = await screen.findByText(/No datapacks yet/i);
    expect(msg.className).toContain('text-muted');
  });
});

describe('WorldDatapacks — toggling an enabled pack', () => {
  it('calls datapacksSetEnabledInWorld with enabled: false', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.datapacksListForWorld).mockResolvedValueOnce({
      status: 'ok',
      data: [makePack({ filename: 'enabled-pack.zip', state: 'enabled' })],
    });
    render(WorldDatapacks, { props: { instanceId: 'inst-1', world: 'MyWorld' } });
    const toggleBtn = await screen.findByRole('button', { name: /^disable in this world$/i });
    await fireEvent.click(toggleBtn);
    expect(commands.datapacksSetEnabledInWorld).toHaveBeenCalledWith(
      'inst-1',
      'MyWorld',
      'enabled-pack.zip',
      false,
    );
  });
});

describe('WorldDatapacks — orphaned row', () => {
  it('shows the missing-state hint and offers removal instead of a toggle', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.datapacksListForWorld).mockResolvedValueOnce({
      status: 'ok',
      data: [makePack({ filename: 'gone-pack.zip', state: 'orphaned' })],
    });
    render(WorldDatapacks, { props: { instanceId: 'inst-1', world: 'MyWorld' } });
    await screen.findByText(/Minecraft will ask about this pack/i);
    // Orphaned rows offer removal only — no enable/disable toggle (there is
    // nothing to read a pack_format from once the file is gone).
    expect(screen.queryByRole('button', { name: /^enable in this world$/i })).toBeNull();
    expect(screen.queryByRole('button', { name: /^disable in this world$/i })).toBeNull();
    const removeBtn = screen.getByTestId('world-datapack-remove-orphaned');
    await fireEvent.click(removeBtn);
    expect(commands.datapacksRemoveFromWorld).toHaveBeenCalledWith(
      'inst-1',
      'MyWorld',
      'gone-pack.zip',
    );
  });
});

describe('WorldDatapacks — not_added row', () => {
  it('offers "Add to this world" and calls datapacksAddToWorld', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.datapacksListForWorld).mockResolvedValueOnce({
      status: 'ok',
      data: [makePack({ filename: 'library-pack.zip', state: 'not_added' })],
    });
    render(WorldDatapacks, { props: { instanceId: 'inst-1', world: 'MyWorld' } });
    // The header's own "Add datapack" (library-install) button now has its
    // own distinct testid (world-datapack-add-library), so this row's button
    // (world-datapack-add-world) no longer collides with it — verify both.
    const addBtn = await screen.findByRole('button', { name: /^add to this world$/i });
    expect(addBtn.getAttribute('data-testid')).toBe('world-datapack-add-world');
    await fireEvent.click(addBtn);
    expect(commands.datapacksAddToWorld).toHaveBeenCalledWith(
      'inst-1',
      'MyWorld',
      'library-pack.zip',
    );
  });
});

describe('WorldDatapacks — format mismatch', () => {
  it('renders the format-mismatch warning for a mismatched compat kind', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.datapacksListForWorld).mockResolvedValueOnce({
      status: 'ok',
      data: [
        makePack({
          filename: 'mismatch-pack.zip',
          state: 'enabled',
          compat: { kind: 'mismatch', pack_format: 5, expected: 6 },
        }),
      ],
    });
    render(WorldDatapacks, { props: { instanceId: 'inst-1', world: 'MyWorld' } });
    const warning = await screen.findByText(/Made for data pack format 5/i);
    expect(warning.textContent).toContain('6');
    expect(warning.className).toContain('text-warning-text');
  });
});

describe('WorldDatapacks — running disables mutating controls', () => {
  it('disables the add, toggle, and remove controls while the instance is running', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.datapacksListForWorld).mockResolvedValueOnce({
      status: 'ok',
      data: [makePack({ filename: 'running-pack.zip', state: 'enabled' })],
    });
    render(WorldDatapacks, {
      props: { instanceId: 'inst-1', world: 'MyWorld', running: true },
    });
    const addBtn = await screen.findByTestId('world-datapack-add-library');
    const toggleBtn = screen.getByTestId('world-datapack-toggle');
    const removeBtn = screen.getByTestId('world-datapack-remove-world');
    expect((addBtn as HTMLButtonElement).disabled).toBe(true);
    expect((toggleBtn as HTMLButtonElement).disabled).toBe(true);
    expect((removeBtn as HTMLButtonElement).disabled).toBe(true);
  });
});

describe('WorldDatapacks — disabled controls stay keyboard-reachable for their tooltip', () => {
  it('the tooltip-wrapper span around a disabled control gains tabindex="0", and drops it again once re-enabled', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.datapacksListForWorld).mockResolvedValueOnce({
      status: 'ok',
      data: [makePack({ filename: 'gate-pack.zip', state: 'enabled' })],
    });
    const { rerender } = render(WorldDatapacks, {
      props: { instanceId: 'inst-1', world: 'MyWorld', running: true },
    });
    const toggleBtn = await screen.findByTestId('world-datapack-toggle');
    const wrapperWhileDisabled = toggleBtn.closest('span');
    // A disabled <button> is unreachable by Tab, so the wrapping span picks up
    // the tab stop instead — otherwise tooltip.ts's focusin handler (which
    // fires on the span, see tooltip.ts) never gets a chance to run and the
    // "why is this disabled" text is mouse-only.
    expect(wrapperWhileDisabled?.getAttribute('tabindex')).toBe('0');

    await rerender({ instanceId: 'inst-1', world: 'MyWorld', running: false });
    // Once the control is enabled again it is directly focusable itself, so
    // the wrapper must NOT also be a tab stop — that would be a second,
    // redundant stop for the same control.
    expect(wrapperWhileDisabled?.getAttribute('tabindex')).toBeNull();
  });
});

describe('WorldDatapacks — command error surfaces', () => {
  it('shows the formatted error when datapacksSetEnabledInWorld fails, instead of swallowing it', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.datapacksListForWorld).mockResolvedValueOnce({
      status: 'ok',
      data: [makePack({ filename: 'err-pack.zip', state: 'enabled' })],
    });
    vi.mocked(commands.datapacksSetEnabledInWorld).mockResolvedValueOnce({
      status: 'error',
      error: { kind: 'io', path: '/datapacks', details: 'disk full' },
    });
    render(WorldDatapacks, { props: { instanceId: 'inst-1', world: 'MyWorld' } });
    const toggleBtn = await screen.findByRole('button', { name: /^disable in this world$/i });
    await fireEvent.click(toggleBtn);
    const err = await screen.findByText(/IO error at \/datapacks/i);
    expect(err.className).toContain('text-danger');
  });
});

// A failed reload after a SUCCESSFUL action — the action itself worked, only
// the follow-up list refresh failed. Reachable with no race: click Disable →
// the backend call succeeds → reload() fails. Before this fix, `packs` was
// never cleared on a failed reload, and the template's loadError / list
// conditionals were independent — so the user saw a red error AND the same
// pack still rendered as "Enabled" with a live, clickable toggle: the UI
// contradicting an action that actually worked.
describe('WorldDatapacks — a reload that fails after a successful action', () => {
  it('shows the error and renders NO pack row, instead of a stale interactive row next to it', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.datapacksListForWorld)
      .mockResolvedValueOnce({
        status: 'ok',
        data: [makePack({ filename: 'flaky-pack.zip', state: 'enabled' })],
      })
      .mockResolvedValueOnce({
        status: 'error',
        error: { kind: 'io', path: '/datapacks', details: 'reload failed' },
      });
    vi.mocked(commands.datapacksSetEnabledInWorld).mockResolvedValueOnce({
      status: 'ok',
      data: null,
    });
    render(WorldDatapacks, { props: { instanceId: 'inst-1', world: 'MyWorld' } });
    const toggleBtn = await screen.findByRole('button', { name: /^disable in this world$/i });
    // The disable command ITSELF succeeds; only the follow-up reload fails.
    await fireEvent.click(toggleBtn);
    await screen.findByText(/IO error at \/datapacks/i);
    // No stale row: neither the filename nor its toggle survive the failed
    // reload — an error and an interactive "still enabled" row must never
    // render together.
    expect(screen.queryByText('flaky-pack.zip')).toBeNull();
    expect(screen.queryByTestId('world-datapack-toggle')).toBeNull();
    expect(screen.queryByText(/no datapacks yet/i)).toBeNull();
  });
});

describe('WorldDatapacks — unknown compatibility', () => {
  it('renders a neutral "compatibility unknown" indicator rather than looking identical to a compatible pack', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.datapacksListForWorld).mockResolvedValueOnce({
      status: 'ok',
      data: [
        makePack({
          filename: 'unknown-compat.zip',
          state: 'enabled',
          compat: { kind: 'unknown' },
        }),
      ],
    });
    render(WorldDatapacks, { props: { instanceId: 'inst-1', world: 'MyWorld' } });
    await screen.findByText('unknown-compat.zip');
    expect(screen.getByText(/compatibility unknown/i)).toBeTruthy();
  });
});

// The whole premise of the feature — a world's datapacks each carrying their
// own independent state — was never exercised by a fixture with more than one
// pack. Four packs, one per WorldPackState, rendered together: each row must
// stay visually distinguishable and every action must target only its own
// filename, never a neighbour's.
describe('WorldDatapacks — mixed state list (all four states rendered together)', () => {
  function mixedPacks(): WorldDatapack[] {
    return [
      makePack({ filename: 'enabled-mix.zip', state: 'enabled' }),
      makePack({ filename: 'disabled-mix.zip', state: 'disabled' }),
      makePack({ filename: 'notadded-mix.zip', state: 'not_added' }),
      makePack({ filename: 'orphaned-mix.zip', state: 'orphaned' }),
    ];
  }

  function rowFor(filename: string): HTMLElement {
    const cell = screen.getByText(filename);
    const row = cell.closest('[data-card-shell]');
    if (!row) throw new Error(`no row rendered for ${filename}`);
    return row as HTMLElement;
  }

  it('renders one row per pack and dims only the disabled one (the shared card-status.ts convention)', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.datapacksListForWorld).mockResolvedValueOnce({
      status: 'ok',
      data: mixedPacks(),
    });
    render(WorldDatapacks, { props: { instanceId: 'inst-1', world: 'MixedWorld' } });
    await screen.findByText('enabled-mix.zip');
    await screen.findByText('disabled-mix.zip');
    await screen.findByText('notadded-mix.zip');
    await screen.findByText('orphaned-mix.zip');

    const enabledRow = rowFor('enabled-mix.zip');
    const disabledRow = rowFor('disabled-mix.zip');
    const notAddedRow = rowFor('notadded-mix.zip');
    const orphanedRow = rowFor('orphaned-mix.zip');

    expect(disabledRow.className).toContain('opacity-60');
    expect(enabledRow.className).not.toContain('opacity-60');
    expect(notAddedRow.className).not.toContain('opacity-60');
    expect(orphanedRow.className).not.toContain('opacity-60');

    // Distinct accent strips per state (data-card-accent is CardShell's own
    // accent strip hook — see src/lib/ui/cards/CardShell.svelte).
    expect(orphanedRow.querySelector('[data-card-accent]')?.className).toContain('bg-danger');
    expect(disabledRow.querySelector('[data-card-accent]')?.className).toContain(
      'bg-border-emphasis',
    );
    expect(enabledRow.querySelector('[data-card-accent]')?.className).toContain('bg-transparent');
    expect(notAddedRow.querySelector('[data-card-accent]')?.className).toContain('bg-transparent');
  });

  it('the enabled row toggle targets only its own filename', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.datapacksListForWorld).mockResolvedValueOnce({
      status: 'ok',
      data: mixedPacks(),
    });
    render(WorldDatapacks, { props: { instanceId: 'inst-1', world: 'MixedWorld' } });
    await screen.findByText('enabled-mix.zip');
    const row = rowFor('enabled-mix.zip');
    await fireEvent.click(within(row).getByTestId('world-datapack-toggle'));
    expect(commands.datapacksSetEnabledInWorld).toHaveBeenCalledWith(
      'inst-1',
      'MixedWorld',
      'enabled-mix.zip',
      false,
    );
  });

  it('the disabled row toggle targets only its own filename', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.datapacksListForWorld).mockResolvedValueOnce({
      status: 'ok',
      data: mixedPacks(),
    });
    render(WorldDatapacks, { props: { instanceId: 'inst-1', world: 'MixedWorld' } });
    await screen.findByText('disabled-mix.zip');
    const row = rowFor('disabled-mix.zip');
    await fireEvent.click(within(row).getByTestId('world-datapack-toggle'));
    expect(commands.datapacksSetEnabledInWorld).toHaveBeenCalledWith(
      'inst-1',
      'MixedWorld',
      'disabled-mix.zip',
      true,
    );
  });

  it('the not_added row add-to-world action targets only its own filename', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.datapacksListForWorld).mockResolvedValueOnce({
      status: 'ok',
      data: mixedPacks(),
    });
    render(WorldDatapacks, { props: { instanceId: 'inst-1', world: 'MixedWorld' } });
    await screen.findByText('notadded-mix.zip');
    const row = rowFor('notadded-mix.zip');
    await fireEvent.click(within(row).getByTestId('world-datapack-add-world'));
    expect(commands.datapacksAddToWorld).toHaveBeenCalledWith(
      'inst-1',
      'MixedWorld',
      'notadded-mix.zip',
    );
  });

  it('the orphaned row remove action targets only its own filename', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.datapacksListForWorld).mockResolvedValueOnce({
      status: 'ok',
      data: mixedPacks(),
    });
    render(WorldDatapacks, { props: { instanceId: 'inst-1', world: 'MixedWorld' } });
    await screen.findByText('orphaned-mix.zip');
    const row = rowFor('orphaned-mix.zip');
    await fireEvent.click(within(row).getByTestId('world-datapack-remove-orphaned'));
    expect(commands.datapacksRemoveFromWorld).toHaveBeenCalledWith(
      'inst-1',
      'MixedWorld',
      'orphaned-mix.zip',
    );
  });
});

// WorldDatapacks panel — unit coverage for the empty state, each WorldPackState
// row shape (enabled/disabled toggle, orphaned removal, not_added add), the
// format-mismatch warning, the running-instance gate, error surfacing, and a
// reload that fails AFTER a successful action (the stale-row bug).
//
// WorldDatapacks does not import or mount ContextualTour (that overlay lives in
// WorldsTab, gated on the worlds list being non-empty) — so, unlike
// tests/worlds-tab.test.ts and tests/intent/worlds.test.ts, no
// markSeen('worlds') call is needed here: the panel alone cannot trigger it.

import { fireEvent, render, screen } from '@testing-library/svelte';
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
    const removeBtn = screen.getByTestId('world-datapack-remove');
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
    // The header's own "Add datapack" (library-install) button shares the
    // world-datapack-add testid with this row's button, so disambiguate by
    // accessible name rather than testid.
    const addBtn = await screen.findByRole('button', { name: /^add to this world$/i });
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
    const addBtn = await screen.findByTestId('world-datapack-add');
    const toggleBtn = screen.getByTestId('world-datapack-toggle');
    const removeBtn = screen.getByTestId('world-datapack-remove');
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

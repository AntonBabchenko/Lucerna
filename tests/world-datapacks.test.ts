// WorldDatapacks panel — unit coverage for the empty state, each WorldPackState
// row shape (enabled/disabled toggle, orphaned removal, not_added add), the
// game's compatibility verdicts, the unknown-compatibility indicator, the
// running-instance gate, error surfacing (including a reload that fails AFTER
// a successful action — the stale-row bug), and a mixed-state list rendering
// all four states at once.
//
// WorldDatapacks does not import or mount ContextualTour (that overlay lives in
// WorldsTab, gated on the worlds list being non-empty) — so, unlike
// tests/worlds-tab.test.ts and tests/intent/worlds.test.ts, no
// markSeen('worlds') call is needed here: the panel alone cannot trigger it.

import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type {
  LevelDatPresence,
  PackCompat,
  WorldDatapack,
  WorldDatapackListing,
} from '$lib/ipc/bindings';
import { hideTooltip, tooltipState } from '$lib/ui/tooltip/tooltip-controller.svelte';
import WorldDatapacks from '$lib/worlds/WorldDatapacks.svelte';
import { revealTooltip } from './test-utils/reveal-tooltip';

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    datapacksListForWorld: vi
      .fn()
      .mockResolvedValue({ status: 'ok', data: { level_dat: 'present', packs: [] } }),
    datapacksListLibrary: vi.fn().mockResolvedValue({
      status: 'ok',
      data: { entries: [], worlds: [] },
    }),
    datapacksInstallFromFile: vi.fn(),
    datapacksAddToWorld: vi.fn().mockResolvedValue({ status: 'ok', data: 'linked' }),
    datapacksRemoveFromWorld: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    // What the this-world removal confirmation asks before it offers its button (U1).
    datapacksWorldEntryKind: vi
      .fn()
      .mockResolvedValue({ status: 'ok', data: { kind: 'library_copy' } }),
    datapacksRemoveFromLibrary: vi.fn(),
    datapacksSetEnabledInWorld: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
  },
}));

afterEach(() => {
  hideTooltip();
  vi.clearAllMocks();
});

function makePack(over: Partial<WorldDatapack> = {}): WorldDatapack {
  return {
    filename: 'test-pack.zip',
    state: 'enabled',
    ignored_reason: null,
    in_library: true,
    compat: { kind: 'compatible' },
    ...over,
  };
}

/** The `datapacksListForWorld` payload: the rows plus the world's level.dat presence. */
function listing(
  packs: WorldDatapack[],
  level_dat: LevelDatPresence = 'present',
): WorldDatapackListing {
  return { level_dat, packs };
}

// The explainer is the shared DatapackConceptHelp component; its copy and
// explanation-level behaviour are asserted once, in
// tests/datapack-concept-help.test.ts. What this pins is PRESENCE on this
// surface — a different claim from copy, and one nothing else covers: deleting
// the tag from WorldDatapacks.svelte is otherwise invisible to every suite.
describe('WorldDatapacks — datapack concept explainer', () => {
  it('renders the "What are data packs?" help trigger beside the panel title', async () => {
    render(WorldDatapacks, { props: { instanceId: 'inst-1', world: 'MyWorld' } });
    expect(await screen.findByRole('button', { name: /what are data packs\?/i })).toBeTruthy();
  });
});

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
      data: listing([makePack({ filename: 'enabled-pack.zip', state: 'enabled' })]),
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
  it('explains the game drops the name itself and offers Clear entry instead of a toggle', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.datapacksListForWorld).mockResolvedValueOnce({
      status: 'ok',
      data: listing([makePack({ filename: 'gone-pack.zip', state: 'orphaned' })]),
    });
    render(WorldDatapacks, { props: { instanceId: 'inst-1', world: 'MyWorld' } });
    // Engine truth: Minecraft logs "Missing data pack", skips the id and drops
    // it at its next save — there is no prompt (spec 2026-09-24 §4 U3).
    await screen.findByText(/drops the name the next time the world is saved/i);
    expect(screen.queryByText(/will ask/i)).toBeNull();
    expect(screen.queryByRole('button', { name: /^enable in this world$/i })).toBeNull();
    expect(screen.queryByRole('button', { name: /^disable in this world$/i })).toBeNull();
    const clear = screen.getByTestId('world-datapack-remove-orphaned');
    expect(clear.textContent).toMatch(/clear entry/i);
    await fireEvent.click(clear);
    // It deletes no file, so it needs no confirmation.
    expect(screen.queryByTestId('datapack-remove-dialog')).toBeNull();
    expect(commands.datapacksRemoveFromWorld).toHaveBeenCalledWith(
      'inst-1',
      'MyWorld',
      'gone-pack.zip',
    );
  });

  it('an orphaned row is a quiet, self-healing state', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.datapacksListForWorld).mockResolvedValueOnce({
      status: 'ok',
      data: listing([makePack({ filename: 'gone-pack.zip', state: 'orphaned' })]),
    });
    render(WorldDatapacks, { props: { instanceId: 'inst-1', world: 'MyWorld' } });
    const hint = await screen.findByText(/drops the name the next time the world is saved/i);
    expect(hint.className).toContain('text-muted');
    const row = screen.getByText('gone-pack.zip').closest('[data-card-shell]') as HTMLElement;
    expect(row.querySelector('[data-card-accent]')?.className).toContain('bg-transparent');
    expect(row.innerHTML).not.toMatch(/bg-danger|text-danger/);
    expect(within(row).getByText('File missing')).toBeTruthy();
  });

  it('a Disabled-only ghost not in the library offers Clear entry, not Add to this world', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.datapacksListForWorld).mockResolvedValueOnce({
      status: 'ok',
      data: listing([makePack({ filename: 'gone.zip', state: 'not_added', in_library: false })]),
    });
    render(WorldDatapacks, { props: { instanceId: 'inst-1', world: 'MyWorld' } });
    const clear = await screen.findByTestId('world-datapack-remove-orphaned');
    expect(clear.textContent).toMatch(/clear entry/i);
    expect(screen.queryByTestId('world-datapack-add-world')).toBeNull();
  });
});

describe('WorldDatapacks — not_added row', () => {
  it('offers "Add to this world" and calls datapacksAddToWorld', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.datapacksListForWorld).mockResolvedValueOnce({
      status: 'ok',
      data: listing([makePack({ filename: 'library-pack.zip', state: 'not_added' })]),
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

describe('WorldDatapacks — compatibility is the game’s own verdict (§1 C5)', () => {
  async function renderWith(compat: PackCompat, state: WorldDatapack['state'] = 'enabled') {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.datapacksListForWorld).mockResolvedValueOnce({
      status: 'ok',
      data: listing([makePack({ filename: 'p.zip', state, compat })]),
    });
    render(WorldDatapacks, { props: { instanceId: 'inst-1', world: 'MyWorld' } });
    return screen.findByTestId('world-datapack-compat');
  }

  it('too_old says the game still loads it', async () => {
    const line = await renderWith({ kind: 'too_old', made_for: '15', game: '48' });
    expect(line.textContent).toMatch(/older version of Minecraft/);
    expect(line.textContent).toMatch(/data pack format 15; this version uses 48/);
    expect(line.textContent).toMatch(/still loads/);
    expect(line.className).toContain('text-warning-text');
    expect(screen.queryByText(/stop the world from loading/)).toBeNull();
    const row = line.closest('[data-card-shell]');
    expect(row?.querySelector('[data-card-accent]')?.className).toContain('bg-warning-text');
  });

  it('too_new says the game still loads it', async () => {
    const line = await renderWith({ kind: 'too_new', made_for: '107.1', game: '94.1' });
    expect(line.textContent).toMatch(/newer version of Minecraft/);
    expect(line.textContent).toMatch(/still loads/);
    expect(screen.queryByText(/stop the world from loading/)).toBeNull();
  });

  it('broken uses the game’s own label and says it still loads', async () => {
    const line = await renderWith({ kind: 'broken' });
    expect(line.textContent).toMatch(/Broken or incompatible/);
    expect(line.textContent).toMatch(/still loads/);
  });

  it('a library pack not in this world that the game skips says why', async () => {
    const line = await renderWith({ kind: 'wont_load', reason: 'no_pack_format' }, 'not_added');
    expect(line.textContent).toMatch(/This version of Minecraft skips this pack/);
  });
});

describe('WorldDatapacks — packs this version skips (§0.5 A1, I11)', () => {
  it.each([
    ['no_pack_mcmeta', /no pack\.mcmeta at its top level/],
    ['no_pack_section', /has no "pack" section/],
    ['no_description', /has no "description"/],
    ['no_pack_format', /has no "pack_format"/],
  ] as const)('a not-loadable row (%s) shows the compat reason and no toggle', async (reason, text) => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.datapacksListForWorld).mockResolvedValueOnce({
      status: 'ok',
      data: listing([
        makePack({
          filename: 'skipped.zip',
          state: 'ignored',
          ignored_reason: 'not_loadable',
          compat: { kind: 'wont_load', reason },
        }),
      ]),
    });
    render(WorldDatapacks, { props: { instanceId: 'inst-1', world: 'MyWorld' } });
    expect(await screen.findByText(text)).toBeTruthy();
    expect(screen.queryByTestId('world-datapack-toggle')).toBeNull();
    expect(screen.queryByRole('button', { name: /^enable in this world$/i })).toBeNull();
    expect(screen.queryByRole('button', { name: /^disable in this world$/i })).toBeNull();
  });

  it('an addable pack this version skips offers no add', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.datapacksListForWorld).mockResolvedValueOnce({
      status: 'ok',
      data: listing([
        makePack({
          filename: 'nullscape.zip',
          state: 'not_added',
          compat: { kind: 'wont_load', reason: 'no_pack_format' },
        }),
      ]),
    });
    render(WorldDatapacks, { props: { instanceId: 'inst-1', world: 'MyWorld' } });
    expect(await screen.findByText(/This version of Minecraft skips this pack/)).toBeTruthy();
    expect(screen.queryByTestId('world-datapack-add-world')).toBeNull();
  });
});

describe('WorldDatapacks — running disables mutating controls', () => {
  it('disables the add, toggle, and remove controls while the instance is running', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.datapacksListForWorld).mockResolvedValueOnce({
      status: 'ok',
      data: listing([makePack({ filename: 'running-pack.zip', state: 'enabled' })]),
    });
    render(WorldDatapacks, {
      props: { instanceId: 'inst-1', world: 'MyWorld', running: true },
    });
    const addBtn = await screen.findByTestId('world-datapack-add-library');
    const addFolderBtn = screen.getByTestId('world-datapack-add-library-folder');
    const toggleBtn = screen.getByTestId('world-datapack-toggle');
    const removeBtn = screen.getByTestId('world-datapack-remove-world');
    expect((addBtn as HTMLButtonElement).disabled).toBe(true);
    expect((addFolderBtn as HTMLButtonElement).disabled).toBe(true);
    expect((toggleBtn as HTMLButtonElement).disabled).toBe(true);
    expect((removeBtn as HTMLButtonElement).disabled).toBe(true);
  });
});

describe('WorldDatapacks — disabled controls stay keyboard-reachable for their tooltip', () => {
  it('the tooltip-wrapper span around a disabled control gains tabindex="0", and drops it again once re-enabled', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.datapacksListForWorld).mockResolvedValueOnce({
      status: 'ok',
      data: listing([makePack({ filename: 'gate-pack.zip', state: 'enabled' })]),
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
      data: listing([makePack({ filename: 'err-pack.zip', state: 'enabled' })]),
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
        data: listing([makePack({ filename: 'flaky-pack.zip', state: 'enabled' })]),
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
      data: listing([
        makePack({
          filename: 'unknown-compat.zip',
          state: 'enabled',
          compat: { kind: 'unknown' },
        }),
      ]),
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
      data: listing(mixedPacks()),
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
    // A ghost is not a problem: the game drops the id itself (U3).
    expect(orphanedRow.querySelector('[data-card-accent]')?.className).toContain('bg-transparent');
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
      data: listing(mixedPacks()),
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
      data: listing(mixedPacks()),
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
      data: listing(mixedPacks()),
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
      data: listing(mixedPacks()),
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

// §3 L.8: an only-old world's rows stay visible (they keep their state
// badges). Which controls stay live there is a separate concern, not pinned
// here.
describe('WorldDatapacks — listing envelope', () => {
  it('renders the rows of any listing, whatever its level.dat presence', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.datapacksListForWorld).mockResolvedValueOnce({
      status: 'ok',
      data: listing([makePack({ filename: 'backup-pack.zip', state: 'disabled' })], 'only_old'),
    });
    render(WorldDatapacks, { props: { instanceId: 'inst-1', world: 'MyWorld' } });
    expect(await screen.findByText('backup-pack.zip')).toBeTruthy();
    expect(screen.queryByText(/No datapacks yet/i)).toBeNull();
  });
});

describe('WorldDatapacks — a row the game ignores', () => {
  it('shows its reason and offers removal but no toggle', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.datapacksListForWorld).mockResolvedValueOnce({
      status: 'ok',
      data: listing([
        makePack({
          filename: 'Loose',
          state: 'ignored',
          ignored_reason: 'folder_without_pack_mcmeta',
          in_library: false,
          compat: { kind: 'unknown' },
        }),
      ]),
    });
    render(WorldDatapacks, { props: { instanceId: 'inst-1', world: 'MyWorld' } });
    await screen.findByText('Ignored by the game');
    expect(screen.getByText(/pack\.mcmeta sits directly inside it/i)).toBeTruthy();
    expect(screen.queryByTestId('world-datapack-toggle')).toBeNull();
    expect(screen.queryByTestId('world-datapack-add-world')).toBeNull();
    expect(screen.queryByText('Compatibility unknown')).toBeNull();
    // The trash asks first (U1): a folder the library does not hold is the
    // only copy, and removing it deletes it permanently.
    vi.mocked(commands.datapacksWorldEntryKind).mockResolvedValueOnce({
      status: 'ok',
      data: { kind: 'own_folder' },
    });
    await fireEvent.click(screen.getByTestId('world-datapack-remove-world'));
    const dialog = await screen.findByTestId('datapack-remove-dialog');
    expect(
      await within(dialog).findByText(/the folder and everything in it permanently/i),
    ).toBeTruthy();
    expect(commands.datapacksRemoveFromWorld).not.toHaveBeenCalled();
    const confirm = screen.getByTestId('datapack-remove-confirm') as HTMLButtonElement;
    await waitFor(() => expect(confirm.disabled).toBe(false));
    await fireEvent.click(confirm);
    await waitFor(() =>
      expect(commands.datapacksRemoveFromWorld).toHaveBeenCalledWith('inst-1', 'MyWorld', 'Loose'),
    );
  });
});

describe('WorldDatapacks — removing a pack from this world (U1)', () => {
  it('the world-row trash opens the removal dialog and removes nothing until confirmed', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    // Once each: the first load, then the reload after the removal (the dialog's
    // onRemoved). A persistent value would leak into every later test.
    vi.mocked(commands.datapacksListForWorld)
      .mockResolvedValueOnce({
        status: 'ok',
        data: listing([makePack({ filename: 'trash-me.zip', state: 'enabled' })]),
      })
      .mockResolvedValueOnce({ status: 'ok', data: listing([]) });
    render(WorldDatapacks, { props: { instanceId: 'inst-1', world: 'MyWorld' } });
    await fireEvent.click(await screen.findByTestId('world-datapack-remove-world'));
    expect(await screen.findByTestId('datapack-remove-dialog')).toBeTruthy();
    expect(commands.datapacksRemoveFromWorld).not.toHaveBeenCalled();
    const confirm = screen.getByTestId('datapack-remove-confirm') as HTMLButtonElement;
    await waitFor(() => expect(confirm.disabled).toBe(false));
    await fireEvent.click(confirm);
    await waitFor(() =>
      expect(commands.datapacksRemoveFromWorld).toHaveBeenCalledWith(
        'inst-1',
        'MyWorld',
        'trash-me.zip',
      ),
    );
  });
});

describe('WorldDatapacks — level.dat presence (D2)', () => {
  it('only_old disables every change and explains how to restore level.dat', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.datapacksListForWorld).mockResolvedValueOnce({
      status: 'ok',
      data: listing(
        [
          makePack({ filename: 'live.zip', state: 'disabled' }),
          makePack({ filename: 'addable.zip', state: 'not_added' }),
          makePack({ filename: 'gone.zip', state: 'orphaned' }),
          makePack({
            filename: 'Loose',
            state: 'ignored',
            ignored_reason: 'folder_without_pack_mcmeta',
            in_library: false,
          }),
        ],
        'only_old',
      ),
    });
    render(WorldDatapacks, { props: { instanceId: 'inst-1', world: 'MyWorld' } });
    const note = await screen.findByTestId('world-datapacks-level-dat-note');
    expect(note.textContent).toMatch(/Attempt to Restore/);
    // Rows stay, read-only, with the state level.dat_old holds.
    expect(screen.getByText('live.zip')).toBeTruthy();
    for (const id of [
      'world-datapack-add-library',
      'world-datapack-add-library-folder',
      'world-datapack-toggle',
      'world-datapack-add-world',
      'world-datapack-remove-orphaned',
    ]) {
      expect((screen.getByTestId(id) as HTMLButtonElement).disabled, id).toBe(true);
    }
    // Both trash buttons: the live row's and the ignored row's.
    const trashes = screen.getAllByTestId('world-datapack-remove-world') as HTMLButtonElement[];
    expect(trashes).toHaveLength(2);
    expect(trashes.every((b) => b.disabled)).toBe(true);
    revealTooltip(screen.getByTestId('world-datapack-toggle').closest('span') as HTMLElement);
    expect(tooltipState.text).toBe('Open this world in Minecraft and restore it from the backup');
  });

  it("absent says Minecraft doesn't treat the folder as a world", async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.datapacksListForWorld).mockResolvedValueOnce({
      status: 'ok',
      data: listing([], 'absent'),
    });
    render(WorldDatapacks, { props: { instanceId: 'inst-1', world: 'NotAWorld' } });
    const note = await screen.findByTestId('world-datapacks-level-dat-note');
    expect(note.textContent).toMatch(/no level\.dat, so Minecraft doesn't list it as a world/);
    expect(screen.queryByText(/No datapacks yet/i)).toBeNull();
    const add = screen.getByTestId('world-datapack-add-library') as HTMLButtonElement;
    expect(add.disabled).toBe(true);
    revealTooltip(add.closest('span') as HTMLElement);
    expect(tooltipState.text).toBe('Not a world: this folder has no level.dat');
  });
});

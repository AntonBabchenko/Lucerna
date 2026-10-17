import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import type { KindView, SectionState } from '$lib/ipc/bindings';

// The Shared game settings rows (spec 2026-10-15 §B9): a checkbox per kind
// with what it is doing, the two questions a join can raise, the unreadable
// sync file's way out, and a kept failure.

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    settingsSyncState: vi.fn(),
    settingsSyncSetKind: vi.fn(),
    settingsSyncStop: vi.fn(),
  },
}));

import SettingsSyncRows from '$lib/instances/settings/SettingsSyncRows.svelte';
import { joinChoices, kindStateKey } from '$lib/instances/settings/settings-sync-view';
import { commands } from '$lib/ipc/bindings';

const view = (over: Partial<KindView> & Pick<KindView, 'kind'>): KindView => ({
  enabled: false,
  join: 'done',
  availability: { status: 'available' },
  join_asks: false,
  ...over,
});

const state = (kinds: KindView[], over: Partial<SectionState> = {}): SectionState => ({
  sidecar: { status: 'readable' },
  kinds,
  last_error: null,
  ...over,
});

const ALL_OFF = [
  view({ kind: 'servers' }),
  view({ kind: 'history' }),
  view({ kind: 'hotbars' }),
  view({ kind: 'options' }),
];

function serve(s: SectionState) {
  vi.mocked(commands.settingsSyncState).mockResolvedValue({ status: 'ok', data: s });
}

function renderRows(over: { running?: boolean; busy?: boolean } = {}) {
  return render(SettingsSyncRows, {
    props: { instanceId: 'inst-1', running: over.running ?? false, busy: over.busy ?? false },
  });
}

beforeEach(() => {
  locale.set('en');
  vi.mocked(commands.settingsSyncState).mockReset();
  vi.mocked(commands.settingsSyncSetKind).mockReset();
  vi.mocked(commands.settingsSyncStop).mockReset();
});

describe('kindStateKey', () => {
  it('names the states a checkbox cannot say, and nothing for plain on or off', () => {
    expect(kindStateKey(view({ kind: 'servers' }))).toBeNull();
    expect(kindStateKey(view({ kind: 'servers', enabled: true }))).toBeNull();
    expect(kindStateKey(view({ kind: 'servers', enabled: true, join: 'use_shared' }))).toBe(
      'instance.settingsSync.state.joining',
    );
    expect(
      kindStateKey(
        view({ kind: 'history', availability: { status: 'unsupported', why: 'history_too_old' } }),
      ),
    ).toBe('instance.settingsSync.state.historyTooOld');
    expect(
      kindStateKey(view({ kind: 'hotbars', availability: { status: 'older_than_shared' } })),
    ).toBe('instance.settingsSync.state.olderThanShared');
  });
});

describe('the options row', () => {
  it('says why it waits and why a version cannot sync it', () => {
    const waiting = (why: 'first_launch' | 'game_save') =>
      kindStateKey(view({ kind: 'options', availability: { status: 'waiting', why } }));
    expect(waiting('first_launch')).toBe('instance.settingsSync.state.optionsFirstLaunch');
    expect(waiting('game_save')).toBe('instance.settingsSync.state.optionsGameSave');
    const unsupported = (
      why: 'options_too_old' | 'options_too_new' | 'options_not_release' | 'options_not_installed',
    ) => kindStateKey(view({ kind: 'options', availability: { status: 'unsupported', why } }));
    expect(unsupported('options_too_old')).toBe('instance.settingsSync.state.optionsTooOld');
    expect(unsupported('options_too_new')).toBe('instance.settingsSync.state.optionsTooNew');
    expect(unsupported('options_not_release')).toBe(
      'instance.settingsSync.state.optionsNotRelease',
    );
    expect(unsupported('options_not_installed')).toBe(
      'instance.settingsSync.state.optionsNotInstalled',
    );
  });

  it('asks which settings to keep only when both sides have some', () => {
    expect(joinChoices(view({ kind: 'options' }))).toBeNull();
    expect(joinChoices(view({ kind: 'options', join_asks: true }))).toEqual([
      'use_shared',
      'make_shared',
    ]);
  });
});

describe('joinChoices', () => {
  it('asks only for hotbars that would replace something, and an older instance can only share', () => {
    expect(joinChoices(view({ kind: 'servers', join_asks: true }))).toBeNull();
    expect(joinChoices(view({ kind: 'hotbars' }))).toBeNull();
    expect(joinChoices(view({ kind: 'hotbars', join_asks: true }))).toEqual([
      'use_shared',
      'make_shared',
    ]);
    expect(
      joinChoices(
        view({ kind: 'hotbars', join_asks: true, availability: { status: 'older_than_shared' } }),
      ),
    ).toEqual(['make_shared']);
  });
});

describe('SettingsSyncRows', () => {
  it('turns a kind on with no question when nothing would be replaced', async () => {
    serve(state(ALL_OFF));
    vi.mocked(commands.settingsSyncSetKind).mockResolvedValue({
      status: 'ok',
      data: { kind: 'joined' },
    });
    renderRows();
    await fireEvent.click(await screen.findByTestId('settings-sync-toggle-servers'));
    expect(commands.settingsSyncSetKind).toHaveBeenCalledWith(
      'inst-1',
      'servers',
      true,
      null,
      false,
    );
  });

  it('asks which hotbars to keep, and a cancel leaves the box unticked', async () => {
    serve(state([view({ kind: 'hotbars', join_asks: true })]));
    renderRows();
    const box = (await screen.findByTestId('settings-sync-toggle-hotbars')) as HTMLInputElement;
    await fireEvent.click(box);
    expect(await screen.findByTestId('settings-sync-use-shared')).toBeTruthy();
    expect(commands.settingsSyncSetKind).not.toHaveBeenCalled();
    await fireEvent.click(screen.getByText('Cancel'));
    expect(box.checked).toBe(false);
  });

  it('joins with the hotbars the user picked', async () => {
    serve(state([view({ kind: 'hotbars', join_asks: true })]));
    vi.mocked(commands.settingsSyncSetKind).mockResolvedValue({
      status: 'ok',
      data: { kind: 'joined' },
    });
    renderRows();
    await fireEvent.click(await screen.findByTestId('settings-sync-toggle-hotbars'));
    await fireEvent.click(await screen.findByTestId('settings-sync-use-shared'));
    // The confirm runs through the dialog's busy wrapper: a tick later.
    await waitFor(() =>
      expect(commands.settingsSyncSetKind).toHaveBeenCalledWith(
        'inst-1',
        'hotbars',
        true,
        'use_shared',
        false,
      ),
    );
  });

  it('offers an instance older than the shared hotbars only to share its own', async () => {
    serve(
      state([
        view({ kind: 'hotbars', join_asks: true, availability: { status: 'older_than_shared' } }),
      ]),
    );
    renderRows();
    vi.mocked(commands.settingsSyncSetKind).mockResolvedValue({
      status: 'ok',
      data: { kind: 'joined' },
    });
    await fireEvent.click(await screen.findByTestId('settings-sync-toggle-hotbars'));
    const share = await screen.findByTestId('settings-sync-make-shared');
    expect(screen.queryByTestId('settings-sync-use-shared')).toBeNull();
    await fireEvent.click(share);
    await waitFor(() =>
      expect(commands.settingsSyncSetKind).toHaveBeenCalledWith(
        'inst-1',
        'hotbars',
        true,
        'make_shared',
        false,
      ),
    );
  });

  it('asks whether a game from before a restart is closed, then joins with the answer', async () => {
    serve(state(ALL_OFF));
    vi.mocked(commands.settingsSyncSetKind)
      .mockResolvedValueOnce({ status: 'error', error: { kind: 'settings_sync_ask_game_closed' } })
      .mockResolvedValueOnce({ status: 'ok', data: { kind: 'joined' } });
    renderRows();
    await fireEvent.click(await screen.findByTestId('settings-sync-toggle-history'));
    await fireEvent.click(await screen.findByTestId('settings-sync-its-closed'));
    await waitFor(() =>
      expect(commands.settingsSyncSetKind).toHaveBeenLastCalledWith(
        'inst-1',
        'history',
        true,
        null,
        true,
      ),
    );
  });

  it('a refused change says why and unticks the box', async () => {
    serve(state(ALL_OFF));
    vi.mocked(commands.settingsSyncSetKind).mockResolvedValue({
      status: 'error',
      error: { kind: 'settings_sync_game_running' },
    });
    renderRows();
    const box = (await screen.findByTestId('settings-sync-toggle-servers')) as HTMLInputElement;
    await fireEvent.click(box);
    expect((await screen.findByTestId('settings-sync-change-failed')).textContent).toContain(
      'still running',
    );
    expect(box.checked).toBe(false);
  });

  it('an unreadable sync file offers only the way out, which stops every kind', async () => {
    serve(state([], { sidecar: { status: 'unreadable', reason: 'damaged' } }));
    vi.mocked(commands.settingsSyncStop).mockResolvedValue({ status: 'ok', data: null });
    renderRows();
    expect((await screen.findByTestId('settings-sync-unreadable')).textContent).toContain(
      'the file is damaged or cut short',
    );
    await fireEvent.click(screen.getByTestId('settings-sync-stop-all'));
    expect(commands.settingsSyncStop).toHaveBeenCalledWith('inst-1', null);
  });

  it('shows a kept failure, naming what did not sync', async () => {
    serve(
      state(ALL_OFF, {
        last_error: { kind: 'history', file: 'game', reason: 'no_access' },
      }),
    );
    renderRows();
    const line = await screen.findByTestId('settings-sync-last-error');
    expect(line.textContent).toContain('the command history');
    expect(line.textContent).toContain("File: this instance's command_history.txt.");
    expect(line.textContent).toContain('Reason: no access to the file');
  });

  it('is locked while the game runs, and says so', async () => {
    serve(state(ALL_OFF));
    renderRows({ running: true });
    const box = (await screen.findByTestId('settings-sync-toggle-servers')) as HTMLInputElement;
    expect(box.disabled).toBe(true);
    expect(screen.getByTestId('settings-sync-locked').textContent).toContain('Stop the game');
    expect(box.getAttribute('aria-describedby')).toContain('settings-sync-locked');
  });

  it("never shows another instance's rows while this one's are read", async () => {
    serve(state([view({ kind: 'servers', enabled: true })]));
    const { rerender } = renderRows();
    await screen.findByTestId('settings-sync-toggle-servers');
    let answer!: (v: Awaited<ReturnType<typeof commands.settingsSyncState>>) => void;
    vi.mocked(commands.settingsSyncState).mockReturnValueOnce(
      new Promise((r) => {
        answer = r;
      }),
    );
    await rerender({ instanceId: 'inst-2', running: false, busy: false });
    await waitFor(() => expect(commands.settingsSyncState).toHaveBeenLastCalledWith('inst-2'));
    expect(screen.queryByTestId('settings-sync-toggle-servers')).toBeNull();
    answer({ status: 'ok', data: state(ALL_OFF) });
    expect(
      ((await screen.findByTestId('settings-sync-toggle-servers')) as HTMLInputElement).checked,
    ).toBe(false);
  });

  it('offers Try again after a failed read, and reads again', async () => {
    vi.mocked(commands.settingsSyncState)
      .mockResolvedValueOnce({ status: 'error', error: { kind: 'instance_busy' } })
      .mockResolvedValueOnce({ status: 'ok', data: state(ALL_OFF) });
    renderRows();
    await fireEvent.click(await screen.findByTestId('settings-sync-retry'));
    expect(await screen.findByTestId('settings-sync-toggle-servers')).toBeTruthy();
  });

  it('says which hotbars file cannot be read, on the hotbars row only', async () => {
    serve(
      state([
        view({ kind: 'servers' }),
        view({
          kind: 'hotbars',
          availability: { status: 'unreadable', file: 'shared', reason: 'damaged' },
        }),
      ]),
    );
    renderRows();
    const line = await screen.findByTestId('settings-sync-state-hotbars');
    expect(line.textContent).toBe(
      'Not available. File: the shared hotbars. Reason: the file is damaged or cut short.',
    );
    expect(screen.queryByTestId('settings-sync-state-servers')).toBeNull();
  });

  it("makes this instance's hotbars the shared ones from the two-choice dialog", async () => {
    serve(state([view({ kind: 'hotbars', join_asks: true })]));
    vi.mocked(commands.settingsSyncSetKind).mockResolvedValue({
      status: 'ok',
      data: { kind: 'joined' },
    });
    renderRows();
    await fireEvent.click(await screen.findByTestId('settings-sync-toggle-hotbars'));
    await screen.findByTestId('settings-sync-use-shared');
    await fireEvent.click(screen.getByTestId('settings-sync-make-shared'));
    await waitFor(() =>
      expect(commands.settingsSyncSetKind).toHaveBeenCalledWith(
        'inst-1',
        'hotbars',
        true,
        'make_shared',
        false,
      ),
    );
  });

  it('says before the first launch when shared options apply, and what they cover', async () => {
    serve(
      state([view({ kind: 'options', availability: { status: 'waiting', why: 'first_launch' } })]),
    );
    renderRows();
    expect((await screen.findByTestId('settings-sync-state-options')).textContent).toBe(
      'Shared settings apply after the first launch.',
    );
    expect(screen.getByTestId('settings-sync-rule-options').textContent).toContain(
      'Render distance',
    );
  });

  it('asks about game options in their own words and joins with the choice', async () => {
    serve(state([view({ kind: 'options', join_asks: true })]));
    vi.mocked(commands.settingsSyncSetKind).mockResolvedValue({
      status: 'ok',
      data: { kind: 'joined' },
    });
    renderRows();
    await fireEvent.click(await screen.findByTestId('settings-sync-toggle-options'));
    expect(await screen.findByText('Shared game options already exist')).toBeTruthy();
    expect(screen.getByTestId('settings-sync-use-shared').textContent).toContain(
      'Use the shared options',
    );
    await fireEvent.click(screen.getByTestId('settings-sync-make-shared'));
    await waitFor(() =>
      expect(commands.settingsSyncSetKind).toHaveBeenCalledWith(
        'inst-1',
        'options',
        true,
        'make_shared',
        false,
      ),
    );
  });

  it('turns sync off once for a double click on the way out', async () => {
    serve(state([], { sidecar: { status: 'unreadable', reason: 'damaged' } }));
    let done!: (v: Awaited<ReturnType<typeof commands.settingsSyncStop>>) => void;
    vi.mocked(commands.settingsSyncStop).mockReturnValue(
      new Promise((r) => {
        done = r;
      }),
    );
    renderRows();
    const button = await screen.findByTestId('settings-sync-stop-all');
    await fireEvent.click(button);
    await fireEvent.click(button);
    expect(commands.settingsSyncStop).toHaveBeenCalledTimes(1);
    done({ status: 'ok', data: null });
  });
});

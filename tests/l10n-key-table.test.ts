import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { KeyRow } from '$lib/ipc/bindings';

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    l10nNamespaceKeys: vi.fn(),
    l10nSetOverride: vi.fn(),
    l10nRevertMachine: vi.fn(),
  },
}));

import { commands } from '$lib/ipc/bindings';
import KeyEditRow from '$lib/l10n/KeyEditRow.svelte';
import KeyTable from '$lib/l10n/KeyTable.svelte';

function keyRow(over: Partial<KeyRow> = {}): KeyRow {
  return {
    key: 'item.create.wrench',
    sourceEn: 'Wrench',
    modValue: null,
    overrideValue: null,
    state: 'missing',
    origin: null,
    ...over,
  };
}

function mockKeysOk(rows: KeyRow[]) {
  vi.mocked(commands.l10nNamespaceKeys).mockResolvedValue({
    status: 'ok',
    data: rows,
    // biome-ignore lint/suspicious/noExplicitAny: mocked IPC envelope
  } as any);
}

const props = { instanceId: 'inst-1', namespace: 'create', lang: 'ru_ru' };

afterEach(() => {
  vi.clearAllMocks();
});

describe('KeyTable', () => {
  it('fetches the namespace keys on mount', async () => {
    mockKeysOk([keyRow()]);
    render(KeyTable, { props });
    await waitFor(() =>
      expect(commands.l10nNamespaceKeys).toHaveBeenCalledWith('inst-1', 'create', 'ru_ru'),
    );
  });

  it('refetches when the namespace changes', async () => {
    mockKeysOk([keyRow()]);
    const { rerender } = render(KeyTable, { props });
    await waitFor(() =>
      expect(commands.l10nNamespaceKeys).toHaveBeenCalledWith('inst-1', 'create', 'ru_ru'),
    );
    await rerender({ ...props, namespace: 'thermal' });
    await waitFor(() =>
      expect(commands.l10nNamespaceKeys).toHaveBeenCalledWith('inst-1', 'thermal', 'ru_ru'),
    );
  });

  it('renders a row per key once loaded', async () => {
    mockKeysOk([keyRow({ key: 'a', sourceEn: 'A' }), keyRow({ key: 'b', sourceEn: 'B' })]);
    render(KeyTable, { props });
    expect(await screen.findAllByTestId('l10n-key-row')).toHaveLength(2);
  });

  it('surfaces a load failure on its own, not as an empty table', async () => {
    vi.mocked(commands.l10nNamespaceKeys).mockResolvedValue({
      status: 'error',
      error: { kind: 'io', path: 'p', details: 'nope' },
      // biome-ignore lint/suspicious/noExplicitAny: mocked IPC envelope
    } as any);
    render(KeyTable, { props });
    const err = await screen.findByTestId('l10n-key-table-error');
    // Appears after mount, replacing content the user was looking at, so it
    // has to interrupt rather than wait to be tabbed into.
    expect(err.getAttribute('role')).toBe('alert');
    expect(screen.queryByTestId('l10n-key-table-empty')).toBeNull();
  });

  it('searches by key text', async () => {
    mockKeysOk([
      keyRow({ key: 'gui.create.wrench', sourceEn: 'Wrench' }),
      keyRow({ key: 'gui.create.saw', sourceEn: 'Saw' }),
    ]);
    render(KeyTable, { props });
    await screen.findAllByTestId('l10n-key-row');
    await fireEvent.input(screen.getByTestId('l10n-key-search'), { target: { value: 'wrench' } });
    await waitFor(() => expect(screen.getAllByTestId('l10n-key-row')).toHaveLength(1));
    expect(screen.getByText('gui.create.wrench')).toBeTruthy();
  });

  it('searches by English source text too', async () => {
    mockKeysOk([
      keyRow({ key: 'gui.create.wrench', sourceEn: 'Wrench' }),
      keyRow({ key: 'gui.create.saw', sourceEn: 'Saw' }),
    ]);
    render(KeyTable, { props });
    await screen.findAllByTestId('l10n-key-row');
    await fireEvent.input(screen.getByTestId('l10n-key-search'), { target: { value: 'saw' } });
    await waitFor(() => expect(screen.getAllByTestId('l10n-key-row')).toHaveLength(1));
    expect(screen.getByText('gui.create.saw')).toBeTruthy();
  });

  it('filter chips carry per-bucket counts and narrow the visible rows', async () => {
    mockKeysOk([
      keyRow({ key: 'a', state: 'from_mod', modValue: 'A' }),
      keyRow({ key: 'b', state: 'ok', overrideValue: 'B' }),
      keyRow({ key: 'c', state: 'missing' }),
      keyRow({ key: 'd', state: 'stale', overrideValue: 'D', modValue: 'D (new)' }),
      keyRow({ key: 'e', state: 'orphan', overrideValue: 'E' }),
    ]);
    render(KeyTable, { props });
    await screen.findAllByTestId('l10n-key-row');

    expect(within(screen.getByTestId('l10n-filter-all')).getByText('5')).toBeTruthy();
    expect(within(screen.getByTestId('l10n-filter-translated')).getByText('2')).toBeTruthy();
    expect(within(screen.getByTestId('l10n-filter-missing')).getByText('1')).toBeTruthy();
    expect(within(screen.getByTestId('l10n-filter-stale')).getByText('1')).toBeTruthy();
    expect(within(screen.getByTestId('l10n-filter-orphan')).getByText('1')).toBeTruthy();

    await fireEvent.click(screen.getByTestId('l10n-filter-orphan'));
    await waitFor(() => expect(screen.getAllByTestId('l10n-key-row')).toHaveLength(1));
    expect(screen.getByText('e')).toBeTruthy();
  });

  it('paginates when a namespace has more keys than one page', async () => {
    mockKeysOk(Array.from({ length: 55 }, (_, i) => keyRow({ key: `k${i}`, sourceEn: `K${i}` })));
    render(KeyTable, { props });
    // Default page size is 50 (one of the shared PAGE_SIZES) — first page full.
    await waitFor(() => expect(screen.getAllByTestId('l10n-key-row')).toHaveLength(50));
    await fireEvent.click(screen.getByTestId('pg-next'));
    await waitFor(() => expect(screen.getAllByTestId('l10n-key-row')).toHaveLength(5));
  });

  it('starts a new namespace on the first page', async () => {
    mockKeysOk(Array.from({ length: 55 }, (_, i) => keyRow({ key: `k${i}`, sourceEn: `K${i}` })));
    const { rerender } = render(KeyTable, { props });
    await waitFor(() => expect(screen.getAllByTestId('l10n-key-row')).toHaveLength(50));
    await fireEvent.click(screen.getByTestId('pg-next'));
    await waitFor(() => expect(screen.getAllByTestId('l10n-key-row')).toHaveLength(5));

    // Same size, so the clamp effect has nothing to clamp — only an explicit
    // reset can bring the user back to the top of an unrelated key set.
    mockKeysOk(Array.from({ length: 55 }, (_, i) => keyRow({ key: `t${i}`, sourceEn: `T${i}` })));
    await rerender({ ...props, namespace: 'thermal' });
    await waitFor(() =>
      expect(commands.l10nNamespaceKeys).toHaveBeenCalledWith('inst-1', 'thermal', 'ru_ru'),
    );

    await waitFor(() => expect(screen.getAllByTestId('l10n-key-row')).toHaveLength(50));
  });

  it('lets the user save a new override, and reflects the resulting state without a refetch', async () => {
    mockKeysOk([keyRow({ key: 'a', sourceEn: 'A', state: 'missing' })]);
    vi.mocked(commands.l10nSetOverride).mockResolvedValue({
      status: 'ok',
      data: null,
      // biome-ignore lint/suspicious/noExplicitAny: mocked IPC envelope
    } as any);
    render(KeyTable, { props });
    await screen.findAllByTestId('l10n-key-row');

    const input = screen.getByTestId('l10n-key-input');
    await fireEvent.input(input, { target: { value: 'А' } });
    await fireEvent.click(screen.getByTestId('l10n-key-save'));

    await waitFor(() =>
      expect(commands.l10nSetOverride).toHaveBeenCalledWith('create', 'ru_ru', 'a', 'А', 'A'),
    );
    // Exactly one call: saving a row must not trigger a full namespace refetch.
    expect(commands.l10nNamespaceKeys).toHaveBeenCalledTimes(1);
    await waitFor(() =>
      expect(screen.getByTestId('l10n-key-state').textContent?.trim()).toBe('Translated'),
    );
  });

  it('disables Save while the draft is empty, so blanking the field can never silently clear an override', async () => {
    mockKeysOk([keyRow({ key: 'a', sourceEn: 'A', state: 'ok', overrideValue: 'А' })]);
    render(KeyTable, { props });
    await screen.findAllByTestId('l10n-key-row');

    const input = screen.getByTestId('l10n-key-input');
    await fireEvent.input(input, { target: { value: '' } });
    expect((screen.getByTestId('l10n-key-save') as HTMLButtonElement).disabled).toBe(true);
  });

  it('shows a rejected translation on the row itself, not as a toast', async () => {
    mockKeysOk([keyRow({ key: 'a', sourceEn: 'A', state: 'missing' })]);
    vi.mocked(commands.l10nSetOverride).mockResolvedValue({
      status: 'error',
      error: {
        kind: 'l10n_translation_invalid',
        key: 'a',
        reason: { kind: 'unsupported_specifier', specifier: 'd' },
      },
      // biome-ignore lint/suspicious/noExplicitAny: mocked IPC envelope
    } as any);
    render(KeyTable, { props });
    await screen.findAllByTestId('l10n-key-row');

    await fireEvent.input(screen.getByTestId('l10n-key-input'), { target: { value: '%d bad' } });
    await fireEvent.click(screen.getByTestId('l10n-key-save'));

    const err = await screen.findByTestId('l10n-key-error');
    expect(err.textContent).toBeTruthy();
    // The row's state must not have silently flipped to ok on a rejection.
    // Trimmed on purpose: StatusBadge's template puts whitespace around the
    // label, so an untrimmed compare would pass against 'Translated' too and
    // prove nothing.
    expect(screen.getByTestId('l10n-key-state').textContent?.trim()).not.toBe('Translated');
  });

  it('clears an override via the explicit Clear action', async () => {
    mockKeysOk([
      keyRow({ key: 'a', sourceEn: 'A', state: 'ok', overrideValue: 'А', modValue: null }),
    ]);
    vi.mocked(commands.l10nSetOverride).mockResolvedValue({
      status: 'ok',
      data: null,
      // biome-ignore lint/suspicious/noExplicitAny: mocked IPC envelope
    } as any);
    render(KeyTable, { props });
    await screen.findAllByTestId('l10n-key-row');

    await fireEvent.click(screen.getByTestId('l10n-key-clear'));

    await waitFor(() =>
      expect(commands.l10nSetOverride).toHaveBeenCalledWith('create', 'ru_ru', 'a', '', 'A'),
    );
    // No mod translation behind it, so clearing drops it to "missing" — and the
    // Clear button itself disappears since there is nothing left to clear.
    await waitFor(() =>
      expect(screen.getByTestId('l10n-key-state').textContent?.trim()).toBe('Untranslated'),
    );
    expect(screen.queryByTestId('l10n-key-clear')).toBeNull();
  });

  it('keeps an orphaned override findable and clearable — its only path back', async () => {
    mockKeysOk([
      keyRow({
        key: 'gui.dropped.key',
        sourceEn: 'Old text',
        state: 'orphan',
        overrideValue: 'Старый текст',
        modValue: null,
      }),
    ]);
    render(KeyTable, { props });
    await screen.findAllByTestId('l10n-key-row');

    expect(screen.getByTestId('l10n-key-state').textContent?.trim()).toBe('Removed from mod');
    expect(screen.getByTestId('l10n-key-clear')).toBeTruthy();
  });

  it('calls onOverrideSaved after a successful save, so the caller can refresh coverage', async () => {
    mockKeysOk([keyRow({ key: 'a', sourceEn: 'A', state: 'missing' })]);
    vi.mocked(commands.l10nSetOverride).mockResolvedValue({
      status: 'ok',
      data: null,
      // biome-ignore lint/suspicious/noExplicitAny: mocked IPC envelope
    } as any);
    const onOverrideSaved = vi.fn();
    render(KeyTable, { props: { ...props, onOverrideSaved } });
    await screen.findAllByTestId('l10n-key-row');

    await fireEvent.input(screen.getByTestId('l10n-key-input'), { target: { value: 'А' } });
    await fireEvent.click(screen.getByTestId('l10n-key-save'));

    await waitFor(() => expect(onOverrideSaved).toHaveBeenCalledTimes(1));
  });

  it('resets search and filter when switching to a different namespace', async () => {
    mockKeysOk([keyRow({ key: 'gui.create.wrench', sourceEn: 'Wrench' })]);
    const { rerender } = render(KeyTable, { props });
    await screen.findAllByTestId('l10n-key-row');
    await fireEvent.input(screen.getByTestId('l10n-key-search'), { target: { value: 'wrench' } });
    expect((screen.getByTestId('l10n-key-search') as HTMLInputElement).value).toBe('wrench');

    mockKeysOk([keyRow({ key: 'gui.thermal.pipe', sourceEn: 'Pipe' })]);
    await rerender({ ...props, namespace: 'thermal' });
    await waitFor(() =>
      expect(commands.l10nNamespaceKeys).toHaveBeenCalledWith('inst-1', 'thermal', 'ru_ru'),
    );
    await waitFor(() =>
      expect((screen.getByTestId('l10n-key-search') as HTMLInputElement).value).toBe(''),
    );
  });

  it('names the namespace and its counts in a pane header', async () => {
    mockKeysOk([
      keyRow({ key: 'a', sourceEn: 'A', overrideValue: 'Aa', state: 'ok', origin: 'manual' }),
      keyRow({ key: 'b', sourceEn: 'B', state: 'missing' }),
    ]);
    render(KeyTable, { props: { ...props, namespace: 'quark' } });
    const header = await screen.findByTestId('l10n-pane-header');
    expect(header.textContent).toContain('quark');
    expect(header.textContent).toContain('2');
  });

  it('blames the filter, not a search term, when the search box is empty', async () => {
    mockKeysOk([keyRow({ key: 'a', sourceEn: 'A', state: 'missing' })]);
    render(KeyTable, { props });
    await screen.findByTestId('l10n-key-row');

    await fireEvent.click(screen.getByTestId('l10n-filter-translated'));

    const empty = await screen.findByTestId('l10n-key-table-empty');
    expect(empty.textContent).toBe(
      'No keys match the current filter. Clear the filter to see the rest.',
    );
  });

  it('hides the pagination footer when everything fits on one page', async () => {
    mockKeysOk([keyRow({ key: 'a', sourceEn: 'A', state: 'missing' })]);
    render(KeyTable, { props });
    await screen.findByTestId('l10n-key-row');
    expect(screen.queryByTestId('pg-first')).toBeNull();
  });

  it('renders the AI marker as an info StatusBadge with no native title attribute', async () => {
    mockKeysOk([
      keyRow({
        key: 'gui.quark.rune',
        sourceEn: 'Rune',
        overrideValue: 'Руна',
        state: 'ok',
        origin: 'machine',
      }),
    ]);
    render(KeyTable, { props: { ...props, namespace: 'quark' } });
    const badge = await screen.findByTestId('l10n-key-origin-machine');
    expect(badge.className).toContain('bg-accent-soft');
    // DESIGN.md §5 bans native title=: it is invisible to keyboard users and
    // unstyleable. Both the marker's explanation and the truncated key go
    // through use:tooltip instead.
    expect(badge.getAttribute('title')).toBeNull();
    expect(screen.getByText('gui.quark.rune').getAttribute('title')).toBeNull();
  });

  it('announces a successful save and links the error to the input', async () => {
    mockKeysOk([keyRow({ key: 'a', sourceEn: 'A', state: 'missing' })]);
    vi.mocked(commands.l10nSetOverride).mockResolvedValue({
      status: 'ok',
      data: null,
      // biome-ignore lint/suspicious/noExplicitAny: mocked IPC envelope
    } as any);
    render(KeyTable, { props });
    await screen.findByTestId('l10n-key-row');

    await fireEvent.input(screen.getByTestId('l10n-key-input'), { target: { value: 'Я' } });
    await fireEvent.click(screen.getByTestId('l10n-key-save'));

    const live = await screen.findByTestId('l10n-key-live');
    await waitFor(() => expect(live.textContent).toBe('Saved'));
    expect(live.getAttribute('aria-live')).toBe('polite');
  });

  // A live region announces text CHANGES, so re-writing the same "Saved"
  // string would be silent — the save-fix-save-again flow would be announced
  // exactly once per row. Clearing on input restores the transition.
  it('clears the announcement on the next keystroke so a second save is audible', async () => {
    mockKeysOk([keyRow({ key: 'a', sourceEn: 'A', state: 'missing' })]);
    vi.mocked(commands.l10nSetOverride).mockResolvedValue({
      status: 'ok',
      data: null,
      // biome-ignore lint/suspicious/noExplicitAny: mocked IPC envelope
    } as any);
    render(KeyTable, { props });
    await screen.findByTestId('l10n-key-row');

    await fireEvent.input(screen.getByTestId('l10n-key-input'), { target: { value: 'Я' } });
    await fireEvent.click(screen.getByTestId('l10n-key-save'));
    const live = await screen.findByTestId('l10n-key-live');
    await waitFor(() => expect(live.textContent).toBe('Saved'));

    await fireEvent.input(screen.getByTestId('l10n-key-input'), { target: { value: 'Ян' } });
    expect(live.textContent).toBe('');
  });

  it('does not double-announce a rejection that the alert paragraph already carries', async () => {
    mockKeysOk([keyRow({ key: 'a', sourceEn: 'A', state: 'missing' })]);
    vi.mocked(commands.l10nSetOverride).mockResolvedValue({
      status: 'error',
      error: { kind: 'io', path: 'p', details: 'nope' },
      // biome-ignore lint/suspicious/noExplicitAny: mocked IPC envelope
    } as any);
    render(KeyTable, { props });
    await screen.findByTestId('l10n-key-row');

    await fireEvent.input(screen.getByTestId('l10n-key-input'), { target: { value: 'Я' } });
    await fireEvent.click(screen.getByTestId('l10n-key-save'));

    const alert = await screen.findByTestId('l10n-key-error');
    expect(alert.getAttribute('role')).toBe('alert');
    // The assertive alert owns the error; the polite region must stay empty or
    // assistive tech reads the same sentence twice.
    expect(screen.getByTestId('l10n-key-live').textContent).toBe('');
  });

  it('marks the input invalid and points it at the rejection text', async () => {
    mockKeysOk([keyRow({ key: 'a', sourceEn: 'A', state: 'missing' })]);
    vi.mocked(commands.l10nSetOverride).mockResolvedValue({
      status: 'error',
      error: {
        kind: 'l10n_translation_invalid',
        key: 'a',
        reason: { kind: 'unsupported_specifier', specifier: 'd' },
      },
      // biome-ignore lint/suspicious/noExplicitAny: mocked IPC envelope
    } as any);
    render(KeyTable, { props });
    await screen.findByTestId('l10n-key-row');

    const input = screen.getByTestId('l10n-key-input');
    // A field that is merely empty is not invalid — the flag has to arrive
    // with the rejection, not sit on the input from the start.
    expect(input.getAttribute('aria-invalid')).toBeNull();

    await fireEvent.input(input, { target: { value: '%d bad' } });
    await fireEvent.click(screen.getByTestId('l10n-key-save'));

    const err = await screen.findByTestId('l10n-key-error');
    expect(err.getAttribute('role')).toBe('alert');
    expect(err.id).toBeTruthy();
    await waitFor(() => expect(input.getAttribute('aria-invalid')).toBe('true'));
    expect(input.getAttribute('aria-describedby')).toBe(err.id);
  });

  it('names the row after the key it edits', async () => {
    mockKeysOk([keyRow({ key: 'gui.quark.rune', sourceEn: 'Rune' })]);
    render(KeyTable, { props });
    const row = await screen.findByTestId('l10n-key-row');
    expect(row.getAttribute('role')).toBe('group');
    expect(row.getAttribute('aria-label')).toBe('gui.quark.rune');
  });

  describe('one filter axis', () => {
    // The inversion of the old two-axis behaviour. Origin is not a narrowing
    // of the state selection — it is a view of its own, because a key's origin
    // exists only when the key has an override, so "untranslated AND
    // AI-written" was empty by construction rather than by data.
    it('the AI view shows every AI-written row regardless of state', async () => {
      mockKeysOk([
        keyRow({ key: 'a', state: 'ok', overrideValue: 'А', origin: 'machine' }),
        keyRow({ key: 'b', state: 'stale', overrideValue: 'Б', modValue: 'B2', origin: 'machine' }),
        keyRow({ key: 'c', state: 'ok', overrideValue: 'В', origin: 'manual' }),
      ]);
      render(KeyTable, { props });
      await screen.findAllByTestId('l10n-key-row');

      await fireEvent.click(screen.getByTestId('l10n-filter-machine'));
      await waitFor(() => expect(screen.getAllByTestId('l10n-key-row')).toHaveLength(2));
      expect(screen.getByText('a')).toBeTruthy();
      expect(screen.getByText('b')).toBeTruthy();
    });

    it('shows an origin view only once it has something in it', async () => {
      mockKeysOk([
        keyRow({ key: 'a', state: 'ok', overrideValue: 'А', origin: 'manual' }),
        keyRow({ key: 'b', state: 'missing' }),
      ]);
      render(KeyTable, { props });
      await screen.findAllByTestId('l10n-key-row');

      // Positive half first, so this test cannot pass merely because a testid
      // does not exist yet.
      expect(within(screen.getByTestId('l10n-filter-manual')).getByText('1')).toBeTruthy();
      expect(screen.queryByTestId('l10n-filter-machine')).toBeNull();
      // The anchors stay whatever the data says.
      expect(within(screen.getByTestId('l10n-filter-missing')).getByText('1')).toBeTruthy();
    });

    it('resets on a namespace switch, so an AI-only view cannot strand the user on an empty table', async () => {
      mockKeysOk([keyRow({ key: 'a', state: 'ok', overrideValue: 'А', origin: 'machine' })]);
      const { rerender } = render(KeyTable, { props });
      await screen.findAllByTestId('l10n-key-row');
      await fireEvent.click(screen.getByTestId('l10n-filter-machine'));
      await waitFor(() => expect(screen.getAllByTestId('l10n-key-row')).toHaveLength(1));

      // A namespace the pre-fill never touched: every row has origin null, so
      // a carried-over "AI only" selection would hide all of them with no
      // visible reason why.
      mockKeysOk([keyRow({ key: 'z', state: 'missing', origin: null })]);
      await rerender({ ...props, namespace: 'thermal' });
      await waitFor(() =>
        expect(commands.l10nNamespaceKeys).toHaveBeenCalledWith('inst-1', 'thermal', 'ru_ru'),
      );

      await waitFor(() => expect(screen.getAllByTestId('l10n-key-row')).toHaveLength(1));
      expect(screen.queryByTestId('l10n-key-table-empty')).toBeNull();
    });
  });

  describe('sticky rows', () => {
    function mockSaveOk() {
      vi.mocked(commands.l10nSetOverride).mockResolvedValue({
        status: 'ok',
        data: null,
        // biome-ignore lint/suspicious/noExplicitAny: mocked IPC envelope
      } as any);
    }

    it('keeps a just-saved row in place instead of yanking it out of the view', async () => {
      mockKeysOk([
        keyRow({ key: 'a', sourceEn: 'A', state: 'missing' }),
        keyRow({ key: 'b', sourceEn: 'B', state: 'missing' }),
      ]);
      mockSaveOk();
      render(KeyTable, { props });
      await screen.findAllByTestId('l10n-key-row');
      await fireEvent.click(screen.getByTestId('l10n-filter-missing'));
      await waitFor(() => expect(screen.getAllByTestId('l10n-key-row')).toHaveLength(2));

      await fireEvent.input(screen.getAllByTestId('l10n-key-input')[0], {
        target: { value: 'А' },
      });
      await fireEvent.click(screen.getAllByTestId('l10n-key-save')[0]);

      // The badge flip IS the confirmation — and it only survives because the
      // row does.
      await waitFor(() =>
        expect(screen.getAllByTestId('l10n-key-state')[0].textContent?.trim()).toBe('Translated'),
      );
      expect(screen.getAllByTestId('l10n-key-row')).toHaveLength(2);
    });

    it('offers a refresh that drops the changed rows and then goes away', async () => {
      mockKeysOk([
        keyRow({ key: 'a', sourceEn: 'A', state: 'missing' }),
        keyRow({ key: 'b', sourceEn: 'B', state: 'missing' }),
      ]);
      mockSaveOk();
      render(KeyTable, { props });
      await screen.findAllByTestId('l10n-key-row');
      await fireEvent.click(screen.getByTestId('l10n-filter-missing'));
      expect(screen.queryByTestId('l10n-sticky-refresh')).toBeNull();

      await fireEvent.input(screen.getAllByTestId('l10n-key-input')[0], {
        target: { value: 'А' },
      });
      await fireEvent.click(screen.getAllByTestId('l10n-key-save')[0]);

      const refresh = await screen.findByTestId('l10n-sticky-refresh');
      expect(refresh.textContent).toContain('1');
      await fireEvent.click(refresh);

      await waitFor(() => expect(screen.getAllByTestId('l10n-key-row')).toHaveLength(1));
      expect(screen.queryByTestId('l10n-sticky-refresh')).toBeNull();
    });

    it('has nothing to refresh under All, where a saved row still belongs', async () => {
      mockKeysOk([keyRow({ key: 'a', sourceEn: 'A', state: 'missing' })]);
      mockSaveOk();
      render(KeyTable, { props });
      await screen.findByTestId('l10n-key-row');

      await fireEvent.input(screen.getByTestId('l10n-key-input'), { target: { value: 'А' } });
      await fireEvent.click(screen.getByTestId('l10n-key-save'));
      await waitFor(() =>
        expect(screen.getByTestId('l10n-key-state').textContent?.trim()).toBe('Translated'),
      );

      expect(screen.queryByTestId('l10n-sticky-refresh')).toBeNull();
    });

    it('exempts a stuck row from the view but not from the search', async () => {
      mockKeysOk([
        keyRow({ key: 'alpha', sourceEn: 'Alpha', state: 'missing' }),
        keyRow({ key: 'beta', sourceEn: 'Beta', state: 'missing' }),
      ]);
      mockSaveOk();
      render(KeyTable, { props });
      await screen.findAllByTestId('l10n-key-row');
      await fireEvent.click(screen.getByTestId('l10n-filter-missing'));

      await fireEvent.input(screen.getAllByTestId('l10n-key-input')[0], {
        target: { value: 'А' },
      });
      await fireEvent.click(screen.getAllByTestId('l10n-key-save')[0]);
      await waitFor(() => expect(screen.getAllByTestId('l10n-key-row')).toHaveLength(2));

      await fireEvent.input(screen.getByTestId('l10n-key-search'), { target: { value: 'beta' } });
      await waitFor(() => expect(screen.getAllByTestId('l10n-key-row')).toHaveLength(1));
      expect(screen.getByText('beta')).toBeTruthy();
    });

    it('keeps the chip of the view you are standing in when its last row goes sticky', async () => {
      mockKeysOk([
        keyRow({ key: 'a', sourceEn: 'A', state: 'stale', overrideValue: 'А', modValue: 'A2' }),
        keyRow({ key: 'b', sourceEn: 'B', state: 'missing' }),
      ]);
      mockSaveOk();
      render(KeyTable, { props });
      await screen.findAllByTestId('l10n-key-row');
      await fireEvent.click(screen.getByTestId('l10n-filter-stale'));
      await waitFor(() => expect(screen.getAllByTestId('l10n-key-row')).toHaveLength(1));

      await fireEvent.input(screen.getByTestId('l10n-key-input'), { target: { value: 'Новое' } });
      await fireEvent.click(screen.getByTestId('l10n-key-save'));
      await waitFor(() =>
        expect(screen.getByTestId('l10n-key-state').textContent?.trim()).toBe('Translated'),
      );

      // The count is now 0, but the chip must not vanish: its disappearance
      // would trip the fallback effect and flood the table with the whole mod.
      expect(screen.getByTestId('l10n-filter-stale')).toBeTruthy();
      expect(screen.getAllByTestId('l10n-key-row')).toHaveLength(1);
    });

    it('drops the sticky rows when the data is refetched', async () => {
      mockKeysOk([
        keyRow({ key: 'a', sourceEn: 'A', state: 'missing' }),
        keyRow({ key: 'b', sourceEn: 'B', state: 'missing' }),
      ]);
      mockSaveOk();
      const { rerender } = render(KeyTable, { props });
      await screen.findAllByTestId('l10n-key-row');
      await fireEvent.click(screen.getByTestId('l10n-filter-missing'));
      await fireEvent.input(screen.getAllByTestId('l10n-key-input')[0], {
        target: { value: 'А' },
      });
      await fireEvent.click(screen.getAllByTestId('l10n-key-save')[0]);
      await screen.findByTestId('l10n-sticky-refresh');

      // The AI pre-fill's door into this component. What is on screen after it
      // is the backend's truth; a "you just did this" claim cannot outlive the
      // data it was made about.
      //
      // A second, DIFFERENT response: `a` comes back translated while the view
      // is still Untranslated. It no longer matches on its own merits, so the
      // only thing that could keep it on screen is a sticky flag that survived
      // the refetch — which is exactly what must not happen.
      mockKeysOk([
        keyRow({ key: 'a', sourceEn: 'A', state: 'ok', overrideValue: 'А', origin: 'manual' }),
        keyRow({ key: 'b', sourceEn: 'B', state: 'missing' }),
      ]);
      await rerender({ ...props, reloadToken: 1 });
      await waitFor(() => expect(commands.l10nNamespaceKeys).toHaveBeenCalledTimes(2));
      await waitFor(() => expect(screen.getAllByTestId('l10n-key-row')).toHaveLength(1));
      expect(screen.getByText('b')).toBeTruthy();
      expect(screen.queryByTestId('l10n-sticky-refresh')).toBeNull();
    });

    // The bulk revert calls load() directly: instanceId, namespace, lang and
    // reloadToken are all unchanged, so the fetch effect never re-runs. A clear
    // wired to that effect instead of to load() would pass every other test in
    // this block and still leave sticky keys pointing at rows the backend has
    // just deleted.
    it('drops the sticky rows when a bulk AI revert refetches', async () => {
      mockKeysOk([
        keyRow({ key: 'a', sourceEn: 'A', state: 'ok', overrideValue: 'А', origin: 'machine' }),
        keyRow({ key: 'b', sourceEn: 'B', state: 'ok', overrideValue: 'Б', origin: 'machine' }),
      ]);
      mockSaveOk();
      vi.mocked(commands.l10nRevertMachine).mockResolvedValue({
        status: 'ok',
        data: 2,
        // biome-ignore lint/suspicious/noExplicitAny: mocked IPC envelope
      } as any);
      render(KeyTable, { props });
      await screen.findAllByTestId('l10n-key-row');

      // Editing an AI row reclaims it as manual, so it leaves the AI view and
      // goes sticky. 'b' stays machine, which keeps the revert button offered.
      await fireEvent.click(screen.getByTestId('l10n-filter-machine'));
      await fireEvent.input(screen.getAllByTestId('l10n-key-input')[0], {
        target: { value: 'Моё' },
      });
      await fireEvent.click(screen.getAllByTestId('l10n-key-save')[0]);
      await screen.findByTestId('l10n-sticky-refresh');

      // The post-revert truth: `a` is manual now (the user's edit reclaimed it)
      // and so falls outside the AI view. If the sticky flag survived load(),
      // it would still be on screen.
      mockKeysOk([
        keyRow({ key: 'a', sourceEn: 'A', state: 'ok', overrideValue: 'Моё', origin: 'manual' }),
        keyRow({ key: 'b', sourceEn: 'B', state: 'ok', overrideValue: 'Б', origin: 'machine' }),
      ]);
      await fireEvent.click(screen.getByTestId('l10n-revert-machine'));
      await fireEvent.click(await screen.findByTestId('l10n-revert-confirm'));

      await waitFor(() => expect(commands.l10nNamespaceKeys).toHaveBeenCalledTimes(2));
      await waitFor(() => expect(screen.getAllByTestId('l10n-key-row')).toHaveLength(1));
      expect(screen.getByText('b')).toBeTruthy();
      expect(screen.queryByTestId('l10n-sticky-refresh')).toBeNull();
    });

    it('drops the sticky rows when a view change re-runs the filter', async () => {
      mockKeysOk([
        keyRow({ key: 'a', sourceEn: 'A', state: 'missing' }),
        keyRow({ key: 'b', sourceEn: 'B', state: 'missing' }),
      ]);
      mockSaveOk();
      render(KeyTable, { props });
      await screen.findAllByTestId('l10n-key-row');
      await fireEvent.click(screen.getByTestId('l10n-filter-missing'));
      await fireEvent.input(screen.getAllByTestId('l10n-key-input')[0], {
        target: { value: 'А' },
      });
      await fireEvent.click(screen.getAllByTestId('l10n-key-save')[0]);
      await screen.findByTestId('l10n-sticky-refresh');

      await fireEvent.click(screen.getByTestId('l10n-filter-all'));
      await fireEvent.click(screen.getByTestId('l10n-filter-missing'));

      await waitFor(() => expect(screen.getAllByTestId('l10n-key-row')).toHaveLength(1));
      expect(screen.queryByTestId('l10n-sticky-refresh')).toBeNull();
    });
  });

  describe('bulk revert', () => {
    it('is offered only when the namespace actually has machine translations', async () => {
      mockKeysOk([keyRow({ key: 'a', state: 'ok', overrideValue: 'А', origin: 'manual' })]);
      const manualOnly = render(KeyTable, { props });
      await screen.findAllByTestId('l10n-key-row');
      expect(screen.queryByTestId('l10n-revert-machine')).toBeNull();
      manualOnly.unmount();

      mockKeysOk([keyRow({ key: 'a', state: 'ok', overrideValue: 'А', origin: 'machine' })]);
      render(KeyTable, { props });
      await screen.findAllByTestId('l10n-key-row');
      expect(screen.getByTestId('l10n-revert-machine')).toBeTruthy();
    });

    it('confirms first, then reverts in ONE command and refetches the namespace', async () => {
      mockKeysOk([keyRow({ key: 'a', state: 'ok', overrideValue: 'А', origin: 'machine' })]);
      vi.mocked(commands.l10nRevertMachine).mockResolvedValue({
        status: 'ok',
        data: 1,
        // biome-ignore lint/suspicious/noExplicitAny: mocked IPC envelope
      } as any);
      render(KeyTable, { props });
      await screen.findAllByTestId('l10n-key-row');

      await fireEvent.click(screen.getByTestId('l10n-revert-machine'));
      // Destructive and bulk — nothing happens until the confirm is pressed.
      expect(commands.l10nRevertMachine).not.toHaveBeenCalled();

      await fireEvent.click(await screen.findByTestId('l10n-revert-confirm'));
      await waitFor(() =>
        expect(commands.l10nRevertMachine).toHaveBeenCalledWith('inst-1', 'ru_ru', 'create'),
      );
      // One command for the whole namespace, not one per key.
      expect(commands.l10nRevertMachine).toHaveBeenCalledTimes(1);
      // The resulting state of each key depends on what the mod itself ships,
      // so the rows come back from the backend rather than being patched here.
      await waitFor(() => expect(commands.l10nNamespaceKeys).toHaveBeenCalledTimes(2));
    });

    it('counts the doomed translations in the singular when there is exactly one', async () => {
      mockKeysOk([keyRow({ key: 'a', state: 'ok', overrideValue: 'А', origin: 'machine' })]);
      render(KeyTable, { props });
      await screen.findAllByTestId('l10n-key-row');

      await fireEvent.click(screen.getByTestId('l10n-revert-machine'));

      const body = await screen.findByText(/This removes/);
      expect(body.textContent).toContain('This removes 1 AI translation from create.');
    });

    it('reports a failed revert inside the confirm, not behind it', async () => {
      mockKeysOk([keyRow({ key: 'a', state: 'ok', overrideValue: 'А', origin: 'machine' })]);
      vi.mocked(commands.l10nRevertMachine).mockResolvedValue({
        status: 'error',
        error: { kind: 'io', path: 'p', details: 'disk full' },
        // biome-ignore lint/suspicious/noExplicitAny: mocked IPC envelope
      } as any);
      render(KeyTable, { props });
      await screen.findAllByTestId('l10n-key-row');

      await fireEvent.click(screen.getByTestId('l10n-revert-machine'));
      await fireEvent.click(await screen.findByTestId('l10n-revert-confirm'));

      // The confirm stays up carrying the reason — an error rendered in the
      // toolbar underneath would be invisible behind the dialog's backdrop.
      await waitFor(() => expect(screen.getByText(/disk full/)).toBeTruthy());
      expect(screen.getByTestId('l10n-revert-confirm')).toBeTruthy();
      // Nothing changed, so nothing was refetched.
      expect(commands.l10nNamespaceKeys).toHaveBeenCalledTimes(1);
    });
  });

  // Regression coverage: translation keys are IDENTICAL across languages —
  // `l10nNamespaceKeys('inst-1', 'create', 'ru_ru')` and the 'de_de' call
  // return rows with the same `key`. Every mock below reuses key 'a' for both
  // languages on purpose: a key present in only one language would be
  // recreated anyway (different `paged` entry entirely) and would prove
  // nothing.
  //
  // Honesty note on what these tests actually pin down: KeyTable's each-block
  // is keyed by (namespace, lang, key), not just key, so KeyEditRow always
  // gets a fresh instance across a language switch — but `load()` ALSO sets
  // `loading = true` synchronously before every `await`, which hides the
  // whole {#each} behind {#if loading} and tears it down regardless of the
  // each-key (Promise continuations are always microtask-deferred, so that
  // commit is guaranteed — verified below by asserting the row's DOM node
  // identity actually changes). That means these tests would pass just as
  // well with a bare `row.key` each-key today; they exercise the
  // user-visible correctness the keying protects, not the keying mechanism
  // in isolation. The `KeyEditRow direct-reuse contract` group further down
  // isolates the mechanism itself, without KeyTable's loading gate in the way.
  describe('language switch', () => {
    function mockKeysPerLang(byLang: Record<string, KeyRow[]>) {
      vi.mocked(commands.l10nNamespaceKeys).mockImplementation(
        async (_id: string, _ns: string, lang: string) =>
          ({ status: 'ok', data: byLang[lang] ?? [] }) as Awaited<
            ReturnType<typeof commands.l10nNamespaceKeys>
          >,
      );
    }

    it('shows the new language value instead of a leftover draft after switching languages', async () => {
      mockKeysPerLang({
        ru_ru: [keyRow({ key: 'a', sourceEn: 'A', modValue: 'Русский', state: 'from_mod' })],
        de_de: [keyRow({ key: 'a', sourceEn: 'A', modValue: 'Deutsch', state: 'from_mod' })],
      });
      const { rerender } = render(KeyTable, { props });
      await waitFor(() =>
        expect((screen.getByTestId('l10n-key-input') as HTMLInputElement).value).toBe('Русский'),
      );

      // Type an in-progress edit that is never saved.
      await fireEvent.input(screen.getByTestId('l10n-key-input'), {
        target: { value: 'Мой черновик' },
      });
      expect((screen.getByTestId('l10n-key-input') as HTMLInputElement).value).toBe('Мой черновик');

      await rerender({ ...props, lang: 'de_de' });
      await waitFor(() =>
        expect(commands.l10nNamespaceKeys).toHaveBeenCalledWith('inst-1', 'create', 'de_de'),
      );

      // The German value, not the abandoned Russian draft and not the old
      // Russian mod value either.
      await waitFor(() =>
        expect((screen.getByTestId('l10n-key-input') as HTMLInputElement).value).toBe('Deutsch'),
      );
    });

    it('does not mark an untouched row as dirty purely from a language switch', async () => {
      mockKeysPerLang({
        ru_ru: [keyRow({ key: 'a', sourceEn: 'A', modValue: 'Русский', state: 'from_mod' })],
        de_de: [keyRow({ key: 'a', sourceEn: 'A', modValue: 'Deutsch', state: 'from_mod' })],
      });
      const { rerender } = render(KeyTable, { props });
      await waitFor(() =>
        expect((screen.getByTestId('l10n-key-input') as HTMLInputElement).value).toBe('Русский'),
      );
      // The user never touches the input before switching languages.

      await rerender({ ...props, lang: 'de_de' });
      await waitFor(() =>
        expect((screen.getByTestId('l10n-key-input') as HTMLInputElement).value).toBe('Deutsch'),
      );
      // A freshly (re)created row seeds `draft` from the new row, so it must
      // not read as dirty just because the underlying value changed languages.
      expect((screen.getByTestId('l10n-key-save') as HTMLButtonElement).disabled).toBe(true);
    });

    it('recreates the row instance on a language switch, dropping stale per-row state like a rejection message', async () => {
      mockKeysPerLang({
        ru_ru: [keyRow({ key: 'a', sourceEn: 'A', state: 'missing' })],
        de_de: [keyRow({ key: 'a', sourceEn: 'A', state: 'missing' })],
      });
      vi.mocked(commands.l10nSetOverride).mockResolvedValue({
        status: 'error',
        error: {
          kind: 'l10n_translation_invalid',
          key: 'a',
          reason: { kind: 'unsupported_specifier', specifier: 'd' },
        },
        // biome-ignore lint/suspicious/noExplicitAny: mocked IPC envelope
      } as any);
      const { rerender } = render(KeyTable, { props });
      await screen.findAllByTestId('l10n-key-row');

      await fireEvent.input(screen.getByTestId('l10n-key-input'), { target: { value: '%d bad' } });
      await fireEvent.click(screen.getByTestId('l10n-key-save'));
      expect(await screen.findByTestId('l10n-key-error')).toBeTruthy();
      const rowBefore = screen.getByTestId('l10n-key-row');

      await rerender({ ...props, lang: 'de_de' });
      await waitFor(() =>
        expect(commands.l10nNamespaceKeys).toHaveBeenCalledWith('inst-1', 'create', 'de_de'),
      );

      // A component that only re-synced `draft` (option (b), by hand) could
      // still be carrying the old rejection message; a genuinely recreated
      // instance cannot, because `error` starts fresh at `null`.
      await waitFor(() => expect(screen.queryByTestId('l10n-key-error')).toBeNull());

      // Pin the actual mechanism, not just its symptom: the DOM node itself
      // must be a different element, proving real destroy+recreate rather
      // than an in-place prop patch. This is what makes the each-block key a
      // no-op today (KeyTable's own loading-gate already guarantees this) —
      // if this assertion ever starts failing, the each-key stops being
      // redundant defense-in-depth and starts being load-bearing.
      const rowAfter = screen.getByTestId('l10n-key-row');
      expect(rowAfter).not.toBe(rowBefore);
    });
  });

  // These tests bypass KeyTable entirely and mount KeyEditRow directly,
  // reusing the SAME component instance across a (row, lang) change via
  // `rerender` — i.e. exactly what KeyTable's each-block would do if it were
  // ever keyed by `row.key` alone. This is the only way to observe the
  // hazard the CONTRACT comment at the top of KeyEditRow.svelte describes:
  // through KeyTable's real render path, the loading-gate already tears the
  // row down on every fetch (see the "language switch" group above), so the
  // each-key's own contribution is invisible there. It is NOT invisible here.
  describe('KeyEditRow direct-reuse contract (documents why KeyTable must key by namespace+lang+key)', () => {
    it('shows stale text from the previous (row, lang) when the SAME instance is reused, proving the each-key is load-bearing', async () => {
      const onSaved = vi.fn();
      const { rerender } = render(KeyEditRow, {
        props: {
          row: keyRow({ key: 'a', sourceEn: 'A', modValue: 'Русский', state: 'from_mod' }),
          namespace: 'create',
          lang: 'ru_ru',
          onSaved,
        },
      });
      expect((screen.getByTestId('l10n-key-input') as HTMLInputElement).value).toBe('Русский');

      // Reuse the identical instance with a new row + lang — this is the
      // reuse KeyTable's (namespace, lang, key) each-key exists to prevent.
      await rerender({
        row: keyRow({ key: 'a', sourceEn: 'A', modValue: 'Deutsch', state: 'from_mod' }),
        namespace: 'create',
        lang: 'de_de',
        onSaved,
      });

      // KeyEditRow does NOT self-protect: seeding is its parent's job. A
      // reused instance keeps showing the stale Russian text instead of the
      // new German value.
      expect((screen.getByTestId('l10n-key-input') as HTMLInputElement).value).toBe('Русский');
    });
  });
});

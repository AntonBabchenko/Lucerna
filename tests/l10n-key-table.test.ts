import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { KeyRow } from '$lib/ipc/bindings';

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    l10nNamespaceKeys: vi.fn(),
    l10nSetOverride: vi.fn(),
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
    expect(await screen.findByTestId('l10n-key-table-error')).toBeTruthy();
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
    await waitFor(() => expect(screen.getByTestId('l10n-key-state').textContent).toBe('Saved'));
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
    expect(screen.getByTestId('l10n-key-state').textContent).not.toBe('Saved');
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
      expect(screen.getByTestId('l10n-key-state').textContent).toBe('Untranslated'),
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

    expect(screen.getByTestId('l10n-key-state').textContent).toBe('Orphaned');
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

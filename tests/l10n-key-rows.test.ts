import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { KeyRow } from '$lib/ipc/bindings';
import {
  countKeyStates,
  countOrigins,
  displayValue,
  filterByOrigin,
  filterRows,
  stickyOutOfView,
  viewCount,
  visibleOriginFilters,
  visibleStateFilters,
  visibleViews,
} from '$lib/l10n/key-rows';

// key-rows.ts itself has no IPC — this mock exists only for the KeyEditRow
// round-trip at the bottom of the file, which defends the other half of the
// same invariant `filterByOrigin` reads (see its own comment).
vi.mock('$lib/ipc/bindings', () => ({
  commands: { l10nSetOverride: vi.fn() },
}));

import { commands } from '$lib/ipc/bindings';
import KeyEditRow from '$lib/l10n/KeyEditRow.svelte';

function row(over: Partial<KeyRow> = {}): KeyRow {
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

describe('displayValue', () => {
  it('prefers the override over the mod value', () => {
    const r = row({ modValue: 'Гаечный ключ', overrideValue: 'Мой ключ', state: 'ok' });
    expect(displayValue(r)).toBe('Мой ключ');
  });

  it('falls back to the mod value when there is no override', () => {
    const r = row({ modValue: 'Гаечный ключ', overrideValue: null, state: 'from_mod' });
    expect(displayValue(r)).toBe('Гаечный ключ');
  });

  it('is empty when neither the mod nor the user has a translation', () => {
    const r = row({ modValue: null, overrideValue: null, state: 'missing' });
    expect(displayValue(r)).toBe('');
  });

  // Orphan: the mod dropped the key, but the user's override is still what
  // would show if it mattered — displayValue doesn't editorialise about
  // whether the value is reachable in game, it just says what's stored.
  it('shows the override for an orphaned key', () => {
    const r = row({ modValue: null, overrideValue: 'Стоп-кран', state: 'orphan' });
    expect(displayValue(r)).toBe('Стоп-кран');
  });
});

describe('filterRows', () => {
  const rows: KeyRow[] = [
    row({ key: 'gui.create.title', sourceEn: 'Create', state: 'from_mod', modValue: 'Крафт' }),
    row({
      key: 'gui.create.wrench',
      sourceEn: 'Wrench',
      state: 'ok',
      overrideValue: 'Ключ',
    }),
    row({
      key: 'gui.create.saw',
      sourceEn: 'Saw',
      state: 'stale',
      overrideValue: 'Пила старая',
      modValue: 'Saw (new)',
    }),
    row({
      key: 'gui.create.ghost',
      sourceEn: 'Ghost item',
      state: 'orphan',
      overrideValue: 'Призрак',
    }),
    row({ key: 'gui.create.untranslated', sourceEn: 'Untranslated thing', state: 'missing' }),
  ];

  it('returns every row for "all" with no search', () => {
    expect(filterRows(rows, '', 'all')).toHaveLength(5);
  });

  it('"translated" includes both ok and from_mod, nothing else', () => {
    const result = filterRows(rows, '', 'translated');
    expect(result.map((r) => r.key).sort()).toEqual(['gui.create.title', 'gui.create.wrench']);
  });

  it('"missing" isolates untranslated keys', () => {
    const result = filterRows(rows, '', 'missing');
    expect(result.map((r) => r.key)).toEqual(['gui.create.untranslated']);
  });

  it('"stale" isolates overrides whose English changed', () => {
    const result = filterRows(rows, '', 'stale');
    expect(result.map((r) => r.key)).toEqual(['gui.create.saw']);
  });

  it('"orphan" isolates overrides for keys the mod no longer ships', () => {
    const result = filterRows(rows, '', 'orphan');
    expect(result.map((r) => r.key)).toEqual(['gui.create.ghost']);
  });

  it('matches the search text against the key', () => {
    const result = filterRows(rows, 'wrench', 'all');
    expect(result.map((r) => r.key)).toEqual(['gui.create.wrench']);
  });

  it('matches the search text against the English source, not just the key', () => {
    const result = filterRows(rows, 'ghost item', 'all');
    expect(result.map((r) => r.key)).toEqual(['gui.create.ghost']);
  });

  it('does not match against the translated value — the user may not be able to read it yet', () => {
    // "Ключ" (Russian for "wrench") is the override value, not the key or the
    // English source; searching for it must not surface the row, otherwise a
    // user who can't read Cyrillic could never find a row by typing English.
    const result = filterRows(rows, 'ключ', 'all');
    expect(result).toHaveLength(0);
  });

  it('is case-insensitive', () => {
    const result = filterRows(rows, 'WRENCH', 'all');
    expect(result.map((r) => r.key)).toEqual(['gui.create.wrench']);
  });

  it('trims whitespace from the search term', () => {
    const result = filterRows(rows, '  wrench  ', 'all');
    expect(result.map((r) => r.key)).toEqual(['gui.create.wrench']);
  });

  it('combines an active filter with a search term', () => {
    const result = filterRows(rows, 'create', 'orphan');
    expect(result.map((r) => r.key)).toEqual(['gui.create.ghost']);
  });

  it('returns nothing when the search matches no row under the active filter', () => {
    const result = filterRows(rows, 'wrench', 'orphan');
    expect(result).toHaveLength(0);
  });
});

describe('countKeyStates', () => {
  it('buckets every row into exactly one state, plus a running "all" total', () => {
    const rows: KeyRow[] = [
      row({ key: 'a', state: 'from_mod' }),
      row({ key: 'b', state: 'ok' }),
      row({ key: 'c', state: 'stale' }),
      row({ key: 'd', state: 'orphan' }),
      row({ key: 'e', state: 'missing' }),
      row({ key: 'f', state: 'missing' }),
    ];
    expect(countKeyStates(rows)).toEqual({
      all: 6,
      translated: 2, // from_mod + ok
      stale: 1,
      orphan: 1,
      missing: 2,
    });
  });

  it('is all-zero for an empty row set', () => {
    expect(countKeyStates([])).toEqual({ all: 0, translated: 0, stale: 0, orphan: 0, missing: 0 });
  });
});

describe('filterByOrigin', () => {
  const rows: KeyRow[] = [
    row({ key: 'a', state: 'ok', overrideValue: 'А', origin: 'manual' }),
    row({ key: 'b', state: 'ok', overrideValue: 'Б', origin: 'machine' }),
    row({ key: 'c', state: 'ok', overrideValue: 'В', origin: 'machine' }),
    // No override at all: nothing to attribute, so it belongs to neither
    // bucket. Origin is a SECOND axis over the state filter, not a
    // partition of every row.
    row({ key: 'd', state: 'missing', origin: null }),
  ];

  it('filters rows by origin', () => {
    expect(filterByOrigin(rows, 'machine').map((r) => r.key)).toEqual(['b', 'c']);
    expect(filterByOrigin(rows, 'manual').map((r) => r.key)).toEqual(['a']);
  });

  it('"all" is identity — it does not silently drop unoverridden keys', () => {
    expect(filterByOrigin(rows, 'all')).toHaveLength(4);
  });

  it('counts each origin, so bulk revert can say how many it would drop', () => {
    expect(countOrigins(rows)).toEqual({ manual: 1, machine: 2 });
    expect(countOrigins([])).toEqual({ manual: 0, machine: 0 });
  });
});

describe('chip visibility', () => {
  const anchors = ['all', 'translated', 'missing'];

  it('always keeps the anchor state chips, even at zero', () => {
    const visible = visibleStateFilters({ all: 0, translated: 0, stale: 0, orphan: 0, missing: 0 });
    expect(visible).toEqual(anchors);
  });

  it('adds an attention chip only when it has something to show', () => {
    const visible = visibleStateFilters({ all: 3, translated: 1, stale: 2, orphan: 0, missing: 0 });
    expect(visible).toEqual([...anchors, 'stale']);
  });

  it('keeps the origin anchor and drops empty origin buckets', () => {
    expect(visibleOriginFilters({ manual: 0, machine: 4 })).toEqual(['all', 'machine']);
    expect(visibleOriginFilters({ manual: 0, machine: 0 })).toEqual(['all']);
  });
});

// The other half of the origin invariant. `filterByOrigin` and the bulk-revert
// action are only safe because a hand-edited machine string stops being a
// machine string: KeyEditRow's optimistic patch spreads `...row`, so without an
// explicit `origin` the marker would survive the edit and the user's own words
// would be wiped by the next bulk revert.
describe('KeyEditRow origin round-trip', () => {
  afterEach(() => {
    vi.clearAllMocks();
  });

  it('editing a machine row turns it manual', async () => {
    vi.mocked(commands.l10nSetOverride).mockResolvedValue({
      status: 'ok',
      data: null,
      // biome-ignore lint/suspicious/noExplicitAny: mocked IPC envelope
    } as any);
    const onSaved = vi.fn();
    render(KeyEditRow, {
      props: {
        row: row({
          key: 'a',
          sourceEn: 'A',
          state: 'ok',
          overrideValue: 'Машина',
          origin: 'machine',
        }),
        namespace: 'create',
        lang: 'ru_ru',
        onSaved,
      },
    });

    await fireEvent.input(screen.getByTestId('l10n-key-input'), { target: { value: 'Моё' } });
    await fireEvent.click(screen.getByTestId('l10n-key-save'));

    await waitFor(() => expect(onSaved).toHaveBeenCalledTimes(1));
    expect(onSaved.mock.calls[0][0]).toMatchObject({ overrideValue: 'Моё', origin: 'manual' });
  });

  it('clearing an override drops its origin, so no override never claims one', async () => {
    vi.mocked(commands.l10nSetOverride).mockResolvedValue({
      status: 'ok',
      data: null,
      // biome-ignore lint/suspicious/noExplicitAny: mocked IPC envelope
    } as any);
    const onSaved = vi.fn();
    render(KeyEditRow, {
      props: {
        row: row({
          key: 'a',
          sourceEn: 'A',
          state: 'ok',
          overrideValue: 'Машина',
          origin: 'machine',
        }),
        namespace: 'create',
        lang: 'ru_ru',
        onSaved,
      },
    });

    await fireEvent.click(screen.getByTestId('l10n-key-clear'));

    await waitFor(() => expect(onSaved).toHaveBeenCalledTimes(1));
    expect(onSaved.mock.calls[0][0]).toMatchObject({ overrideValue: null, origin: null });
  });

  // `state_of` returns Orphan for a key that has vanished from the mod's
  // English map BEFORE it ever compares source_en — see
  // `state_is_orphan_when_the_key_vanished_from_english` in
  // src-tauri/src/l10n/store.rs. So a save cannot turn an orphan into 'ok';
  // claiming it does flips the row into a state the next refetch contradicts,
  // and under the "Removed" view it would leave the view on a false premise.
  it('saving an orphan keeps it orphan — the mod still does not ship the key', async () => {
    vi.mocked(commands.l10nSetOverride).mockResolvedValue({
      status: 'ok',
      data: null,
      // biome-ignore lint/suspicious/noExplicitAny: mocked IPC envelope
    } as any);
    const onSaved = vi.fn();
    render(KeyEditRow, {
      props: {
        row: row({
          key: 'gone',
          sourceEn: 'Old English',
          state: 'orphan',
          overrideValue: 'Старое',
        }),
        namespace: 'create',
        lang: 'ru_ru',
        onSaved,
      },
    });

    await fireEvent.input(screen.getByTestId('l10n-key-input'), { target: { value: 'Новое' } });
    await fireEvent.click(screen.getByTestId('l10n-key-save'));

    await waitFor(() => expect(onSaved).toHaveBeenCalledTimes(1));
    expect(onSaved.mock.calls[0][0]).toMatchObject({ state: 'orphan', overrideValue: 'Новое' });
  });
});

describe('one filter axis', () => {
  const rows: KeyRow[] = [
    row({ key: 'a', sourceEn: 'A', state: 'ok', overrideValue: 'А', origin: 'manual' }),
    row({ key: 'b', sourceEn: 'B', state: 'ok', overrideValue: 'Б', origin: 'machine' }),
    row({
      key: 'c',
      sourceEn: 'C',
      state: 'stale',
      overrideValue: 'В',
      modValue: 'C2',
      origin: 'machine',
    }),
    row({ key: 'd', sourceEn: 'D', state: 'missing' }),
    row({ key: 'e', sourceEn: 'E', state: 'from_mod', modValue: 'Е' }),
  ];

  // The whole point of the collapse: "AI" is a VIEW over every row the AI
  // wrote, not a narrowing of whatever state chip happens to be active. As a
  // second axis it could only ever intersect ok/stale/orphan, because origin
  // is written alongside an override and nowhere else.
  it('"machine" shows every AI-written row regardless of state', () => {
    expect(filterRows(rows, '', 'machine').map((r) => r.key)).toEqual(['b', 'c']);
  });

  it('"manual" shows every hand-written row regardless of state', () => {
    expect(filterRows(rows, '', 'manual').map((r) => r.key)).toEqual(['a']);
  });

  it('an origin view still obeys the search box', () => {
    expect(filterRows(rows, 'c', 'machine').map((r) => r.key)).toEqual(['c']);
  });

  it('a sticky key survives a view it no longer matches', () => {
    const sticky = new Set(['d']);
    // 'd' is missing, so "translated" excludes it — but the user just changed
    // it, so it keeps its place in the list.
    expect(filterRows(rows, '', 'translated', sticky).map((r) => r.key)).toEqual([
      'a',
      'b',
      'd',
      'e',
    ]);
  });

  it('a sticky key is exempt from the view but never from the search', () => {
    const sticky = new Set(['d']);
    expect(filterRows(rows, 'a', 'translated', sticky).map((r) => r.key)).toEqual(['a']);
  });

  it('preserves row order rather than moving a sticky row to the end', () => {
    const sticky = new Set(['a']);
    expect(filterRows(rows, '', 'missing', sticky).map((r) => r.key)).toEqual(['a', 'd']);
  });

  it('counts only the sticky rows that no longer match the view', () => {
    expect(stickyOutOfView(rows, '', 'translated', new Set(['d']))).toBe(1);
    // 'a' is translated, so it is in the view on its own merits — nothing to
    // refresh away.
    expect(stickyOutOfView(rows, '', 'translated', new Set(['a']))).toBe(0);
    expect(stickyOutOfView(rows, '', 'all', new Set(['a', 'd']))).toBe(0);
    expect(stickyOutOfView(rows, '', 'translated', new Set())).toBe(0);
  });

  // The count must equal what a click on the refresh affordance would remove.
  // A sticky row the search has already excluded is not on screen, so counting
  // it would promise a row the user cannot see.
  it('does not count a sticky row the search has already excluded', () => {
    const sticky = new Set(['d']);
    expect(filterRows(rows, 'zzz', 'translated', sticky)).toHaveLength(0);
    expect(stickyOutOfView(rows, 'zzz', 'translated', sticky)).toBe(0);
    // Same row, a search it does match: back on screen, and counted.
    expect(filterRows(rows, 'd', 'translated', sticky).map((r) => r.key)).toEqual(['d']);
    expect(stickyOutOfView(rows, 'd', 'translated', sticky)).toBe(1);
  });

  it('reads each view’s count from the bucket that owns it', () => {
    const counts = { all: 5, translated: 3, stale: 1, orphan: 0, missing: 1 };
    const origins = { manual: 1, machine: 2 };
    expect(viewCount('all', counts, origins)).toBe(5);
    expect(viewCount('translated', counts, origins)).toBe(3);
    expect(viewCount('missing', counts, origins)).toBe(1);
    expect(viewCount('stale', counts, origins)).toBe(1);
    expect(viewCount('orphan', counts, origins)).toBe(0);
    expect(viewCount('manual', counts, origins)).toBe(1);
    expect(viewCount('machine', counts, origins)).toBe(2);
  });

  it('always renders the anchor views and drops the empty ones', () => {
    const visible = visibleViews(
      { all: 0, translated: 0, stale: 0, orphan: 0, missing: 0 },
      { manual: 0, machine: 0 },
    );
    expect(visible).toEqual(['all', 'translated', 'missing']);
  });

  it('adds a view once it has something to show, in canonical order', () => {
    const visible = visibleViews(
      { all: 6, translated: 3, stale: 2, orphan: 0, missing: 1 },
      { manual: 1, machine: 2 },
    );
    expect(visible).toEqual(['all', 'translated', 'missing', 'stale', 'manual', 'machine']);
  });

  // Without this, saving the last "needs review" key drops its count to zero,
  // the chip vanishes, and KeyTable's fallback effect forces the view back to
  // "all" — flooding the table with the whole mod on the very action the
  // sticky set exists to make undisruptive.
  it('keeps the view you are standing in while it still holds sticky rows', () => {
    const visible = visibleViews(
      { all: 6, translated: 4, stale: 0, orphan: 0, missing: 2 },
      { manual: 1, machine: 0 },
      'stale',
    );
    expect(visible).toEqual(['all', 'translated', 'missing', 'stale', 'manual']);
  });

  it('does not duplicate a kept view that is already visible', () => {
    const visible = visibleViews(
      { all: 6, translated: 4, stale: 1, orphan: 0, missing: 2 },
      { manual: 0, machine: 0 },
      'stale',
    );
    expect(visible).toEqual(['all', 'translated', 'missing', 'stale']);
  });
});

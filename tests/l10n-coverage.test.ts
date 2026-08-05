import { describe, expect, it } from 'vitest';
import type { NamespaceCoverage } from '$lib/ipc/bindings';
import {
  coverageTone,
  NAMESPACE_SORTS,
  namespacePercent,
  sortNamespaces,
} from '$lib/l10n/coverage';

const ns = (over: Partial<NamespaceCoverage>): NamespaceCoverage => ({
  namespace: 'x',
  totalKeys: 10,
  fromMod: 0,
  overridden: 0,
  ...over,
});

describe('namespacePercent', () => {
  it('counts mod translations and user overrides together', () => {
    expect(namespacePercent(ns({ totalKeys: 10, fromMod: 3, overridden: 2 }))).toBe(50);
  });

  it('treats a namespace with nothing to translate as complete', () => {
    // Not 0% — there is no work here, and 0 would read as a problem.
    expect(namespacePercent(ns({ totalKeys: 0 }))).toBe(100);
  });

  it('truncates rather than rounding up', () => {
    // 2/3 must read 66, never 67 — and 999/1000 must never read 100,
    // because "100%" has to mean actually finished.
    expect(namespacePercent(ns({ totalKeys: 3, fromMod: 2 }))).toBe(66);
    expect(namespacePercent(ns({ totalKeys: 1000, fromMod: 999 }))).toBe(99);
  });
});

describe('coverageTone', () => {
  it('routes a fully covered namespace to the success tone', () => {
    expect(coverageTone(100)).toBe('ok');
  });

  it('routes a partially covered namespace to the warning tone', () => {
    expect(coverageTone(50)).toBe('partial');
    expect(coverageTone(99)).toBe('partial');
  });

  it('routes an untranslated namespace to its own muted tone', () => {
    // Zero is not a warning — nothing is wrong, it is simply not translated.
    expect(coverageTone(0)).toBe('none');
  });
});

describe('sortNamespaces', () => {
  it('puts the least translated first so the work to do is on top', () => {
    const rows = [
      ns({ namespace: 'thermal', totalKeys: 10, fromMod: 10 }),
      ns({ namespace: 'create', totalKeys: 100, fromMod: 20 }),
      ns({ namespace: 'ae2', totalKeys: 50, fromMod: 0 }),
    ];
    expect(sortNamespaces(rows).map((r) => r.namespace)).toEqual(['ae2', 'create', 'thermal']);
  });

  it('breaks ties by key count, largest first', () => {
    const rows = [
      ns({ namespace: 'small', totalKeys: 10, fromMod: 0 }),
      ns({ namespace: 'big', totalKeys: 900, fromMod: 0 }),
    ];
    expect(sortNamespaces(rows).map((r) => r.namespace)).toEqual(['big', 'small']);
  });

  it('does not mutate its input', () => {
    const rows = [ns({ namespace: 'b', fromMod: 10 }), ns({ namespace: 'a', fromMod: 0 })];
    const before = rows.map((r) => r.namespace);
    sortNamespaces(rows);
    expect(rows.map((r) => r.namespace)).toEqual(before);
  });
});

describe('sortNamespaces orders', () => {
  // The mod's authors translated `heavy` almost fully; the user has barely
  // touched it. The user did all the work on `mine`. If "by my translations"
  // agrees with the default here, the comparator is reading the wrong field.
  const heavy = { namespace: 'heavy', totalKeys: 100, fromMod: 90, overridden: 1 };
  const mine = { namespace: 'mine', totalKeys: 100, fromMod: 0, overridden: 40 };

  it('puts the least covered first by default', () => {
    expect(sortNamespaces([heavy, mine]).map((r) => r.namespace)).toEqual(['mine', 'heavy']);
  });

  it('disagrees with the default when ordering by the user own translations', () => {
    expect(sortNamespaces([heavy, mine], 'mine').map((r) => r.namespace)).toEqual([
      'mine',
      'heavy',
    ]);
    expect(sortNamespaces([mine, heavy], 'mine')[0].overridden).toBe(40);
    expect(sortNamespaces([mine, heavy], 'mostCovered')[0].namespace).toBe('heavy');
  });

  it('sorts by name and by size', () => {
    expect(sortNamespaces([mine, heavy], 'name').map((r) => r.namespace)).toEqual([
      'heavy',
      'mine',
    ]);
    const big = { namespace: 'big', totalKeys: 500, fromMod: 0, overridden: 0 };
    expect(sortNamespaces([mine, big], 'mostKeys')[0].namespace).toBe('big');
  });

  it('offers every order exactly once', () => {
    expect(new Set(NAMESPACE_SORTS).size).toBe(NAMESPACE_SORTS.length);
  });
});

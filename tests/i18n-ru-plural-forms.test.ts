import { get } from 'svelte/store';
import { afterEach, describe, expect, it } from 'vitest';
import { locale, t } from '$lib/i18n';

// Russian needs four ICU plural categories where English needs two, so an
// en/ru pair can be in parity by key AND by placeholder name — all that
// tests/i18n-parity.test.ts can check — while still reading as broken Russian.
// This file pins the actual agreement for keys where that has bitten.
//
// Out of scope here: whether call sites hand these keys a NUMBER rather than a
// pre-formatted string (the "не число" failure). That invariant is enforced
// repo-wide by tests/i18n-plural-args.test.ts; this key's call site,
// src/lib/ops/op-queue.svelte.ts, passes `report.problems.length`.

afterEach(() => locale.set('en'));

const REPAIR_PARTIAL_FORMS: ReadonlyArray<[count: number, noun: string]> = [
  [1, 'файл'],
  [2, 'файла'],
  [3, 'файла'],
  [4, 'файла'],
  [5, 'файлов'],
  [11, 'файлов'], // 11-14 take the genitive plural despite ending in 1-4
  [21, 'файл'],
  [0, 'файлов'],
];

describe('instance.integrity.toastRepairPartial', () => {
  it.each(REPAIR_PARTIAL_FORMS)('agrees in Russian: "%i %s"', (count, noun) => {
    locale.set('ru');
    const text = get(t)('instance.integrity.toastRepairPartial', { name: 'Инстанс', count });
    expect(text).toBe(`Инстанс: ${count} ${noun} не удалось восстановить`);
  });

  it('leaves the English singular/plural pair unchanged', () => {
    locale.set('en');
    const tr = get(t);
    expect(tr('instance.integrity.toastRepairPartial', { name: 'Inst', count: 1 })).toBe(
      'Inst: 1 file could not be repaired',
    );
    expect(tr('instance.integrity.toastRepairPartial', { name: 'Inst', count: 2 })).toBe(
      'Inst: 2 files could not be repaired',
    );
  });
});

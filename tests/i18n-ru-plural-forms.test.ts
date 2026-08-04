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

// OperationsBar (src/lib/tasks/OperationsBar.svelte) shows "N operations"
// when several tasks are in flight. "операция" is a feminine noun with its
// own one/few/many split, distinct from the masculine "файл" pattern above
// (few/many share the "-и"/"-й" endings) — pinned separately so a future
// edit to one key's plural forms can't accidentally pass by coincidence.
const STRIP_COUNT_FORMS: ReadonlyArray<[count: number, noun: string]> = [
  [1, 'операция'],
  [2, 'операции'],
  [3, 'операции'],
  [4, 'операции'],
  [5, 'операций'],
  [11, 'операций'], // 11-14 take the genitive plural despite ending in 1-4
  [21, 'операция'],
  [0, 'операций'],
];

describe('tasks.strip.count', () => {
  it.each(STRIP_COUNT_FORMS)('agrees in Russian: "%i %s"', (count, noun) => {
    locale.set('ru');
    const text = get(t)('tasks.strip.count', { count });
    expect(text).toBe(`${count} ${noun}`);
  });

  it('leaves the English singular/plural pair unchanged', () => {
    locale.set('en');
    const tr = get(t);
    expect(tr('tasks.strip.count', { count: 1 })).toBe('1 operation');
    expect(tr('tasks.strip.count', { count: 2 })).toBe('2 operations');
  });
});

// OperationsBar's finished-only state ("N operations finished") pairs the
// same "операция" noun with a short passive participle ("завершена" /
// "завершены" / "завершено") that agrees with it independently — a second
// word whose ending could drift out of sync with the noun's own plural
// bucket without its own pin.
const STRIP_FINISHED_SUMMARY_FORMS: ReadonlyArray<[count: number, phrase: string]> = [
  [1, 'операция завершена'],
  [2, 'операции завершены'],
  [3, 'операции завершены'],
  [4, 'операции завершены'],
  [5, 'операций завершено'],
  [11, 'операций завершено'], // 11-14 take the genitive plural despite ending in 1-4
  [21, 'операция завершена'],
  [0, 'операций завершено'],
];

describe('tasks.strip.finishedSummary', () => {
  it.each(STRIP_FINISHED_SUMMARY_FORMS)('agrees in Russian: "%i %s"', (count, phrase) => {
    locale.set('ru');
    const text = get(t)('tasks.strip.finishedSummary', { count });
    expect(text).toBe(`${count} ${phrase}`);
  });

  it('leaves the English singular/plural pair unchanged', () => {
    locale.set('en');
    const tr = get(t);
    expect(tr('tasks.strip.finishedSummary', { count: 1 })).toBe('1 operation finished');
    expect(tr('tasks.strip.finishedSummary', { count: 2 })).toBe('2 operations finished');
  });
});

// TaskReportModal (src/lib/tasks/TaskReportModal.svelte): the "All" filter
// chip and the header totals line both share this key, and share the same
// "файл" agreement STRIP_COUNT_FORMS's sibling REPAIR_PARTIAL_FORMS already
// pinned above — spelled out again here (not reused) because a future editor
// touching either key's forms independently should not silently desync the
// other.
const REPORT_FILES_COUNT_FORMS: ReadonlyArray<[count: number, noun: string]> = [
  [1, 'файл'],
  [2, 'файла'],
  [3, 'файла'],
  [4, 'файла'],
  [5, 'файлов'],
  [11, 'файлов'], // 11-14 take the genitive plural despite ending in 1-4
  [21, 'файл'],
  [0, 'файлов'],
];

describe('tasks.report.filesCount', () => {
  it.each(REPORT_FILES_COUNT_FORMS)('agrees in Russian: "%i %s"', (count, noun) => {
    locale.set('ru');
    const text = get(t)('tasks.report.filesCount', { count });
    expect(text).toBe(`${count} ${noun}`);
  });

  it('leaves the English singular/plural pair unchanged', () => {
    locale.set('en');
    const tr = get(t);
    expect(tr('tasks.report.filesCount', { count: 1 })).toBe('1 file');
    expect(tr('tasks.report.filesCount', { count: 2 })).toBe('2 files');
  });
});

// The four outcome filter chips ("2 skipped", "265 installed", …) are
// deliberately worded as invariant short-form participles / fixed phrases
// ("установлено", "без изменений", "пропущено", "не удалось") rather than
// re-triggering the "файл" noun agreement above — Russian does not inflect
// these regardless of the count they're attached to. Pinned explicitly (not
// assumed) so a future edit that "fixes" one into a plural block would fail
// here instead of shipping subtly wrong grammar.
const REPORT_FILTER_COUNTS = [1, 2, 3, 4, 5, 11, 21, 0];

describe('tasks.report.filter.installed', () => {
  it.each(REPORT_FILTER_COUNTS)('stays invariant in Russian at count %i', (count) => {
    locale.set('ru');
    expect(get(t)('tasks.report.filter.installed', { count })).toBe(`${count} установлено`);
  });

  it('leaves the English phrasing unchanged', () => {
    locale.set('en');
    const tr = get(t);
    expect(tr('tasks.report.filter.installed', { count: 1 })).toBe('1 installed');
    expect(tr('tasks.report.filter.installed', { count: 2 })).toBe('2 installed');
  });
});

describe('tasks.report.filter.unchanged', () => {
  it.each(REPORT_FILTER_COUNTS)('stays invariant in Russian at count %i', (count) => {
    locale.set('ru');
    expect(get(t)('tasks.report.filter.unchanged', { count })).toBe(`${count} без изменений`);
  });

  it('leaves the English phrasing unchanged', () => {
    locale.set('en');
    const tr = get(t);
    expect(tr('tasks.report.filter.unchanged', { count: 1 })).toBe('1 unchanged');
    expect(tr('tasks.report.filter.unchanged', { count: 2 })).toBe('2 unchanged');
  });
});

describe('tasks.report.filter.skipped', () => {
  it.each(REPORT_FILTER_COUNTS)('stays invariant in Russian at count %i', (count) => {
    locale.set('ru');
    expect(get(t)('tasks.report.filter.skipped', { count })).toBe(`${count} пропущено`);
  });

  it('leaves the English phrasing unchanged', () => {
    locale.set('en');
    const tr = get(t);
    expect(tr('tasks.report.filter.skipped', { count: 2 })).toBe('2 skipped');
  });
});

describe('tasks.report.filter.failed', () => {
  it.each(REPORT_FILTER_COUNTS)('stays invariant in Russian at count %i', (count) => {
    locale.set('ru');
    expect(get(t)('tasks.report.filter.failed', { count })).toBe(`${count} не удалось`);
  });

  it('leaves the English phrasing unchanged', () => {
    locale.set('en');
    const tr = get(t);
    expect(tr('tasks.report.filter.failed', { count: 5 })).toBe('5 failed');
  });
});

import { describe, expect, it } from 'vitest';
import type { ContentAction, JournalEntry, LaunchOutcome } from '$lib/ipc/bindings';
import {
  ACTION_COPY,
  contentChangesBefore,
  groupByDay,
  journalFilter,
  OUTCOME_COPY,
} from '$lib/journal/journal-view';
import en from '../src/lib/i18n/locales/en.json';
import ru from '../src/lib/i18n/locales/ru.json';

const HOUR = 60 * 60 * 1000;

function change(atMs: number, subject = 'Sodium', action: ContentAction = 'mod_installed') {
  return {
    at_unix_ms: atMs,
    event: {
      kind: 'content',
      action,
      subject,
      from_version: null,
      to_version: null,
      affected: null,
    },
  } satisfies JournalEntry;
}

function launch(atMs: number, outcome: LaunchOutcome = 'crashed') {
  return {
    at_unix_ms: atMs,
    event: { kind: 'launch', outcome, exit_code: -1, duration_seconds: 30, log_path: null },
  } satisfies JournalEntry;
}

/** Walk a dotted key path through a locale object. */
function lookup(obj: unknown, path: string): unknown {
  return path.split('.').reduce<unknown>((acc, part) => {
    if (acc && typeof acc === 'object' && part in acc) {
      return (acc as Record<string, unknown>)[part];
    }
    return undefined;
  }, obj);
}

describe('contentChangesBefore', () => {
  const anchor = 100 * HOUR;

  it('returns content changes inside the window, newest first', () => {
    const entries = [
      change(anchor - 1 * HOUR, 'Recent'),
      change(anchor - 3 * HOUR, 'Older'),
      change(anchor - 2 * HOUR, 'Middle'),
    ];
    const got = contentChangesBefore(entries, anchor, 24 * HOUR, 10);
    expect(got.map((e) => (e.event.kind === 'content' ? e.event.subject : null))).toEqual([
      'Recent',
      'Middle',
      'Older',
    ]);
  });

  it('excludes changes outside the window', () => {
    const entries = [change(anchor - 1 * HOUR, 'In'), change(anchor - 48 * HOUR, 'WayBefore')];
    const got = contentChangesBefore(entries, anchor, 24 * HOUR, 10);
    expect(got).toHaveLength(1);
    expect(got[0].event.kind === 'content' && got[0].event.subject).toBe('In');
  });

  it('excludes changes AFTER the anchor', () => {
    // A mod installed after the crash cannot have caused it. Including it would
    // make the correlation strip actively misleading.
    const entries = [change(anchor + 1 * HOUR, 'After'), change(anchor - 1 * HOUR, 'Before')];
    const got = contentChangesBefore(entries, anchor, 24 * HOUR, 10);
    expect(got).toHaveLength(1);
    expect(got[0].event.kind === 'content' && got[0].event.subject).toBe('Before');
  });

  it('excludes launch entries', () => {
    const entries = [launch(anchor - 1 * HOUR), change(anchor - 2 * HOUR, 'TheMod')];
    const got = contentChangesBefore(entries, anchor, 24 * HOUR, 10);
    expect(got).toHaveLength(1);
    expect(got[0].event.kind).toBe('content');
  });

  it('caps the result at the limit, keeping the newest', () => {
    const entries = [
      change(anchor - 1 * HOUR, 'A'),
      change(anchor - 2 * HOUR, 'B'),
      change(anchor - 3 * HOUR, 'C'),
      change(anchor - 4 * HOUR, 'D'),
    ];
    const got = contentChangesBefore(entries, anchor, 24 * HOUR, 2);
    expect(got.map((e) => (e.event.kind === 'content' ? e.event.subject : null))).toEqual([
      'A',
      'B',
    ]);
  });

  it('includes a change at exactly the anchor and at exactly the window edge', () => {
    const entries = [change(anchor, 'AtAnchor'), change(anchor - 24 * HOUR, 'AtEdge')];
    expect(contentChangesBefore(entries, anchor, 24 * HOUR, 10)).toHaveLength(2);
  });

  it('returns nothing for an empty journal', () => {
    expect(contentChangesBefore([], anchor, 24 * HOUR, 3)).toEqual([]);
  });
});

describe('groupByDay', () => {
  it('buckets entries by local calendar day, newest day first', () => {
    const day2 = new Date(2026, 6, 30, 10, 0, 0).getTime();
    const day2Later = new Date(2026, 6, 30, 18, 0, 0).getTime();
    const day1 = new Date(2026, 6, 29, 23, 30, 0).getTime();
    const groups = groupByDay([change(day2Later, 'C'), change(day2, 'B'), change(day1, 'A')]);
    expect(groups).toHaveLength(2);
    expect(groups[0].entries).toHaveLength(2);
    expect(groups[1].entries).toHaveLength(1);
    // Each group's key is stable and unique — it keys the `{#each}` blocks.
    expect(new Set(groups.map((g) => g.key)).size).toBe(2);
  });

  it('keeps entry order within a day', () => {
    const later = new Date(2026, 6, 30, 18, 0, 0).getTime();
    const earlier = new Date(2026, 6, 30, 9, 0, 0).getTime();
    const groups = groupByDay([change(later, 'Later'), change(earlier, 'Earlier')]);
    expect(
      groups[0].entries.map((e) => (e.event.kind === 'content' ? e.event.subject : null)),
    ).toEqual(['Later', 'Earlier']);
  });

  it('returns no groups for no entries', () => {
    expect(groupByDay([])).toEqual([]);
  });
});

describe('journalFilter', () => {
  const entries = [change(5), launch(4), change(3)];

  it('all keeps everything', () => {
    expect(journalFilter(entries, 'all')).toHaveLength(3);
  });

  it('content keeps only content changes', () => {
    const got = journalFilter(entries, 'content');
    expect(got).toHaveLength(2);
    expect(got.every((e) => e.event.kind === 'content')).toBe(true);
  });

  it('launches keeps only launch attempts', () => {
    const got = journalFilter(entries, 'launches');
    expect(got).toHaveLength(1);
    expect(got[0].event.kind).toBe('launch');
  });
});

describe('copy-map completeness', () => {
  // Every backend enum variant must have a row here, or a journal written by a
  // future build renders as a blank line. The union types come straight from
  // the generated bindings, so adding a Rust variant fails this test.
  const ALL_ACTIONS: ContentAction[] = [
    'mod_installed',
    'mod_updated',
    'mod_removed',
    'mod_enabled',
    'mod_disabled',
    'asset_installed',
    'asset_updated',
    'asset_removed',
    'modpack_imported',
    'modpack_updated',
    'integrity_repaired',
  ];
  const ALL_OUTCOMES: LaunchOutcome[] = ['ok', 'crashed', 'stopped'];

  it('covers every ContentAction', () => {
    expect(Object.keys(ACTION_COPY).sort()).toEqual([...ALL_ACTIONS].sort());
  });

  it('covers every LaunchOutcome', () => {
    expect(Object.keys(OUTCOME_COPY).sort()).toEqual([...ALL_OUTCOMES].sort());
  });

  it('resolves every referenced key in BOTH en and ru', () => {
    const keys = [
      ...ALL_ACTIONS.map((a) => ACTION_COPY[a].labelKey),
      ...ALL_OUTCOMES.map((o) => OUTCOME_COPY[o].labelKey),
    ];
    for (const key of keys) {
      expect(typeof lookup(en, key), `${key} missing in en`).toBe('string');
      expect(typeof lookup(ru, key), `${key} missing in ru`).toBe('string');
    }
  });

  it('has en+ru copy for every static journal string the panel renders', () => {
    const keys = [
      'logs.journal.viewLogs',
      'logs.journal.viewJournal',
      'logs.journal.title',
      'logs.journal.empty',
      'logs.journal.loadFailed',
      'logs.journal.filterAll',
      'logs.journal.filterContent',
      'logs.journal.filterLaunches',
      'logs.journal.clear',
      'logs.journal.clearTitle',
      'logs.journal.clearBody',
      'logs.journal.clearConfirm',
      'logs.journal.cleared',
      'logs.journal.openLog',
      'logs.journal.affected',
      'logs.journal.exitCode',
      'logs.journal.contextTitle',
      'logs.journal.contextAgo',
      'logs.toolbar.collapseRepeats',
      'logs.compact.repeatCount',
    ];
    for (const key of keys) {
      expect(typeof lookup(en, key), `${key} missing in en`).toBe('string');
      expect(typeof lookup(ru, key), `${key} missing in ru`).toBe('string');
    }
  });
});

// src/lib/journal/journal-view.ts
//
// Pure view logic for the instance journal: the single mapping from a backend
// enum to icon / tone / i18n key, day grouping for the list, the filter, and
// the crash-correlation query. No DOM, no IPC — unit-tested in isolation; the
// JournalPanel / JournalCrashContext components render its output.

import type { TranslationKey } from '$lib/i18n/keys.generated';
import type { ContentAction, JournalEntry, LaunchOutcome } from '$lib/ipc/bindings';
import type { IconName } from '$lib/ui/icons';

/** Row tone. Maps to the shared status palette, not to raw colours. */
export type JournalTone = 'neutral' | 'positive' | 'warning' | 'danger';

export interface JournalRowCopy {
  icon: IconName;
  tone: JournalTone;
  labelKey: TranslationKey;
}

/**
 * One row per `ContentAction`. Adding a Rust variant without adding a row here
 * is a test failure, not a blank line in the UI (see journal-view.test.ts).
 */
export const ACTION_COPY: Record<ContentAction, JournalRowCopy> = {
  mod_installed: {
    icon: 'download',
    tone: 'positive',
    labelKey: 'logs.journal.action.modInstalled',
  },
  mod_updated: { icon: 'update', tone: 'neutral', labelKey: 'logs.journal.action.modUpdated' },
  mod_removed: { icon: 'trash', tone: 'danger', labelKey: 'logs.journal.action.modRemoved' },
  mod_enabled: { icon: 'eye', tone: 'positive', labelKey: 'logs.journal.action.modEnabled' },
  mod_disabled: { icon: 'eyeOff', tone: 'warning', labelKey: 'logs.journal.action.modDisabled' },
  asset_installed: {
    icon: 'resourcePack',
    tone: 'positive',
    labelKey: 'logs.journal.action.assetInstalled',
  },
  asset_updated: { icon: 'update', tone: 'neutral', labelKey: 'logs.journal.action.assetUpdated' },
  asset_removed: { icon: 'trash', tone: 'danger', labelKey: 'logs.journal.action.assetRemoved' },
  modpack_imported: {
    icon: 'package',
    tone: 'positive',
    labelKey: 'logs.journal.action.modpackImported',
  },
  modpack_updated: {
    icon: 'update',
    tone: 'neutral',
    labelKey: 'logs.journal.action.modpackUpdated',
  },
  integrity_repaired: {
    icon: 'wrench',
    tone: 'neutral',
    labelKey: 'logs.journal.action.integrityRepaired',
  },
};

export const OUTCOME_COPY: Record<LaunchOutcome, JournalRowCopy> = {
  ok: { icon: 'success', tone: 'positive', labelKey: 'logs.journal.outcome.ok' },
  crashed: { icon: 'circleX', tone: 'danger', labelKey: 'logs.journal.outcome.crashed' },
  stopped: { icon: 'stop', tone: 'neutral', labelKey: 'logs.journal.outcome.stopped' },
};

/** Tailwind text colour per tone — kept beside the tone so rows can't drift. */
export function toneClass(tone: JournalTone): string {
  switch (tone) {
    case 'positive':
      return 'text-success';
    case 'warning':
      return 'text-warning-text';
    case 'danger':
      return 'text-danger';
    default:
      return 'text-muted';
  }
}

/**
 * An entry's timestamp as a plain number.
 *
 * specta exports every Rust `f64` as `number | null` (a non-finite float
 * serialises to `null`), so the nullability is a wire artefact rather than a
 * real state — a recorded entry always has a time. Normalising here keeps the
 * `?? 0` out of every consumer.
 */
export function entryTime(entry: JournalEntry): number {
  return entry.at_unix_ms ?? 0;
}

export type JournalFilterMode = 'all' | 'content' | 'launches';

export function journalFilter(entries: JournalEntry[], mode: JournalFilterMode): JournalEntry[] {
  if (mode === 'all') return entries;
  const want = mode === 'content' ? 'content' : 'launch';
  return entries.filter((e) => e.event.kind === want);
}

export interface JournalDayGroup {
  /** Stable, unique per group — used as the `{#each}` key. */
  key: string;
  /** Midnight (local) of the group's day, for the header's date formatting. */
  dayStartMs: number;
  entries: JournalEntry[];
}

/**
 * Bucket entries into local calendar days, preserving the caller's order
 * (newest-first) both between and within groups.
 */
export function groupByDay(entries: JournalEntry[]): JournalDayGroup[] {
  const groups: JournalDayGroup[] = [];
  let current: JournalDayGroup | null = null;
  for (const entry of entries) {
    const d = new Date(entryTime(entry));
    const dayStartMs = new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
    if (!current || current.dayStartMs !== dayStartMs) {
      current = { key: String(dayStartMs), dayStartMs, entries: [] };
      groups.push(current);
    }
    current.entries.push(entry);
  }
  return groups;
}

/**
 * The crash-correlation query: content changes recorded in the `windowMs`
 * leading up to `anchorMs`, newest first, capped at `limit`.
 *
 * Entries after the anchor are excluded — a mod installed after a crash cannot
 * have caused it, and showing it would make the strip misleading rather than
 * merely noisy. Launch entries are excluded because "you played before you
 * crashed" is not evidence.
 */
export function contentChangesBefore(
  entries: JournalEntry[],
  anchorMs: number,
  windowMs: number,
  limit: number,
): JournalEntry[] {
  return entries
    .filter(
      (e) =>
        e.event.kind === 'content' &&
        entryTime(e) <= anchorMs &&
        anchorMs - entryTime(e) <= windowMs,
    )
    .sort((a, b) => entryTime(b) - entryTime(a))
    .slice(0, Math.max(0, limit));
}

/**
 * The version fragment for a content row: `from → to`, a bare `to`, a bare
 * `from` (a removal knows only what left), or empty. Version strings are data,
 * so this needs no locale.
 */
export function versionLabel(fromVersion: string | null, toVersion: string | null): string {
  if (fromVersion && toVersion) return `${fromVersion} → ${toVersion}`;
  return toVersion ?? fromVersion ?? '';
}

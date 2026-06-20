// Pure helpers for the server console's level-filter + in-log search (#14).
//
// The console renders either a live line array or an archived log blob; both
// reduce to `string[]`. These functions tag each line with a carried-forward
// severity (reusing the client log parser), count/expose the present levels for
// the toggle chips, drop hidden levels, and compute case-insensitive search
// matches + highlight HTML. No DOM dependencies — unit-tested in isolation; the
// component wires the output through Svelte each-blocks.

import { type Severity, type TaggedLine, tagWithSeverity } from '$lib/logs/render';

export type { Severity, TaggedLine };

/** A single case-insensitive search match within the visible (filtered) lines. */
export interface ConsoleMatch {
  /** Index into the filtered line array. */
  line: number;
  /** Match start offset within the line text. */
  start: number;
  /** Match end offset (exclusive). */
  end: number;
}

// Chip display order: most severe first, the catch-all `other` last. Only the
// levels actually present in the output are shown (see `presentLevels`).
const LEVEL_ORDER: Severity[] = ['fatal', 'error', 'warn', 'info', 'debug', 'trace', 'other'];

/** Tag raw console lines with a carried-forward severity (stack lines inherit). */
export function tagConsoleLines(lines: string[]): TaggedLine[] {
  return tagWithSeverity(lines);
}

/** Count how many lines fall under each severity. */
export function countLevels(tagged: TaggedLine[]): Map<Severity, number> {
  const counts = new Map<Severity, number>();
  for (const l of tagged) counts.set(l.level, (counts.get(l.level) ?? 0) + 1);
  return counts;
}

/** The severities present in the output, in fixed display order. */
export function presentLevels(tagged: TaggedLine[]): Severity[] {
  const counts = countLevels(tagged);
  return LEVEL_ORDER.filter((lv) => counts.has(lv));
}

/** Drop lines whose level the user has hidden. Identity when nothing is hidden. */
export function filterLevels(tagged: TaggedLine[], hidden: ReadonlySet<Severity>): TaggedLine[] {
  if (hidden.size === 0) return tagged;
  return tagged.filter((l) => !hidden.has(l.level));
}

/** Every case-insensitive match of `needle` across `lines`. Empty for blank. */
export function findMatches(lines: string[], needle: string): ConsoleMatch[] {
  const matches: ConsoleMatch[] = [];
  if (!needle) return matches;
  const lower = needle.toLowerCase();
  for (let i = 0; i < lines.length; i++) {
    const hay = lines[i].toLowerCase();
    let pos = 0;
    while (pos < hay.length) {
      const idx = hay.indexOf(lower, pos);
      if (idx === -1) break;
      matches.push({ line: i, start: idx, end: idx + lower.length });
      pos = idx + lower.length;
    }
  }
  return matches;
}

/** Map a severity to the shared ToggleChip tone palette. */
export function levelChipTone(lv: Severity): 'danger' | 'warning' | 'muted' | 'neutral' {
  switch (lv) {
    case 'error':
    case 'fatal':
      return 'danger';
    case 'warn':
      return 'warning';
    case 'debug':
    case 'trace':
      return 'muted';
    default:
      return 'neutral';
  }
}

export function escapeHtml(s: string): string {
  return s
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&#39;');
}

/**
 * Render one line to HTML with `<mark>` around each match. The match whose
 * global index equals `activeGlobalIndex` gets `data-match-active="true"` so the
 * view can scroll it into view. Text is HTML-escaped — safe for `{@html}`.
 */
export function highlightConsoleLine(
  text: string,
  lineIndex: number,
  allMatches: ConsoleMatch[],
  activeGlobalIndex: number,
): string {
  const forLine: { m: ConsoleMatch; gi: number }[] = [];
  for (let gi = 0; gi < allMatches.length; gi++) {
    if (allMatches[gi].line === lineIndex) forLine.push({ m: allMatches[gi], gi });
  }
  if (forLine.length === 0) return escapeHtml(text);
  let out = '';
  let pos = 0;
  for (const { m, gi } of forLine) {
    out += escapeHtml(text.slice(pos, m.start));
    const seg = escapeHtml(text.slice(m.start, m.end));
    out +=
      gi === activeGlobalIndex
        ? `<mark class="bg-accent text-white" data-match-active="true">${seg}</mark>`
        : `<mark>${seg}</mark>`;
    pos = m.end;
  }
  out += escapeHtml(text.slice(pos));
  return out;
}

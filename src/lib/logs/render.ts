// src/lib/logs/render.ts
//
// Pure-function pipeline that turns a raw log body into a render
// model: severity-tagged lines, stack-trace folds, and (for crash
// reports) section blocks. No DOM dependencies — unit-tested in
// isolation; the LogsPopover component wires the output through
// Svelte each-blocks.

export type Severity = 'info' | 'warn' | 'error' | 'debug' | 'trace' | 'fatal' | 'other';

export interface TaggedLine {
  text: string;
  level: Severity;
  /** 0-based index of this line in the ORIGINAL text (pre-fold, pre-filter).
   *  Inline hint annotations from the backend are keyed on it. */
  index: number;
}

export type RenderUnit =
  | { kind: 'line'; text: string; level: Severity; index: number }
  | {
      // No index on folds — deliberate: hint badges never attach to folded
      // stack frames (patterns match the exception line, not `at ...` frames),
      // and the first frame of a fold is by definition a stack frame.
      kind: 'fold';
      level: Severity;
      firstFrame: string;
      hiddenFrames: string[];
    }
  | {
      // A run of consecutive lines that say the same thing. Carries the FIRST
      // member's text, level and index — so an inline hint badge still attaches
      // to a repeated error line — plus the remaining raw lines for expansion.
      kind: 'repeat';
      text: string;
      level: Severity;
      index: number;
      /** Total members in the run, including the visible first one. */
      count: number;
      hiddenLines: string[];
    };

export interface CrashSection {
  title: string;
  body: string;
  /** 0-based line index (in the full report) of the section body's first line. */
  startLine: number;
}

const LEVEL_RE = /\[[^/\]]+\/(INFO|WARN|ERROR|DEBUG|TRACE|FATAL)\]/;

function parseLevel(line: string): Severity | null {
  const m = LEVEL_RE.exec(line);
  if (!m) return null;
  return m[1].toLowerCase() as Severity;
}

export function tagWithSeverity(lines: string[], startLine = 0): TaggedLine[] {
  let current: Severity = 'other';
  return lines.map((text, i) => {
    const parsed = parseLevel(text);
    if (parsed) current = parsed;
    return { text, level: current, index: startLine + i };
  });
}

const STACK_FRAME_RE = /^\s*at\s+[\w.$<>[\]]/;
const FOLD_THRESHOLD = 5;

export function groupStackFolds(lines: TaggedLine[]): RenderUnit[] {
  const out: RenderUnit[] = [];
  let i = 0;
  while (i < lines.length) {
    if (STACK_FRAME_RE.test(lines[i].text)) {
      const start = i;
      while (i < lines.length && STACK_FRAME_RE.test(lines[i].text)) {
        i += 1;
      }
      const frames = lines.slice(start, i);
      if (frames.length >= FOLD_THRESHOLD) {
        out.push({
          kind: 'fold',
          level: frames[0].level,
          firstFrame: frames[0].text,
          hiddenFrames: frames.slice(1).map((f) => f.text),
        });
      } else {
        for (const f of frames) {
          out.push({ kind: 'line', text: f.text, level: f.level, index: f.index });
        }
      }
      continue;
    }
    out.push({ kind: 'line', text: lines[i].text, level: lines[i].level, index: lines[i].index });
    i += 1;
  }
  return out;
}

// Leading MC log timestamp: `[HH:MM:SS]` or `[HH:MM:SS.mmm]`.
const LEADING_TIMESTAMP_RE = /^\s*\[\d{1,2}:\d{2}:\d{2}(?:[.,]\d{1,3})?\]\s*/;

/** Minimum run length before a repeat collapses. Two adjacent duplicates read
 *  fine; three is where a wall of spam starts. */
const REPEAT_THRESHOLD = 3;

/**
 * Comparison key for repeat detection: the line minus its leading timestamp.
 *
 * Stripping the timestamp is what makes this work at all — every line in a real
 * MC log carries a distinct `[HH:MM:SS]`, so whole-line equality would never
 * match the very spam this targets. Nothing else is normalised; in particular
 * thread names stay, because `Worker-Main-1` and `Worker-Main-2` are genuinely
 * different producers and folding them together would hide information.
 */
export function repeatKey(text: string): string {
  return text.replace(LEADING_TIMESTAMP_RE, '');
}

/**
 * Collapse runs of `threshold`+ consecutive `line` units that say the same
 * thing (per `repeatKey`) into a single `repeat` unit.
 *
 * A `fold` unit terminates a run: folds are already a collapse, and nesting one
 * inside the other buys nothing. Shorter runs pass through untouched.
 *
 * All members of a run share a severity by construction — identical text parses
 * to the same level, and a continuation line inherits from the same predecessor
 * — so the caller may filter a `repeat` by its `level` without loss.
 */
export function collapseRepeats(units: RenderUnit[], threshold = REPEAT_THRESHOLD): RenderUnit[] {
  const out: RenderUnit[] = [];
  let i = 0;
  while (i < units.length) {
    const first = units[i];
    if (first.kind !== 'line') {
      out.push(first);
      i += 1;
      continue;
    }
    const key = repeatKey(first.text);
    let end = i + 1;
    while (end < units.length) {
      const next = units[end];
      if (next.kind !== 'line' || repeatKey(next.text) !== key) break;
      end += 1;
    }
    const run = end - i;
    if (run >= threshold) {
      const hiddenLines: string[] = [];
      for (let j = i + 1; j < end; j++) {
        const u = units[j];
        if (u.kind === 'line') hiddenLines.push(u.text);
      }
      out.push({
        kind: 'repeat',
        text: first.text,
        level: first.level,
        index: first.index,
        count: run,
        hiddenLines,
      });
    } else {
      for (let j = i; j < end; j++) out.push(units[j]);
    }
    i = end;
  }
  return out;
}

const CRASH_HEADER_RE = /^----\s*Minecraft Crash Report\s*----/m;
const SECTION_MARKER_RE = /^-- (.+) --\s*$/;

export function maybeParseCrashReport(body: string): CrashSection[] | null {
  if (!CRASH_HEADER_RE.test(body)) return null;
  const lines = body.split('\n');
  const sections: CrashSection[] = [];
  let currentTitle = 'Head';
  let currentLines: string[] = [];
  let currentStart = 0;
  for (let i = 0; i < lines.length; i++) {
    const m = SECTION_MARKER_RE.exec(lines[i]);
    if (m) {
      sections.push({
        title: currentTitle,
        body: currentLines.join('\n'),
        startLine: currentStart,
      });
      currentTitle = m[1].trim();
      currentLines = [];
      currentStart = i + 1;
    } else {
      currentLines.push(lines[i]);
    }
  }
  sections.push({ title: currentTitle, body: currentLines.join('\n'), startLine: currentStart });
  return sections;
}

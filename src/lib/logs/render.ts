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
      kind: 'fold';
      level: Severity;
      firstFrame: string;
      hiddenFrames: string[];
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

export function tagWithSeverity(lines: string[], startIndex = 0): TaggedLine[] {
  let current: Severity = 'other';
  return lines.map((text, i) => {
    const parsed = parseLevel(text);
    if (parsed) current = parsed;
    return { text, level: current, index: startIndex + i };
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

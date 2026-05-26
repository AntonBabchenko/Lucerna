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
}

export type RenderUnit =
  | { kind: 'line'; text: string; level: Severity }
  | {
      kind: 'fold';
      level: Severity;
      firstFrame: string;
      hiddenFrames: string[];
    };

export interface CrashSection {
  title: string;
  body: string;
}

const LEVEL_RE = /\[[^/\]]+\/(INFO|WARN|ERROR|DEBUG|TRACE|FATAL)\]/;

function parseLevel(line: string): Severity | null {
  const m = LEVEL_RE.exec(line);
  if (!m) return null;
  return m[1].toLowerCase() as Severity;
}

export function tagWithSeverity(lines: string[]): TaggedLine[] {
  let current: Severity = 'other';
  return lines.map((text) => {
    const parsed = parseLevel(text);
    if (parsed) current = parsed;
    return { text, level: current };
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
          out.push({ kind: 'line', text: f.text, level: f.level });
        }
      }
      continue;
    }
    out.push({ kind: 'line', text: lines[i].text, level: lines[i].level });
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
  for (const line of lines) {
    const m = SECTION_MARKER_RE.exec(line);
    if (m) {
      sections.push({ title: currentTitle, body: currentLines.join('\n') });
      currentTitle = m[1].trim();
      currentLines = [];
    } else {
      currentLines.push(line);
    }
  }
  sections.push({ title: currentTitle, body: currentLines.join('\n') });
  return sections;
}

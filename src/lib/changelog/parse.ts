import type { Changelog, ChangelogSection, ChangelogVersion, SectionKind } from './types';

// `## [version] — date` — separator may be an em dash, en dash, or hyphen;
// the date group is optional (e.g. `## [Unreleased]`).
const VERSION_RE = /^##\s+\[([^\]]+)\]\s*(?:[—–-]\s*(.+?))?\s*$/;
const SECTION_RE = /^###\s+(.+?)\s*$/;
const BULLET_RE = /^[-*]\s+(.+)$/;
const LINKREF_RE = /^\[([^\]]+)\]:\s*(\S+)/;

const KNOWN: readonly SectionKind[] = [
  'added',
  'changed',
  'fixed',
  'deprecated',
  'removed',
  'security',
];

function toKind(heading: string): SectionKind {
  const h = heading.trim().toLowerCase();
  return (KNOWN as readonly string[]).includes(h) ? (h as SectionKind) : 'other';
}

/**
 * Parse Keep-a-Changelog Markdown into a structured model. Tolerant by
 * design: unrecognized input yields whatever could be recognized (in the
 * worst case an empty array) and never throws.
 */
export function parseChangelog(md: string): Changelog {
  const lines = md.split(/\r?\n/);

  // Pass 1 — collect link-reference URLs (label → url) from anywhere.
  const urls = new Map<string, string>();
  for (const line of lines) {
    const m = LINKREF_RE.exec(line);
    if (m) urls.set(m[1].trim(), m[2].trim());
  }

  const versions: ChangelogVersion[] = [];
  let version: ChangelogVersion | null = null;
  let section: ChangelogSection | null = null;
  let inItem = false;

  const flushSection = (): void => {
    if (version && section) version.sections.push(section);
    section = null;
    inItem = false;
  };
  const flushVersion = (): void => {
    flushSection();
    if (version) versions.push(version);
    version = null;
  };

  for (const line of lines) {
    const v = VERSION_RE.exec(line);
    if (v) {
      flushVersion();
      version = { version: v[1].trim(), date: v[2] ? v[2].trim() : null, url: null, sections: [] };
      continue;
    }
    if (!version) continue; // skip intro before the first version heading

    const s = SECTION_RE.exec(line);
    if (s) {
      flushSection();
      const heading = s[1].trim();
      section = { kind: toKind(heading), heading, items: [] };
      continue;
    }

    if (LINKREF_RE.test(line)) {
      inItem = false;
      continue;
    }

    const b = BULLET_RE.exec(line);
    if (b && section) {
      section.items.push(b[1].trim());
      inItem = true;
      continue;
    }

    if (line.trim() === '') {
      inItem = false;
      continue;
    }

    // A wrapped continuation line of the current bullet.
    if (inItem && section && section.items.length > 0) {
      section.items[section.items.length - 1] += ` ${line.trim()}`;
    }
  }
  flushVersion();

  for (const ver of versions) ver.url = urls.get(ver.version) ?? null;
  return versions;
}

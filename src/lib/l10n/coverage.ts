// Pure view logic for the translation-coverage surfaces. No DOM, no IPC —
// unit-tested in isolation.
import type { NamespaceCoverage } from '$lib/ipc/bindings';

export type CoverageTone = 'ok' | 'partial' | 'none';

/** Percent covered for one namespace; a namespace with no English keys counts
 *  as complete, because there is nothing there to translate. */
export function namespacePercent(row: NamespaceCoverage): number {
  if (row.totalKeys === 0) return 100;
  return Math.floor(((row.fromMod + row.overridden) * 100) / row.totalKeys);
}

/** Zero is deliberately its own tone: nothing is wrong, it is just untranslated. */
export function coverageTone(percent: number): CoverageTone {
  if (percent >= 100) return 'ok';
  if (percent <= 0) return 'none';
  return 'partial';
}

/** Least translated first, then largest first — the work to do floats to the top. */
export function sortNamespaces(rows: NamespaceCoverage[]): NamespaceCoverage[] {
  return [...rows].sort((a, b) => {
    const d = namespacePercent(a) - namespacePercent(b);
    return d !== 0 ? d : b.totalKeys - a.totalKeys;
  });
}

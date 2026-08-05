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

/** The orders the sidebar offers. `leastCovered` is the historical default and
 *  stays it: the work to do floats to the top. */
export type NamespaceSort = 'leastCovered' | 'mostCovered' | 'name' | 'mostKeys' | 'mine';

export const NAMESPACE_SORTS: NamespaceSort[] = [
  'leastCovered',
  'mostCovered',
  'name',
  'mostKeys',
  'mine',
];

/** Least translated first, then largest first — the work to do floats to the top. */
export function sortNamespaces(
  rows: NamespaceCoverage[],
  order: NamespaceSort = 'leastCovered',
): NamespaceCoverage[] {
  const byKeysDesc = (a: NamespaceCoverage, b: NamespaceCoverage) => b.totalKeys - a.totalKeys;
  return [...rows].sort((a, b) => {
    switch (order) {
      case 'name':
        return a.namespace.localeCompare(b.namespace);
      case 'mostKeys':
        return byKeysDesc(a, b) || a.namespace.localeCompare(b.namespace);
      case 'mostCovered': {
        const d = namespacePercent(b) - namespacePercent(a);
        return d !== 0 ? d : byKeysDesc(a, b);
      }
      // Deliberately NOT a rearrangement of coverage. Coverage is the SUM of
      // what the mod's authors did and what the user did, which answers "where
      // should I work next" and cannot answer "where did I already work" — the
      // question after a whole-instance pre-fill writes hundreds of strings.
      case 'mine': {
        const d = b.overridden - a.overridden;
        return d !== 0 ? d : a.namespace.localeCompare(b.namespace);
      }
      default: {
        const d = namespacePercent(a) - namespacePercent(b);
        return d !== 0 ? d : byKeysDesc(a, b);
      }
    }
  });
}

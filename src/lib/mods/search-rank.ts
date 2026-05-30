// Title-first re-ranking for the mod / modpack browser.
//
// Both Modrinth and CurseForge respect the chosen `sort` parameter, which
// is great for "show me the most-downloaded mod" — but when the user has
// also typed a query, the most-downloaded hit is often a popular mod that
// merely mentions the query in its description (e.g. "Xaero's Minimap …
// lets you create waypoints"), while the actual name match (the "Create"
// engineering mod) gets pushed down.
//
// `prioritizeByTitle` partitions the hit list into [title-matched, others]
// and concatenates them, preserving the platform's intra-partition order
// so the chosen sort still wins within each partition. Empty query is a
// no-op: the platform's ranking is left alone.

export function prioritizeByTitle<T>(
  hits: readonly T[],
  query: string,
  getTitle: (hit: T) => string,
): T[] {
  const q = query.trim().toLowerCase();
  if (q === '') return [...hits];
  const titleMatched: T[] = [];
  const rest: T[] = [];
  for (const h of hits) {
    if (getTitle(h).toLowerCase().includes(q)) titleMatched.push(h);
    else rest.push(h);
  }
  return [...titleMatched, ...rest];
}

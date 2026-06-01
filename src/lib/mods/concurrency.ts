// Run `fn` over `items` with a bounded number of concurrent calls, preserving
// input order in the result. Used to enrich installed mods with their platform
// summaries without firing one request per mod all at once — a large parallel
// burst intermittently trips platform rate-limits / transient failures, which
// made rows flicker into a degraded "details unavailable" state. A small pool
// keeps the load steady and the display stable.
export async function mapLimit<T, R>(
  items: readonly T[],
  limit: number,
  fn: (item: T, index: number) => Promise<R>,
): Promise<R[]> {
  const results = new Array<R>(items.length);
  let next = 0;
  const worker = async () => {
    while (next < items.length) {
      const i = next++;
      results[i] = await fn(items[i], i);
    }
  };
  const pool = Math.max(1, Math.min(limit, items.length));
  await Promise.all(Array.from({ length: pool }, worker));
  return results;
}

import type { LoaderKind } from '$lib/ipc/bindings';

// The "narrowing" facets shared by both browsers. `showInstalled` is
// optional: the mod browser passes it (default true); the modpack
// browser omits it entirely. `source` is intentionally NOT here — it
// always has a value (default Modrinth), so it is a context switch, not
// a narrowing filter, and must not show in the active-filter count.
export type FilterFacets = {
  loader: LoaderKind | '';
  mc: string;
  showInstalled?: boolean;
};

// Number of active narrowing filters — drives the toolbar's "Clear all" button.
// (The chip row that once rendered these was removed in favour of inline
// controls, so only the count survives.)
export function activeCount(f: FilterFacets): number {
  let n = 0;
  if (f.loader) n += 1;
  if (f.mc) n += 1;
  if (f.showInstalled === false) n += 1;
  return n;
}

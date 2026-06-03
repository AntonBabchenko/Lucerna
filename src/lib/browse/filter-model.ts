import type { LoaderKind } from '$lib/ipc/bindings';

// The "narrowing" facets shared by both browsers. `showInstalled` is
// optional: the mod browser passes it (default true); the modpack
// browser omits it entirely. `source` is intentionally NOT here — it
// always has a value (default Modrinth), so it is a context switch, not
// a narrowing filter, and must not show in the badge count or chip row.
export type FilterFacets = {
  loader: LoaderKind | '';
  mc: string;
  showInstalled?: boolean;
};

export type FilterChipKey = 'loader' | 'mc' | 'showInstalled';
export type FilterChip = { key: FilterChipKey; label: string };

const LOADER_LABELS: Record<string, string> = {
  fabric: 'Fabric',
  quilt: 'Quilt',
  forge: 'Forge',
  neoforge: 'NeoForge',
  vanilla: 'Vanilla',
};

// Overridable chip labels. Loader names (brand) and the MC version (a number)
// are language-neutral, but the "Installed hidden" label is prose and must be
// localized — the caller passes the translated string. Defaults keep the model
// usable without i18n (e.g. the badge count, which ignores labels).
export type ChipLabels = { installedHidden?: string };

// Build the ordered list of active-filter chips. This is the single
// source of truth: the badge count is just its length, and the chip row
// renders it directly.
export function activeChips(f: FilterFacets, labels: ChipLabels = {}): FilterChip[] {
  const chips: FilterChip[] = [];
  if (f.loader) chips.push({ key: 'loader', label: LOADER_LABELS[f.loader] ?? f.loader });
  if (f.mc) chips.push({ key: 'mc', label: f.mc });
  if (f.showInstalled === false)
    chips.push({ key: 'showInstalled', label: labels.installedHidden ?? 'Installed hidden' });
  return chips;
}

export function activeCount(f: FilterFacets): number {
  return activeChips(f).length;
}

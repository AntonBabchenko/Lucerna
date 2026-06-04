// Persisted filter state for the Modpack browse view — survives modal
// close/reopen across the app session.  Mirrors the browser-prefs.svelte.ts
// singleton pattern: $state fields + $effect.root write-through to
// localStorage.  `query` (the search box text) is intentionally NOT
// persisted here; it resets to '' on each open.

import type { LoaderKind, ModpackSort, ModSource } from '$lib/ipc/bindings';

const KEY = 'lucerna.modpackBrowseState';

const MOD_SOURCES: ModSource[] = ['modrinth', 'curseforge', 'ftb'];
const MODPACK_SORTS: ModpackSort[] = ['relevance', 'downloads', 'newest', 'updated'];
// LoaderKind values that are valid modpack loader filters (plus '' for "any").
const LOADER_KINDS: LoaderKind[] = ['vanilla', 'fabric', 'quilt', 'forge', 'neoforge'];

const DEFAULTS = {
  source: 'modrinth' as ModSource,
  mcFilter: '',
  loaderFilter: '' as LoaderKind | '',
  sortChoice: 'relevance' as ModpackSort,
};

function loadSource(): ModSource {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return DEFAULTS.source;
    const parsed = JSON.parse(raw);
    return MOD_SOURCES.includes(parsed?.source) ? (parsed.source as ModSource) : DEFAULTS.source;
  } catch {
    return DEFAULTS.source;
  }
}

function loadMcFilter(): string {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return DEFAULTS.mcFilter;
    const parsed = JSON.parse(raw);
    return typeof parsed?.mcFilter === 'string' ? parsed.mcFilter : DEFAULTS.mcFilter;
  } catch {
    return DEFAULTS.mcFilter;
  }
}

function loadLoaderFilter(): LoaderKind | '' {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return DEFAULTS.loaderFilter;
    const parsed = JSON.parse(raw);
    const v = parsed?.loaderFilter;
    if (v === '') return '';
    return LOADER_KINDS.includes(v) ? (v as LoaderKind) : DEFAULTS.loaderFilter;
  } catch {
    return DEFAULTS.loaderFilter;
  }
}

function loadSortChoice(): ModpackSort {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return DEFAULTS.sortChoice;
    const parsed = JSON.parse(raw);
    return MODPACK_SORTS.includes(parsed?.sortChoice)
      ? (parsed.sortChoice as ModpackSort)
      : DEFAULTS.sortChoice;
  } catch {
    return DEFAULTS.sortChoice;
  }
}

class ModpackBrowseState {
  source = $state<ModSource>(loadSource());
  mcFilter = $state<string>(loadMcFilter());
  loaderFilter = $state<LoaderKind | ''>(loadLoaderFilter());
  sortChoice = $state<ModpackSort>(loadSortChoice());

  constructor() {
    try {
      // Module singleton — lives for the whole app session; the $effect.root
      // is intentionally never disposed (same rationale as BrowserPrefs).
      $effect.root(() => {
        $effect(() => {
          try {
            localStorage.setItem(
              KEY,
              JSON.stringify({
                source: this.source,
                mcFilter: this.mcFilter,
                loaderFilter: this.loaderFilter,
                sortChoice: this.sortChoice,
              }),
            );
          } catch {
            /* localStorage unavailable — non-fatal */
          }
        });
      });
    } catch {
      /* $effect.root requires a reactive context; persistence is best-effort */
    }
  }
}

export const modpackBrowseState = new ModpackBrowseState();

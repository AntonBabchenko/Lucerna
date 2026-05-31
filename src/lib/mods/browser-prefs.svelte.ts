// Shared, persisted UI preferences for the Mod + Modpack browsers:
// results-per-page and grid/list layout. Single source of truth so both
// browsers (and later the resourcepack/shader + CF browsers) stay in sync.
// Persisted to localStorage; can migrate into Settings (#6) later.

export type PageSize = 20 | 50 | 100;
export type Layout = 'grid' | 'list';

export const PAGE_SIZES: PageSize[] = [20, 50, 100];

const KEY = 'lucerna.browserPrefs';
const DEFAULTS = { pageSize: 20 as PageSize, layout: 'grid' as Layout };

export function loadPrefs(): { pageSize: PageSize; layout: Layout } {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return { ...DEFAULTS };
    const parsed = JSON.parse(raw);
    const pageSize = (PAGE_SIZES as number[]).includes(parsed?.pageSize)
      ? (parsed.pageSize as PageSize)
      : DEFAULTS.pageSize;
    const layout: Layout = parsed?.layout === 'list' ? 'list' : 'grid';
    return { pageSize, layout };
  } catch {
    return { ...DEFAULTS };
  }
}

const initial = loadPrefs();

class BrowserPrefs {
  pageSize = $state<PageSize>(initial.pageSize);
  layout = $state<Layout>(initial.layout);

  constructor() {
    try {
      $effect.root(() => {
        $effect(() => {
          try {
            localStorage.setItem(
              KEY,
              JSON.stringify({ pageSize: this.pageSize, layout: this.layout }),
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

export const browserPrefs = new BrowserPrefs();

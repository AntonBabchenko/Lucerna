// Translations of CHANGELOG.md: one Keep-a-Changelog mirror per locale at
// ./locales/<code>.md, the same BCP-47 codes as src/lib/i18n/locales/<code>.json
// and discovered the same way (no registration). Each file is its own Vite
// chunk, loaded on demand — a language costs nothing until it is selected.
// English is not a locale here: CHANGELOG.md itself is the source, embedded by
// ./source.ts.
import { parseChangelog } from './parse';
import type { Changelog } from './types';

/** The language CHANGELOG.md is written in. */
export const CHANGELOG_SOURCE_LOCALE = 'en';

type RawLoader = () => Promise<string>;

const LOADERS: Readonly<Record<string, RawLoader>> = Object.fromEntries(
  Object.entries(import.meta.glob<string>('./locales/*.md', { query: '?raw', import: 'default' }))
    .map(([path, load]) => [path.match(/\/([^/]+)\.md$/)?.[1] ?? '', load] as const)
    .filter(([code]) => code !== ''),
);

/** Sorted codes that have a changelog translation in the bundle. */
export const CHANGELOG_LOCALES: readonly string[] = Object.keys(LOADERS).sort();

/**
 * The three things a load can come back as. `missing` and `failed` are kept
 * apart on purpose: the panel tells the user "not translated yet" for one and
 * "could not be loaded" for the other, because they are not the same event.
 */
export type LocaleLoad =
  | { status: 'ready'; entries: Changelog }
  | { status: 'missing' }
  | { status: 'failed'; error: unknown };

/** Load and parse the changelog for `code`. Never throws. */
export async function loadChangelogLocale(
  code: string,
  loaders: Readonly<Record<string, RawLoader>> = LOADERS,
): Promise<LocaleLoad> {
  // Own keys only: a code like "constructor" must read as missing, not as a
  // function inherited from Object.prototype.
  const load = Object.hasOwn(loaders, code) ? loaders[code] : undefined;
  if (!load) return { status: 'missing' };
  try {
    const entries = parseChangelog(await load());
    // The parser never throws; a file it cannot make sense of yields nothing.
    // That is a broken translation, not an empty one.
    if (entries.length === 0) {
      return {
        status: 'failed',
        error: new Error(`changelog locale "${code}" contains no version headings`),
      };
    }
    return { status: 'ready', entries };
  } catch (error) {
    return { status: 'failed', error };
  }
}

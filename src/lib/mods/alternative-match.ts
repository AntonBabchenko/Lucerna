/**
 * Normalize a mod name for fuzzy cross-source matching: lowercase and drop
 * every character that is not a letter or digit. Cross-source identity can
 * only be guessed by name (the blocked entry has no usable hash), so this is
 * a *hint* for the UI, never an auto-install signal.
 */
export function normalizeModName(name: string): string {
  return name.toLowerCase().replace(/[^a-z0-9]/g, '');
}

/**
 * True when two mod names plausibly refer to the same mod: their normalized
 * forms are equal, or one contains the other (e.g. "JEI" vs
 * "JEI (Just Enough Items)"). Requires at least 3 normalized chars on the
 * shorter side to avoid spurious substring hits.
 */
export function isLikelyMatch(a: string, b: string): boolean {
  const na = normalizeModName(a);
  const nb = normalizeModName(b);
  if (!na || !nb) return false;
  if (na === nb) return true;
  const [short, long] = na.length <= nb.length ? [na, nb] : [nb, na];
  return short.length >= 3 && long.includes(short);
}

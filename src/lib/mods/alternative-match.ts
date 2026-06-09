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

/**
 * Decide whether a Modrinth search hit plausibly IS the mod the user searched
 * for, by comparing the query against the hit's display name AND its slug.
 * Modrinth's relevance search returns loosely-related hits (e.g. by a
 * similarly-named author) that are NOT the mod — those must be excluded so the
 * dialog never offers an unrelated mod as a one-click "alternative". Matching
 * the slug catches mods whose display name differs from their id (e.g. the
 * "jei" slug for "Just Enough Items").
 */
export function isPlausibleAlternative(
  query: string,
  candidateName: string,
  candidateSlug: string | null,
): boolean {
  return (
    isLikelyMatch(query, candidateName) ||
    (candidateSlug !== null && isLikelyMatch(query, candidateSlug))
  );
}

/**
 * Derive a Modrinth-search-friendly query from a blocked mod's name, which is
 * often a raw jar filename (e.g. `moreoverlays-1.21.5-mc1.19.2.jar`). Strips a
 * trailing archive extension, then keeps the leading run of name tokens up to
 * the first version/loader/MC token — so `moreoverlays-1.21.5-mc1.19.2.jar`
 * becomes `moreoverlays` and `create_train_parts-0.4.0-….jar` becomes
 * `create train parts`. A clean display name (no extension, no version tokens)
 * passes through with only separators normalised to spaces.
 */
export function deriveSearchQuery(name: string): string {
  const noExt = name.replace(/\.(jar|zip|litemod|disabled)$/i, '');
  const tokens = noExt.split(/[\s_-]+/).filter(Boolean);
  const isJunk = (tok: string): boolean =>
    /^\d/.test(tok) || // version-like: starts with a digit (1.21.5, 216)
    /^mc\d/i.test(tok) || // mc1.19.2
    /^v\d/i.test(tok) || // v1.2
    ['forge', 'fabric', 'neoforge', 'quilt', 'mc'].includes(tok.toLowerCase());
  const kept: string[] = [];
  for (const tok of tokens) {
    if (isJunk(tok)) break; // stop at the first version/loader token
    kept.push(tok);
  }
  const query = kept.join(' ').trim();
  // Fallback: if everything looked like junk, search the cleaned stem rather
  // than an empty string.
  return query || noExt.replace(/[\s_-]+/g, ' ').trim();
}

// Cross-platform mod name normalisation.
//
// Both Modrinth and CurseForge often append loader/edition suffixes to the
// same mod's project name inconsistently ("Cloth Config API (Forge)",
// "Sodium Fabric Mod", …). `nameKey` strips those noise words and reduces
// the name to a lowercase alphanumeric string so the Browse view can match
// an installed Modrinth entry against a CurseForge card (and vice-versa)
// without a false-positive on coincidentally-similar names.

export const NAME_NOISE = /\b(api|fabric|forge|neoforge|quilt|mod|edition|for)\b/g;

/**
 * Reduce a display name to a comparison key.
 *
 * Steps:
 * 1. Lowercase the input.
 * 2. Remove loader / edition noise words.
 * 3. Keep only `[a-z0-9]` characters.
 *
 * Returns `''` for names that collapse to nothing (e.g. "Forge API") —
 * callers treat an empty key as "no match" rather than a universal match.
 */
export function nameKey(s: string): string {
  return s
    .toLowerCase()
    .replace(NAME_NOISE, '')
    .replace(/[^a-z0-9]+/g, '');
}

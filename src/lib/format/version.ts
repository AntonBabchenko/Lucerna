/**
 * A content version as the lists show it: «v» before a version that starts with a digit
 * («v0.6.0»), nothing before any other — «mc1.21.1-0.13.1», «beta-3», an already-prefixed «v2.1»,
 * the «?» of an unknown one — where a glued «v» would read as part of the version (plan §5b V1).
 * The one place a «v» is put before a mod's, a modpack's or a data pack's version: the installed
 * row, its update badge and the update review; the Overview's pack card and its update badge, the
 * imported pack's card and drawer, the import picker; the library label on Browse. No string may
 * write a «v» of its own before a version (tests/i18n-parity.test.ts): «vv2.1.0» (plan §5b V2).
 */
export function displayVersion(version: string): string {
  return /^\d/.test(version) ? `v${version}` : version;
}

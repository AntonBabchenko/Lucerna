// Which build this is, in words (Settings → About). Two forms: the line shown
// under the version, in the interface language, and the block "Copy version
// info" puts on the clipboard — English and fixed, because it goes into an
// English issue form.
import type { Translate } from '$lib/i18n';
import type { BuildInfo } from '$lib/ipc/bindings';

/** `v0.25.0-rc.2 · a1b2c3d · windows x86_64`, or `Local build · …`. */
export function buildLine(_info: BuildInfo, _t: Translate): string {
  // STUB (red).
  return '';
}

/**
 * The paste-ready block. `info` is null when the build info could not be read:
 * the block still says which version the page itself was built with, and says
 * `unknown` rather than leaving a field blank.
 */
export function versionInfoBlock(_info: BuildInfo | null, _pkgVersion: string): string {
  // STUB (red).
  return '';
}

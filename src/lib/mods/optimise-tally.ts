/** One Optimise install's answer, as `installModWithDeps` returns it. */
export type OptimiseInstallResult = { status: 'ok' } | { status: 'error'; error: { kind: string } };

/** What a run of Optimise installs comes to, for its toast. A refusal because the profile already
 *  has the mod — under another project, or the same mod-id from the other platform (spec
 *  2026-10-09 same-mod-by-id D8) — is skipped, not failed. */
export function tallyOptimise(results: readonly OptimiseInstallResult[]): {
  installed: number;
  failed: number;
  skipped: number;
} {
  let installed = 0;
  let failed = 0;
  let skipped = 0;
  for (const r of results) {
    if (r.status === 'ok') installed += 1;
    else if (r.error.kind === 'mods_already_installed') skipped += 1;
    else failed += 1;
  }
  return { installed, failed, skipped };
}

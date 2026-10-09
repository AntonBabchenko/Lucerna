// Build the per-mod dependency lines for the post-install success toast.
//
// Extracted from ModBrowseView's DependencyDialog onConfirm success branch.
// Pure function — no IPC, no side effects.

import type { ModVersion, TaskDetail } from '$lib/ipc/bindings';
import type { DepItem, OptionalItem } from '$lib/mods/dep-prompt';

/**
 * Build the deduped list of dependency project names shown in the install
 * success toast, in the same order the DependencyDialog displayed them.
 *
 * Dedup key: `${source}:${project_id}` — first-seen wins.
 *
 * Order:
 * 1. All required deps (in their list order).
 * 2. For each chosen optional (in chosenOptional order):
 *    a. The optional itself.
 *    b. Its transitive requires (in sub-list order).
 *
 * A chosenOptional entry with no matching prompt.optional entry is skipped
 * (defensive; the dialog should only pass versions it knows about).
 *
 * A dependency whose file the install left out (`skipped`, by file name — the
 * profile already had its mod, spec 2026-10-09 same-mod-by-id D3) is never named
 * as installed: its line is `alreadyThere(name)`, in the same place.
 */
export function buildInstalledDepLines(
  prompt: { required: DepItem[]; optional: OptionalItem[] },
  chosenOptional: ModVersion[],
  skipped: ReadonlySet<string> = new Set(),
  alreadyThere: (name: string) => string = (name) => name,
): string[] {
  const seen = new Set<string>();
  const depLines: string[] = [];

  const pushDep = (name: string, version: ModVersion) => {
    const key = `${version.source}:${version.project_id}`;
    if (!seen.has(key)) {
      seen.add(key);
      depLines.push(skipped.has(version.primary_file.filename) ? alreadyThere(name) : name);
    }
  };

  for (const d of prompt.required) {
    pushDep(d.projectName, d.version);
  }

  for (const v of chosenOptional) {
    const o = prompt.optional.find(
      (x) =>
        x.version.source === v.source &&
        x.version.project_id === v.project_id &&
        x.version.version_id === v.version_id,
    );
    if (!o) continue;
    pushDep(o.projectName, o.version);
    for (const r of o.requires) {
      pushDep(r.projectName, r.version);
    }
  }

  return depLines;
}

/** The files an install left out because the profile already had their mod: its report's
 *  `skipped` rows (spec 2026-10-09 same-mod-by-id D3), by file name. */
export function skippedFiles(details: readonly TaskDetail[]): Set<string> {
  return new Set(
    details
      .filter((d) => d.outcome.kind === 'skipped')
      .map((d) => d.install_path.replace(/^mods\//, '')),
  );
}
